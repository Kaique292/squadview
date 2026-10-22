//! Vídeo desenhado **por baixo** da interface, pelo DirectComposition (ADR 0021).
//!
//! Cada vídeo é uma [`Layer`]: um visual com um swap chain de composição, preso à árvore de uma
//! janela (`CreateTargetForHwnd(janela, topmost = false)`). Com `topmost = false`, a árvore fica por
//! baixo das janelas filhas, e a WebView2 (transparente onde está o vídeo) desenha por cima com
//! mistura de verdade: selos, menus, sombras e animações aparecem sobre o vídeo, sem recortes.
//!
//! Um dispositivo do DirectComposition só, para o app inteiro: um visual só pode ir para a árvore de
//! outra janela (o picture in picture) se as duas forem do mesmo dispositivo. As interfaces dele não
//! são `Send`: tudo passa pela trava de [`Comp`], de qualquer thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use sv_gpu::{Gpu, Rect, Size, fit_inside};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionRectangleClip, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
};
use windows::core::Result;

use crate::window::Bounds;

/// Uma janela que recebe vídeos: o alvo de composição dela e a raiz onde os vídeos entram.
struct Host {
    _target: IDCompositionTarget,
    root: IDCompositionVisual,
    /// Quantos vídeos estão nela (sem nenhum, o alvo é solto: a janela pode fechar).
    layers: usize,
}

/// O dispositivo do app e as janelas com vídeo. Só se mexe com a trava.
struct Comp {
    device: IDCompositionDevice,
    hosts: HashMap<isize, Host>,
}

// SAFETY: as interfaces do DirectComposition só são usadas com a trava de `COMP` (uma thread por
// vez), e o dispositivo é seguro de usar de qualquer thread, desde que não em paralelo.
unsafe impl Send for Comp {}

static COMP: OnceLock<std::result::Result<Mutex<Comp>, String>> = OnceLock::new();

/// O dispositivo do DirectComposition do app (criado na primeira vez).
fn comp() -> Result<MutexGuard<'static, Comp>> {
    let comp = COMP.get_or_init(|| {
        // SAFETY: cria o dispositivo sem DXGI (o conteúdo dos visuais é swap chain, não superfície).
        unsafe { DCompositionCreateDevice::<_, IDCompositionDevice>(None::<&IDXGIDevice>) }
            .map(|device| {
                Mutex::new(Comp {
                    device,
                    hosts: HashMap::new(),
                })
            })
            .map_err(|e| e.message())
    });
    match comp {
        Ok(m) => Ok(m.lock().unwrap_or_else(|e| e.into_inner())),
        Err(message) => Err(windows::core::Error::new(windows::core::HRESULT(-1), message.clone())),
    }
}

impl Comp {
    /// A raiz da janela `hwnd` (cria o alvo na primeira vez).
    fn root(&mut self, hwnd: isize) -> Result<IDCompositionVisual> {
        if let Some(host) = self.hosts.get_mut(&hwnd) {
            host.layers += 1;
            return Ok(host.root.clone());
        }
        // SAFETY: a janela é do nosso processo e está viva (quem chama garante).
        let (target, root) = unsafe {
            let target = self.device.CreateTargetForHwnd(HWND(hwnd as *mut _), false)?;
            let root = self.device.CreateVisual()?;
            target.SetRoot(&root)?;
            (target, root)
        };
        self.hosts.insert(
            hwnd,
            Host {
                _target: target,
                root: root.clone(),
                layers: 1,
            },
        );
        Ok(root)
    }

    /// Um vídeo saiu da janela `hwnd`: sem nenhum, solta o alvo.
    fn release(&mut self, hwnd: isize) {
        let empty = self.hosts.get_mut(&hwnd).is_some_and(|h| {
            h.layers = h.layers.saturating_sub(1);
            h.layers == 0
        });
        if empty {
            self.hosts.remove(&hwnd);
        }
    }

    fn commit(&self) {
        // SAFETY: dispositivo vivo.
        let _ = unsafe { self.device.Commit() };
    }
}

/// O que a interface pediu e onde a imagem está agora.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// Área do quadro na janela (pixels físicos).
    area: Bounds,
    radius: u32,
    visible: bool,
    /// Tamanho do vídeo (do último quadro).
    video: Size,
}

/// Onde o visual fica: o quadro inteiro (o renderizador centraliza a imagem dentro dele, com faixas
/// pretas se a proporção for outra, como no Discord).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// O vídeo `video` no quadro `area`: o quadro inteiro, onde a página deixa o buraco transparente.
/// `None`: nada a mostrar (área zero, ou nenhum quadro do vídeo ainda).
pub(crate) fn place(area: Bounds, video: Size) -> Option<Placement> {
    let Rect { right, bottom, .. } = fit_inside(video, Size::new(area.width, area.height));
    (right > 0 && bottom > 0).then_some(Placement {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    })
}

struct Visuals {
    visual: IDCompositionVisual,
    clip: IDCompositionRectangleClip,
}

// SAFETY: como `Comp`: só usados com a trava de `COMP`.
unsafe impl Send for Visuals {}
unsafe impl Sync for Visuals {}

struct LayerState {
    layout: Layout,
    /// A janela onde o vídeo está (a do app, ou a do picture in picture).
    host: isize,
    /// O visual está na árvore da janela (escondido: fora dela).
    in_tree: bool,
    /// Onde o visual está agora.
    placed: Option<Placement>,
}

/// Um vídeo desenhado por baixo da interface de uma janela.
pub struct Layer {
    visuals: Visuals,
    state: Mutex<LayerState>,
    /// A janela de origem (o app): é para onde o vídeo volta do picture in picture.
    home: isize,
}

impl std::fmt::Debug for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layer")
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Layer {
    /// Um vídeo na janela `hwnd`, escondido até a interface dizer onde ele fica.
    pub fn new(hwnd: isize) -> Result<Arc<Self>> {
        let mut comp = comp()?;
        // SAFETY: dispositivo vivo; o visual e o recorte são dele.
        let visuals = unsafe {
            let visual = comp.device.CreateVisual()?;
            let clip = comp.device.CreateRectangleClip()?;
            visual.SetClip(&clip)?;
            Visuals { visual, clip }
        };
        comp.root(hwnd)?;
        Ok(Arc::new(Self {
            visuals,
            state: Mutex::new(LayerState {
                layout: Layout {
                    area: Bounds::default(),
                    radius: 0,
                    visible: true,
                    video: Size::new(0, 0),
                },
                host: hwnd,
                in_tree: false,
                placed: None,
            }),
            home: hwnd,
        }))
    }

    fn state(&self) -> MutexGuard<'_, LayerState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Onde fica o quadro do vídeo na janela (pixels físicos). Tamanho zero esconde.
    pub fn set_area(&self, area: Bounds) {
        self.change(|l| l.area = area);
    }

    /// O raio dos cantos do quadro (pixels físicos), para o vídeo acompanhar a borda arredondada.
    pub fn set_radius(&self, radius: u32) {
        self.change(|l| l.radius = radius);
    }

    pub fn set_visible(&self, visible: bool) {
        self.change(|l| l.visible = visible);
    }

    /// O tamanho do vídeo mudou (o primeiro quadro, ou quem transmite trocou a resolução).
    pub fn set_video(&self, video: Size) {
        self.change(|l| l.video = video);
    }

    /// Leva o vídeo para a janela `hwnd` (o picture in picture), ou de volta para a do app (`None`).
    pub fn move_to(&self, hwnd: Option<isize>) -> Result<()> {
        let target = hwnd.unwrap_or(self.home);
        let mut comp = comp()?;
        let mut state = self.state();
        if state.host == target {
            return Ok(());
        }
        self.leave_tree(&comp, &mut state);
        let previous = state.host;
        comp.root(target)?;
        comp.release(previous);
        state.host = target;
        state.placed = None;
        self.apply(&comp, &mut state);
        Ok(())
    }

    /// O tamanho do swap chain: o quadro inteiro (`None`: nada a mostrar).
    pub(crate) fn image_size(&self) -> Option<Size> {
        let state = self.state();
        let l = state.layout;
        place(l.area, l.video).map(|p| Size::new(p.width, p.height))
    }

    /// Cria o swap chain de composição e põe no visual.
    pub(crate) fn swapchain(&self, gpu: &Gpu) -> Result<IDXGISwapChain1> {
        // SAFETY: fábrica DXGI e swap chain de composição no dispositivo da GPU.
        let swapchain = unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                // Cresce no primeiro quadro, para o tamanho da imagem.
                Width: 1,
                Height: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            factory.CreateSwapChainForComposition(&gpu.device, &desc, None)?
        };
        let comp = comp()?;
        // SAFETY: visual vivo; o swap chain passa a ser o conteúdo dele.
        unsafe { self.visuals.visual.SetContent(&swapchain)? };
        comp.commit();
        Ok(swapchain)
    }

    fn change(&self, f: impl FnOnce(&mut Layout)) {
        let Ok(comp) = comp() else { return };
        let mut state = self.state();
        f(&mut state.layout);
        self.apply(&comp, &mut state);
    }

    /// Põe o visual onde a imagem fica (ou tira da árvore, escondido) e manda o DirectComposition
    /// mostrar. Sem mudança, não faz nada.
    fn apply(&self, comp: &Comp, state: &mut LayerState) {
        let l = state.layout;
        let placed = if l.visible { place(l.area, l.video) } else { None };
        let Some(p) = placed else {
            if state.in_tree {
                self.leave_tree(comp, state);
                comp.commit();
            }
            return;
        };
        if state.in_tree && state.placed == Some(p) {
            return;
        }
        let v = &self.visuals;
        let (w, h, r) = (p.width as f32, p.height as f32, l.radius as f32);
        // SAFETY: visual e recorte vivos, com a trava do dispositivo.
        unsafe {
            let _ = v.visual.SetOffsetX2(p.x as f32);
            let _ = v.visual.SetOffsetY2(p.y as f32);
            let _ = v.clip.SetLeft2(0.0);
            let _ = v.clip.SetTop2(0.0);
            let _ = v.clip.SetRight2(w);
            let _ = v.clip.SetBottom2(h);
            for set in [
                IDCompositionRectangleClip::SetTopLeftRadiusX2,
                IDCompositionRectangleClip::SetTopLeftRadiusY2,
                IDCompositionRectangleClip::SetTopRightRadiusX2,
                IDCompositionRectangleClip::SetTopRightRadiusY2,
                IDCompositionRectangleClip::SetBottomLeftRadiusX2,
                IDCompositionRectangleClip::SetBottomLeftRadiusY2,
                IDCompositionRectangleClip::SetBottomRightRadiusX2,
                IDCompositionRectangleClip::SetBottomRightRadiusY2,
            ] {
                let _ = set(&v.clip, r);
            }
            if !state.in_tree
                && let Some(host) = comp.hosts.get(&state.host)
                && host.root.AddVisual(&v.visual, true, None).is_ok()
            {
                state.in_tree = true;
            }
        }
        state.placed = Some(p);
        comp.commit();
    }

    fn leave_tree(&self, comp: &Comp, state: &mut LayerState) {
        if !state.in_tree {
            return;
        }
        if let Some(host) = comp.hosts.get(&state.host) {
            // SAFETY: o visual está nesta raiz.
            let _ = unsafe { host.root.RemoveVisual(&self.visuals.visual) };
        }
        state.in_tree = false;
        state.placed = None;
    }

    /// Está aparecendo agora (na árvore de alguma janela), e onde.
    pub fn placed(&self) -> Option<(isize, Bounds)> {
        let state = self.state();
        state.in_tree.then_some(())?;
        let p = state.placed?;
        Some((
            state.host,
            Bounds {
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
            },
        ))
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        let Ok(mut comp) = comp() else { return };
        let mut state = self.state();
        self.leave_tree(&comp, &mut state);
        let host = state.host;
        drop(state);
        comp.release(host);
        comp.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds { x, y, width, height }
    }

    #[test]
    fn o_video_cobre_o_quadro_inteiro_quando_ha_imagem() {
        // 16:9 num quadro 16:9: ocupa tudo.
        assert_eq!(
            place(area(100, 50, 640, 360), Size::new(1920, 1080)),
            Some(Placement {
                x: 100,
                y: 50,
                width: 640,
                height: 360
            })
        );
        // Janela estreita: o visual cobre o quadro inteiro (as faixas pretas ficam dentro dele, com
        // o canto arredondado), e a página só precisa do buraco do quadro.
        let p = place(area(0, 0, 640, 360), Size::new(1000, 1000)).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (0, 0, 640, 360));
        // Sem quadro ainda, ou sem área: nada a mostrar.
        assert_eq!(place(area(0, 0, 640, 360), Size::new(0, 0)), None);
        assert_eq!(place(area(0, 0, 0, 0), Size::new(1920, 1080)), None);
    }
}
