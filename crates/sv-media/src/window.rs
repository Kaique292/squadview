//! Superfície nativa que mostra o vídeo (flip-model do DXGI, sem passar pela WebView).
//!
//! Dois jeitos de abrir ([`WindowTarget`]):
//! - **embutida**: uma janela filha, sem borda, grudada dentro da janela do app, exatamente sobre a
//!   área que a interface reserva para o stream (como no Discord). A interface informa a posição
//!   com [`WindowControl::set_bounds`].
//! - **solta**: uma janela própria, com título (usada pelo exemplo de terminal).
//!
//! A embutida também vira **picture in picture** ([`WindowControl::set_pip`]): a mesma janela se
//! solta do app e fica pequena, sempre no topo, num canto da tela. Nada muda no desenho (o swap
//! chain é o mesmo): só o pai e o estilo da janela.
//!
//! A janela vive na própria thread (fila de mensagens do Windows). O [`Renderer`] desenha a partir
//! da thread do vídeo: converte a textura NV12 direto no back buffer, centralizada e sem distorcer.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock, mpsc};
use std::thread::JoinHandle;

use sv_gpu::{Converter, Gpu, GpuFrame, Size, fit_inside};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
    IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow, RGN_DIFF, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE, VK_F11,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GWLP_USERDATA, GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW,
    GetWindowRect, HTCAPTION, HTCLIENT, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_ARROW, LoadCursorW, MINMAXINFO,
    MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SET_WINDOW_POS_FLAGS, SW_SHOW, SWP_ASYNCWINDOWPOS,
    SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetParent, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_STYLE, WM_APP, WM_CLOSE,
    WM_DESTROY, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_RBUTTONUP, WM_SIZE, WM_SIZING, WMSZ_BOTTOM, WMSZ_TOP,
    WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_OVERLAPPEDWINDOW, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{HSTRING, Result, w};

use crate::composition::Layer;

/// Onde o vídeo aparece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowTarget {
    /// Janela própria, com título e borda.
    Standalone { title: String },
    /// Dentro da janela do app (`parent` é o HWND dela). Começa escondida até receber a posição.
    Embedded { parent: isize },
    /// Desenhado por baixo da interface da janela `parent`, pelo DirectComposition (ADR 0021): a
    /// interface fica por cima com mistura de verdade, sem recortes. Sem janela própria.
    Composed { parent: isize },
}

/// O que o usuário fez dentro da área do vídeo embutida (na janela solta, a própria janela
/// cuida da tela cheia).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    /// Dois cliques ou F11.
    ToggleFullscreen,
    /// Esc.
    ExitFullscreen,
    /// No picture in picture: dois cliques, Esc ou fechar. Quem decide voltar é o app.
    ExitPip,
    /// Clique no vídeo embutido (pixels físicos, a partir do canto do vídeo). O vídeo fica por
    /// cima da interface e recebe o mouse no lugar dela: o app repassa para a interface.
    Click { x: i32, y: i32 },
    /// Botão direito no vídeo embutido: a interface abre o menu ali.
    ContextMenu { x: i32, y: i32 },
    /// O mouse entrou no vídeo embutido (um aviso por entrada, não um por movimento).
    PointerEnter,
    /// O mouse saiu do vídeo embutido.
    PointerLeave,
}

/// Mensagem para entrar (`wparam` 1) ou sair (0) do picture in picture, tratada na thread da janela.
const WM_APP_PIP: u32 = WM_APP + 1;
/// Menor tamanho do picture in picture (pixels físicos).
const PIP_MIN: (i32, i32) = (240, 135);
/// Distância do canto da tela ao abrir o picture in picture pela primeira vez.
const PIP_MARGIN: i32 = 24;

type OnEvent = Box<dyn Fn(WindowEvent) + Send + Sync>;

/// Estado compartilhado entre a thread da janela, a do vídeo e quem controla a posição.
#[derive(Default)]
struct Shared {
    width: AtomicU32,
    height: AtomicU32,
    closed: AtomicBool,
    embedded: AtomicBool,
    /// Posição da janela solta antes de entrar em tela cheia.
    restore: Mutex<Option<RECT>>,
    on_event: Mutex<Option<OnEvent>>,
    /// Em picture in picture agora: a posição pedida pela interface fica de lado.
    pip: AtomicBool,
    /// Janela do app (o pai) enquanto a embutida está solta em picture in picture.
    pip_parent: AtomicIsize,
    /// Onde o picture in picture estava (volta no mesmo lugar e tamanho).
    pip_rect: Mutex<Option<RECT>>,
    /// Tamanho do vídeo, para o picture in picture redimensionar sem barras pretas.
    video_width: AtomicU32,
    video_height: AtomicU32,
    /// Fechando de verdade (o app descartou a janela): no picture in picture, fechar só volta.
    dropping: AtomicBool,
    /// O mouse está sobre o vídeo embutido (já avisamos a entrada; esperando a saída).
    hovering: AtomicBool,
    /// O último lugar pedido pela interface: reaplicado ao voltar do picture in picture.
    layout: Mutex<EmbeddedLayout>,
    /// No modo [`WindowTarget::Composed`]: o vídeo por baixo da interface.
    layer: OnceLock<Arc<Layer>>,
}

/// O que a interface pediu para a janela embutida (posição, visível e recortes).
#[derive(Debug)]
struct EmbeddedLayout {
    bounds: Bounds,
    visible: bool,
    holes: Vec<Hole>,
}

impl Default for EmbeddedLayout {
    fn default() -> Self {
        Self {
            bounds: Bounds::default(),
            visible: true,
            holes: Vec::new(),
        }
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl Shared {
    fn layout(&self) -> std::sync::MutexGuard<'_, EmbeddedLayout> {
        self.layout.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, event: WindowEvent) {
        if let Some(cb) = self.on_event.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            cb(event);
        }
    }
}

/// Retângulo em pixels físicos, relativo à área interna da janela do app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Um recorte no vídeo (onde a interface fica por cima), com cantos arredondados de `radius`
/// pixels: um menu ou um selo em pílula aparece inteiro, sem cantos pretos em volta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hole {
    pub bounds: Bounds,
    pub radius: u32,
}

/// Controla a superfície de vídeo a partir de qualquer thread.
#[derive(Debug, Clone)]
pub struct WindowControl {
    hwnd: isize,
    shared: Arc<Shared>,
}

impl WindowControl {
    /// Move e redimensiona a área do vídeo. Tamanho zero esconde.
    pub fn set_bounds(&self, b: Bounds) {
        self.shared.layout().bounds = b;
        if let Some(layer) = self.shared.layer.get() {
            layer.set_area(b);
            return;
        }
        let show = show_flag(b.width > 0 && b.height > 0);
        self.position(b.x, b.y, b.width as i32, b.height as i32, show);
    }

    /// Esconde (ex.: um modal abriu por cima) ou mostra de novo, na última posição.
    pub fn set_visible(&self, visible: bool) {
        self.shared.layout().visible = visible;
        if let Some(layer) = self.shared.layer.get() {
            layer.set_visible(visible);
            return;
        }
        self.position(0, 0, 0, 0, show_flag(visible) | SWP_NOMOVE | SWP_NOSIZE);
    }

    /// Recorta da área do vídeo os retângulos `holes` (relativos ao canto do próprio vídeo): ali a
    /// interface aparece por cima (menus, dicas, avisos) sem precisar esconder o vídeo inteiro.
    /// Vazio: o vídeo volta a ocupar a área toda. Não mexe no desenho (o swap chain continua igual).
    pub fn set_holes(&self, holes: &[Hole]) {
        holes.clone_into(&mut self.shared.layout().holes);
        // Por baixo da interface, nada precisa de recorte: ela já aparece por cima.
        if self.shared.layer.get().is_some()
            || self.shared.closed.load(Ordering::Relaxed)
            || self.shared.pip.load(Ordering::Relaxed)
        {
            return;
        }
        // SAFETY: HWND vivo (não fechado).
        unsafe { apply_holes(HWND(self.hwnd as *mut _), holes) };
    }

    /// Solta o vídeo embutido numa janela pequena, sempre no topo (`true`), ou devolve ao app.
    /// Enquanto solto, [`Self::set_bounds`], [`Self::set_visible`] e [`Self::set_holes`] só ficam
    /// guardados; ao voltar, a janela vai para o último lugar que a interface pediu.
    pub fn set_pip(&self, pip: bool) {
        if self.shared.closed.load(Ordering::Relaxed) || !self.shared.embedded.load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: HWND vivo; a troca de pai e estilo acontece na thread da janela.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_APP_PIP,
                WPARAM(usize::from(pip)),
                LPARAM(0),
            );
        }
    }

    /// Raio dos cantos do quadro (pixels físicos): o vídeo acompanha a borda arredondada. Só no modo
    /// [`WindowTarget::Composed`].
    pub fn set_corner_radius(&self, radius: u32) {
        if let Some(layer) = self.shared.layer.get() {
            layer.set_radius(radius);
        }
    }

    /// Leva o vídeo para a janela `hwnd` (a do picture in picture, que o app abre) ou de volta para
    /// a do app (`None`). Só no modo [`WindowTarget::Composed`]; devolve se deu.
    pub fn set_host(&self, hwnd: Option<isize>) -> bool {
        self.shared.layer.get().is_some_and(|layer| layer.move_to(hwnd).is_ok())
    }

    /// O HWND da janela do vídeo (0 no modo [`WindowTarget::Composed`], que não tem janela).
    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// O vídeo por baixo da interface (modo [`WindowTarget::Composed`]).
    pub fn layer(&self) -> Option<&Arc<Layer>> {
        self.shared.layer.get()
    }

    /// Recebe os eventos de tela cheia e de picture in picture (substitui o callback anterior).
    pub fn on_event(&self, cb: impl Fn(WindowEvent) + Send + Sync + 'static) {
        *self.shared.on_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(cb));
    }

    fn position(&self, x: i32, y: i32, w: i32, h: i32, flags: SET_WINDOW_POS_FLAGS) {
        if self.shared.closed.load(Ordering::Relaxed) || self.shared.pip.load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: HWND vivo (não fechado). Assíncrono: não espera a thread da janela. HWND_TOP
        // mantém o vídeo acima da WebView, que é outra janela filha do app.
        unsafe {
            let _ = SetWindowPos(
                HWND(self.hwnd as *mut _),
                Some(HWND_TOP),
                x,
                y,
                w,
                h,
                flags | SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE,
            );
        }
    }
}

/// Uma superfície de vídeo aberta. Fecha ao ser descartada.
#[derive(Debug)]
pub struct VideoWindow {
    hwnd: isize,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl VideoWindow {
    /// Abre a superfície. Solta: tamanho inicial com a proporção do vídeo. Embutida: escondida
    /// até a interface informar a posição.
    pub fn open(target: WindowTarget, video: Size) -> Result<Self> {
        let shared = Arc::new(Shared::default());
        if let WindowTarget::Composed { parent } = target {
            // Sem janela própria: o vídeo é um visual na árvore da janela do app.
            let _ = shared.layer.set(Layer::new(parent)?);
            return Ok(Self {
                hwnd: 0,
                shared,
                thread: None,
            });
        }
        shared
            .embedded
            .store(matches!(target, WindowTarget::Embedded { .. }), Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Result<isize>>();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sv-video-window".into())
            .spawn(move || window_thread(target, video, thread_shared, tx))
            .expect("não foi possível criar a thread da janela");
        let hwnd = rx.recv().unwrap_or_else(|_| Err(windows::core::Error::from_thread()))?;
        Ok(Self {
            hwnd,
            shared,
            thread: Some(thread),
        })
    }

    /// A superfície foi fechada (o usuário fechou a janela solta, ou o app fechou).
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Relaxed)
    }

    pub fn control(&self) -> WindowControl {
        WindowControl {
            hwnd: self.hwnd,
            shared: self.shared.clone(),
        }
    }

    /// Cria o renderer que desenha nesta superfície.
    pub fn renderer(&self, gpu: &Gpu) -> Result<Renderer> {
        if let Some(layer) = self.shared.layer.get() {
            return Ok(Renderer {
                gpu: gpu.clone(),
                swapchain: layer.swapchain(gpu)?,
                converter: None,
                size: Size::new(1, 1),
                shared: self.shared.clone(),
                layer: Some(layer.clone()),
                video: Size::new(0, 0),
            });
        }
        // SAFETY: criação da fábrica DXGI e da swapchain para um HWND vivo.
        let swapchain = unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                // A janela embutida começa com 0×0: a swapchain é criada com 1×1 e cresce no primeiro frame.
                Width: 1,
                Height: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
                Flags: 0,
                ..Default::default()
            };
            factory.CreateSwapChainForHwnd(&gpu.device, HWND(self.hwnd as *mut _), &desc, None, None)?
        };
        Ok(Renderer {
            gpu: gpu.clone(),
            swapchain,
            converter: None,
            size: Size::new(1, 1),
            shared: self.shared.clone(),
            layer: None,
            video: Size::new(0, 0),
        })
    }
}

impl Drop for VideoWindow {
    fn drop(&mut self) {
        self.shared.dropping.store(true, Ordering::Relaxed);
        if let Some(layer) = self.shared.layer.get() {
            // Sem janela para fechar: o vídeo some na hora (quem ainda guarda o controle não o traz de volta).
            self.shared.closed.store(true, Ordering::Relaxed);
            layer.set_visible(false);
            return;
        }
        if !self.is_closed() {
            // SAFETY: o HWND ainda existe (não foi fechado); a mensagem é tratada na thread dele.
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Desenha frames numa [`VideoWindow`]. Use na thread que decodifica o vídeo.
pub struct Renderer {
    gpu: Gpu,
    swapchain: IDXGISwapChain1,
    converter: Option<Converter>,
    size: Size,
    shared: Arc<Shared>,
    /// No modo [`WindowTarget::Composed`]: o swap chain tem o tamanho da imagem, e o visual vai
    /// para onde ela fica.
    layer: Option<Arc<Layer>>,
    /// O tamanho do vídeo do último quadro (só avisa a camada quando muda).
    video: Size,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").field("size", &self.size).finish()
    }
}

impl Renderer {
    /// Mostra um frame. Não faz nada se a superfície estiver escondida, minimizada ou fechada.
    pub fn present(&mut self, frame: &GpuFrame) -> Result<()> {
        if self.shared.closed.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Para o picture in picture manter a proporção do vídeo (só dois números, sem alocar).
        self.shared.video_width.store(frame.size.width, Ordering::Relaxed);
        self.shared.video_height.store(frame.size.height, Ordering::Relaxed);
        let window = match &self.layer {
            Some(layer) => {
                if frame.size != self.video {
                    self.video = frame.size;
                    layer.set_video(frame.size);
                }
                // O quadro inteiro: a imagem vai centralizada dentro dele (o conversor pinta as faixas).
                layer.image_size().unwrap_or(Size::new(0, 0))
            }
            None => Size::new(
                self.shared.width.load(Ordering::Relaxed),
                self.shared.height.load(Ordering::Relaxed),
            ),
        };
        if window.width == 0 || window.height == 0 {
            return Ok(());
        }
        if window != self.size {
            // O conversor guarda views do back buffer: precisa sumir antes do ResizeBuffers.
            self.converter = None;
            // SAFETY: nenhuma referência aos buffers antigos sobrou.
            unsafe {
                self.swapchain.ResizeBuffers(
                    0,
                    window.width,
                    window.height,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?
            };
            self.size = window;
        }
        let converter = match &mut self.converter {
            Some(c) => {
                c.ensure_sizes(frame.size, window)?;
                c
            }
            None => self.converter.insert(Converter::new(&self.gpu, frame.size, window)?),
        };
        // SAFETY: o buffer 0 da swapchain flip-model é o back buffer atual.
        let back: ID3D11Texture2D = unsafe { self.swapchain.GetBuffer(0)? };
        // O conversor pinta o fundo de preto fora do retângulo do vídeo.
        converter.convert(&frame.texture, frame.slice, &back, Some(fit_inside(frame.size, window)))?;
        // SAFETY: apresenta sem esperar o vsync (menor latência; o DWM compõe sem rasgar a imagem).
        unsafe { self.swapchain.Present(0, DXGI_PRESENT(0)).ok() }
    }
}

fn window_thread(target: WindowTarget, video: Size, shared: Arc<Shared>, ready: mpsc::Sender<Result<isize>>) {
    static REGISTER: Once = Once::new();
    // SAFETY: registro de classe e criação de janela na thread que vai rodar a fila de mensagens.
    // O ponteiro do Arc guardado em GWLP_USERDATA vive até o fim desta função (depois do WM_DESTROY).
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        REGISTER.call_once(|| {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("SquadViewVideo"),
                ..Default::default()
            };
            RegisterClassExW(&class);
        });

        let (title, style, x, y, width, height, parent): (HSTRING, WINDOW_STYLE, i32, i32, i32, i32, Option<HWND>) =
            match &target {
                WindowTarget::Standalone { title } => (
                    title.into(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    STANDALONE_WIDTH,
                    standalone_height(video),
                    None,
                ),
                // Filha da janela do app, sem borda e escondida até a interface dizer onde ficar.
                // O modo de composição não abre janela (não chega aqui); fica junto do embutido.
                WindowTarget::Embedded { parent } | WindowTarget::Composed { parent } => (
                    HSTRING::new(),
                    WS_CHILD | WS_CLIPSIBLINGS,
                    0,
                    0,
                    0,
                    0,
                    Some(HWND(*parent as *mut _)),
                ),
            };
        let hwnd = match CreateWindowExW(
            Default::default(),
            w!("SquadViewVideo"),
            &title,
            style,
            x,
            y,
            width,
            height,
            parent,
            None,
            Some(instance.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::as_ptr(&shared) as isize);
        store_client_size(hwnd, &shared);
        if parent.is_none() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = ready.send(Ok(hwnd.0 as isize));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    shared.closed.store(true, Ordering::Relaxed);
}

/// Largura inicial da janela solta.
const STANDALONE_WIDTH: i32 = 960;

/// Altura inicial da janela solta: a proporção do vídeo em [`STANDALONE_WIDTH`] px de largura,
/// com no mínimo 240 px (540 se o vídeo não tiver tamanho).
fn standalone_height(video: Size) -> i32 {
    (STANDALONE_WIDTH as u32 * video.height)
        .checked_div(video.width)
        .map_or(540, |h| h.max(240)) as i32
}

/// Mostrar ou esconder junto com a mudança de posição.
fn show_flag(visible: bool) -> SET_WINDOW_POS_FLAGS {
    if visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW }
}

/// O que a janela faz com cada mensagem do Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Guarda o tamanho novo da área interna (o renderer redimensiona a swapchain).
    StoreSize,
    /// Não apaga o fundo: o vídeo cobre tudo (evita piscar).
    SkipErase,
    /// Pega o foco do teclado (clique no vídeo embutido) para F11 e Esc funcionarem.
    Focus,
    /// Embutido: repassa o clique para a interface.
    Click,
    /// Embutido: repassa o botão direito para a interface (o menu do quadro).
    ContextMenu,
    /// Embutido: o mouse se mexeu; na primeira vez, avisa a entrada e pede o aviso de saída.
    Hover,
    /// Embutido: o mouse saiu.
    Leave,
    ToggleFullscreen,
    ExitFullscreen,
    Close,
    Destroyed,
    /// Entra ou sai do picture in picture.
    Pip,
    /// No picture in picture: pede ao app para voltar.
    ExitPip,
    /// No picture in picture: arrastar por qualquer ponto do vídeo move a janela.
    DragAnywhere,
    /// No picture in picture: redimensionar mantendo a proporção do vídeo.
    KeepAspect,
    /// No picture in picture: tamanho mínimo.
    MinSize,
    /// O padrão do Windows.
    Default,
}

fn action(msg: u32, key: usize, embedded: bool, pip: bool) -> Action {
    match msg {
        WM_APP_PIP => Action::Pip,
        WM_NCHITTEST if pip => Action::DragAnywhere,
        WM_SIZING if pip => Action::KeepAspect,
        WM_GETMINMAXINFO if pip => Action::MinSize,
        // Arrastando pelo vídeo, os dois cliques chegam como clique na "barra de título".
        WM_NCLBUTTONDBLCLK | WM_LBUTTONDBLCLK if pip => Action::ExitPip,
        WM_KEYDOWN if pip && key == VK_ESCAPE.0 as usize => Action::ExitPip,
        WM_KEYDOWN if pip && key == VK_F11.0 as usize => Action::Default,
        WM_CLOSE if pip => Action::ExitPip,
        WM_SIZE => Action::StoreSize,
        WM_ERASEBKGND => Action::SkipErase,
        WM_LBUTTONDOWN if embedded => Action::Focus,
        WM_LBUTTONUP if embedded && !pip => Action::Click,
        WM_RBUTTONUP if embedded && !pip => Action::ContextMenu,
        WM_MOUSEMOVE if embedded && !pip => Action::Hover,
        WM_MOUSELEAVE if embedded && !pip => Action::Leave,
        WM_KEYDOWN if key == VK_F11.0 as usize => Action::ToggleFullscreen,
        WM_KEYDOWN if key == VK_ESCAPE.0 as usize && embedded => Action::ExitFullscreen,
        WM_LBUTTONDBLCLK => Action::ToggleFullscreen,
        WM_CLOSE => Action::Close,
        WM_DESTROY => Action::Destroyed,
        _ => Action::Default,
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA guarda o ponteiro de um Arc<Shared> que vive enquanto a janela existe.
    unsafe {
        let shared = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared).as_ref();
        let embedded = shared.is_some_and(|s| s.embedded.load(Ordering::Relaxed));
        let pip = shared.is_some_and(|s| s.pip.load(Ordering::Relaxed));
        // O app está fechando a janela de vez: no picture in picture também fecha.
        let dropping = shared.is_some_and(|s| s.dropping.load(Ordering::Relaxed));
        let act = match action(msg, wparam.0, embedded, pip) {
            Action::ExitPip if msg == WM_CLOSE && dropping => Action::Close,
            other => other,
        };
        match act {
            Action::StoreSize => {
                if let Some(shared) = shared {
                    store_client_size(hwnd, shared);
                }
                LRESULT(0)
            }
            Action::SkipErase => LRESULT(1),
            Action::Focus => {
                let _ = SetFocus(Some(hwnd));
                LRESULT(0)
            }
            Action::Click | Action::ContextMenu => {
                if let Some(shared) = shared {
                    let (x, y) = client_point(lparam);
                    shared.emit(if act == Action::Click {
                        WindowEvent::Click { x, y }
                    } else {
                        WindowEvent::ContextMenu { x, y }
                    });
                }
                LRESULT(0)
            }
            Action::Hover => {
                if let Some(shared) = shared
                    && !shared.hovering.swap(true, Ordering::Relaxed)
                {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    shared.emit(WindowEvent::PointerEnter);
                }
                LRESULT(0)
            }
            Action::Leave => {
                if let Some(shared) = shared {
                    shared.hovering.store(false, Ordering::Relaxed);
                    shared.emit(WindowEvent::PointerLeave);
                }
                LRESULT(0)
            }
            Action::ToggleFullscreen => {
                if let Some(shared) = shared {
                    toggle_fullscreen(hwnd, shared);
                }
                LRESULT(0)
            }
            Action::ExitFullscreen => {
                if let Some(shared) = shared {
                    shared.emit(WindowEvent::ExitFullscreen);
                }
                LRESULT(0)
            }
            Action::Close => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            Action::Pip => {
                if let Some(shared) = shared {
                    set_pip(hwnd, shared, wparam.0 != 0);
                }
                LRESULT(0)
            }
            Action::ExitPip => {
                if let Some(shared) = shared {
                    shared.emit(WindowEvent::ExitPip);
                }
                LRESULT(0)
            }
            Action::DragAnywhere => {
                let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                if hit.0 == HTCLIENT as isize {
                    LRESULT(HTCAPTION as isize)
                } else {
                    hit
                }
            }
            Action::KeepAspect => {
                if let (Some(shared), Some(rect)) = (shared, (lparam.0 as *mut RECT).as_mut()) {
                    keep_aspect(hwnd, shared, wparam.0 as u32, rect);
                }
                LRESULT(1)
            }
            Action::MinSize => {
                if let Some(info) = (lparam.0 as *mut MINMAXINFO).as_mut() {
                    info.ptMinTrackSize = POINT {
                        x: PIP_MIN.0,
                        y: PIP_MIN.1,
                    };
                }
                LRESULT(0)
            }
            Action::Destroyed => {
                if let Some(shared) = shared {
                    shared.closed.store(true, Ordering::Relaxed);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            Action::Default => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// # Safety
/// `hwnd` precisa ser uma janela viva.
unsafe fn store_client_size(hwnd: HWND, shared: &Shared) {
    let mut rect = RECT::default();
    // SAFETY: garantido pelo chamador.
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
        shared
            .width
            .store((rect.right - rect.left).max(0) as u32, Ordering::Relaxed);
        shared
            .height
            .store((rect.bottom - rect.top).max(0) as u32, Ordering::Relaxed);
    }
}

/// O ponto do mouse numa mensagem (`lparam`: x nos 16 bits de baixo, y nos de cima, com sinal).
fn client_point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
}

/// Recorta `holes` da janela (vazio: a janela inteira volta a aparecer).
///
/// # Safety
/// `hwnd` precisa ser uma janela viva. As regiões criadas aqui são apagadas aqui, menos a final,
/// que passa a ser da janela (o Windows apaga quando trocar ou fechar).
unsafe fn apply_holes(hwnd: HWND, holes: &[Hole]) {
    // SAFETY: garantido pelo chamador.
    unsafe {
        if holes.is_empty() {
            let _ = SetWindowRgn(hwnd, None, true);
            return;
        }
        // Retângulo grande o bastante para qualquer tamanho: a região não depende do tamanho.
        let region = CreateRectRgn(0, 0, i16::MAX as i32, i16::MAX as i32);
        for h in holes {
            let (b, r) = (h.bounds, h.radius as i32);
            let (right, bottom) = (b.x + b.width as i32, b.y + b.height as i32);
            let hole = if r > 0 {
                // A elipse do canto tem o dobro do raio; o +1 fecha a borda de baixo e a da direita.
                CreateRoundRectRgn(b.x, b.y, right + 1, bottom + 1, 2 * r, 2 * r)
            } else {
                CreateRectRgn(b.x, b.y, right, bottom)
            };
            let _ = CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF);
            let _ = DeleteObject(HGDIOBJ(hole.0));
        }
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
}

/// Proporção do vídeo (largura / altura), 16:9 enquanto nenhum quadro chegou.
fn video_aspect(shared: &Shared) -> f64 {
    let (w, h) = (
        shared.video_width.load(Ordering::Relaxed),
        shared.video_height.load(Ordering::Relaxed),
    );
    if w == 0 || h == 0 {
        16.0 / 9.0
    } else {
        f64::from(w) / f64::from(h)
    }
}

/// Onde o picture in picture abre da primeira vez: no canto de baixo à direita da tela do app,
/// com um quarto da largura dela (entre 320 e 640 px), na proporção do vídeo.
fn default_pip_rect(work: RECT, aspect: f64) -> RECT {
    let width = ((work.right - work.left) / 4).clamp(320, 640);
    let height = (f64::from(width) / aspect).round() as i32;
    RECT {
        left: work.right - width - PIP_MARGIN,
        top: work.bottom - height - PIP_MARGIN,
        right: work.right - PIP_MARGIN,
        bottom: work.bottom - PIP_MARGIN,
    }
}

/// Ajusta o retângulo que o usuário está arrastando para a área do vídeo manter a proporção:
/// puxando pelo topo ou por baixo muda a largura; pelos lados ou cantos, a altura.
fn fit_sizing(rect: &mut RECT, edge: u32, aspect: f64, frame: (i32, i32)) {
    let client_w = (rect.right - rect.left - frame.0).max(1);
    let client_h = (rect.bottom - rect.top - frame.1).max(1);
    if edge == WMSZ_TOP || edge == WMSZ_BOTTOM {
        rect.right = rect.left + (f64::from(client_h) * aspect).round() as i32 + frame.0;
    } else {
        let height = (f64::from(client_w) / aspect).round() as i32 + frame.1;
        if edge == WMSZ_TOPLEFT || edge == WMSZ_TOPRIGHT {
            rect.top = rect.bottom - height;
        } else {
            rect.bottom = rect.top + height;
        }
    }
}

/// # Safety
/// Chamar só na thread da janela, com `hwnd` vivo.
unsafe fn keep_aspect(hwnd: HWND, shared: &Shared, edge: u32, rect: &mut RECT) {
    let (mut outer, mut inner) = (RECT::default(), RECT::default());
    // SAFETY: garantido pelo chamador.
    unsafe {
        let _ = GetWindowRect(hwnd, &mut outer);
        let _ = GetClientRect(hwnd, &mut inner);
    }
    let frame = (
        (outer.right - outer.left) - (inner.right - inner.left),
        (outer.bottom - outer.top) - (inner.bottom - inner.top),
    );
    fit_sizing(rect, edge, video_aspect(shared), frame);
}

/// Solta a janela embutida do app (pequena, sempre no topo, fora da barra de tarefas) ou devolve
/// ao app, no último lugar que a interface pediu.
///
/// # Safety
/// Chamar só na thread da janela, com `hwnd` vivo.
unsafe fn set_pip(hwnd: HWND, shared: &Shared, pip: bool) {
    if pip == shared.pip.load(Ordering::Relaxed) || !shared.embedded.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: garantido pelo chamador.
    unsafe {
        if pip {
            let Ok(parent) = GetParent(hwnd) else {
                return;
            };
            shared.pip_parent.store(parent.0 as isize, Ordering::Relaxed);
            shared.pip.store(true, Ordering::Relaxed);
            let saved = *shared.pip_rect.lock().unwrap_or_else(|e| e.into_inner());
            let rect = saved.unwrap_or_else(|| {
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(MonitorFromWindow(parent, MONITOR_DEFAULTTONEAREST), &mut info);
                default_pip_rect(info.rcWork, video_aspect(shared))
            });
            // Sem os recortes dos menus do app: aqui não há nada por cima.
            let _ = SetWindowRgn(hwnd, None, false);
            let _ = SetParent(hwnd, None);
            SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_THICKFRAME | WS_CLIPSIBLINGS).0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (WS_EX_TOPMOST | WS_EX_TOOLWINDOW).0 as isize);
            // Cantos arredondados no Windows 11 (no 10, a chamada só não faz nada).
            let corners = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corners).cast(),
                std::mem::size_of_val(&corners) as u32,
            );
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        } else {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                *shared.pip_rect.lock().unwrap_or_else(|e| e.into_inner()) = Some(rect);
            }
            let parent = HWND(shared.pip_parent.load(Ordering::Relaxed) as *mut _);
            // O "sempre no topo" só sai pelo SetWindowPos, e enquanto ainda é janela de nível superior.
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_NOTOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_CHILD | WS_CLIPSIBLINGS).0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, 0);
            let _ = SetParent(hwnd, Some(parent));
            // Volta para o último lugar que a interface pediu (o que ela mandou durante o picture
            // in picture ficou guardado), já com os recortes dos menus.
            shared.pip.store(false, Ordering::Relaxed);
            let layout = shared.layout();
            let b = layout.bounds;
            let show = show_flag(layout.visible && b.width > 0 && b.height > 0);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                b.x,
                b.y,
                b.width as i32,
                b.height as i32,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | show,
            );
            apply_holes(hwnd, &layout.holes);
        }
    }
}

/// Embutida: pede ao app para alternar a tela cheia (quem decide o layout é a interface).
/// Solta: alterna entre janela normal e tela cheia sem bordas no monitor onde ela está.
///
/// # Safety
/// Chamar só na thread da janela, com `hwnd` vivo.
unsafe fn toggle_fullscreen(hwnd: HWND, shared: &Shared) {
    if shared.embedded.load(Ordering::Relaxed) {
        shared.emit(WindowEvent::ToggleFullscreen);
        return;
    }
    let mut restore = shared.restore.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: garantido pelo chamador.
    unsafe {
        if let Some(rect) = restore.take() {
            SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW | WS_VISIBLE).0 as isize);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
            );
        } else {
            let mut current = RECT::default();
            let _ = GetWindowRect(hwnd, &mut current);
            *restore = Some(current);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
            let m = info.rcMonitor;
            SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                m.left,
                m.top,
                m.right - m.left,
                m.bottom - m.top,
                SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    use super::*;

    /// Espera até 3 s a condição ficar verdadeira (a janela vive noutra thread).
    fn eventually(cond: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        cond()
    }

    fn post(window: &VideoWindow, msg: u32, wparam: usize) {
        // SAFETY: mensagem para uma janela viva, tratada na thread dela.
        unsafe {
            PostMessageW(Some(HWND(window.hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(0)).unwrap();
        }
    }

    fn style(window: &VideoWindow) -> u32 {
        // SAFETY: leitura de atributo de uma janela viva.
        unsafe { GetWindowLongPtrW(HWND(window.hwnd as *mut _), GWL_STYLE) as u32 }
    }

    fn visible(window: &VideoWindow) -> bool {
        // SAFETY: consulta a uma janela viva.
        unsafe { IsWindowVisible(HWND(window.hwnd as *mut _)).as_bool() }
    }

    fn solta(titulo: &str) -> VideoWindow {
        VideoWindow::open(WindowTarget::Standalone { title: titulo.into() }, Size::new(1280, 720))
            .expect("abrir a janela solta")
    }

    #[test]
    fn altura_inicial_segue_a_proporcao_do_video() {
        assert_eq!(standalone_height(Size::new(1920, 1080)), 540);
        assert_eq!(standalone_height(Size::new(1080, 1920)), 1706);
        assert_eq!(standalone_height(Size::new(4000, 100)), 240, "no mínimo 240 px");
        assert_eq!(standalone_height(Size::new(0, 0)), 540, "vídeo sem tamanho");
    }

    #[test]
    fn tamanho_zero_esconde_a_area_do_video() {
        assert_eq!(show_flag(true), SWP_SHOWWINDOW);
        assert_eq!(show_flag(false), SWP_HIDEWINDOW);
    }

    #[test]
    fn teclas_e_cliques_viram_acoes() {
        let f11 = VK_F11.0 as usize;
        let esc = VK_ESCAPE.0 as usize;
        let letra_a = 0x41;
        assert_eq!(action(WM_SIZE, 0, false, false), Action::StoreSize);
        assert_eq!(action(WM_ERASEBKGND, 0, true, false), Action::SkipErase);
        assert_eq!(action(WM_LBUTTONDOWN, 0, true, false), Action::Focus);
        assert_eq!(
            action(WM_LBUTTONDOWN, 0, false, false),
            Action::Default,
            "solta: clique normal"
        );
        assert_eq!(action(WM_KEYDOWN, f11, false, false), Action::ToggleFullscreen);
        assert_eq!(action(WM_KEYDOWN, f11, true, false), Action::ToggleFullscreen);
        assert_eq!(action(WM_KEYDOWN, esc, true, false), Action::ExitFullscreen);
        assert_eq!(
            action(WM_KEYDOWN, esc, false, false),
            Action::Default,
            "solta: Esc não faz nada"
        );
        assert_eq!(action(WM_KEYDOWN, letra_a, true, false), Action::Default);
        assert_eq!(action(WM_LBUTTONDBLCLK, 0, false, false), Action::ToggleFullscreen);
        assert_eq!(action(WM_CLOSE, 0, false, false), Action::Close);
        assert_eq!(action(WM_DESTROY, 0, false, false), Action::Destroyed);
        assert_eq!(action(0x0200, 0, false, false), Action::Default);
    }

    #[test]
    fn embutido_repassa_clique_botao_direito_e_mouse_para_a_interface() {
        assert_eq!(action(WM_LBUTTONUP, 0, true, false), Action::Click);
        assert_eq!(action(WM_RBUTTONUP, 0, true, false), Action::ContextMenu);
        assert_eq!(action(WM_MOUSEMOVE, 0, true, false), Action::Hover);
        assert_eq!(action(WM_MOUSELEAVE, 0, true, false), Action::Leave);
        // Solta (janela própria) ou no picture in picture: nada é repassado.
        assert_eq!(action(WM_RBUTTONUP, 0, false, false), Action::Default);
        assert_eq!(action(WM_LBUTTONUP, 0, true, true), Action::Default);
        assert_eq!(action(WM_MOUSEMOVE, 0, false, false), Action::Default);
        assert_eq!(action(WM_NCHITTEST, 0, true, true), Action::DragAnywhere);
    }

    #[test]
    fn ponto_do_mouse_sai_do_lparam_com_sinal() {
        assert_eq!(client_point(LPARAM((40 << 16) | 120)), (120, 40));
        // Arrastando para fora da janela, as coordenadas ficam negativas.
        assert_eq!(client_point(LPARAM((0xFFFF << 16) | 0xFFFE)), (-2, -1));
    }

    #[test]
    fn no_picture_in_picture_arrastar_redimensionar_e_voltar_viram_acoes() {
        let esc = VK_ESCAPE.0 as usize;
        let f11 = VK_F11.0 as usize;
        assert_eq!(action(WM_APP_PIP, 1, true, false), Action::Pip);
        assert_eq!(action(WM_NCHITTEST, 0, true, true), Action::DragAnywhere);
        assert_eq!(action(WM_NCHITTEST, 0, true, false), Action::Default);
        assert_eq!(action(WM_SIZING, 0, true, true), Action::KeepAspect);
        assert_eq!(action(WM_GETMINMAXINFO, 0, true, true), Action::MinSize);
        assert_eq!(action(WM_NCLBUTTONDBLCLK, 0, true, true), Action::ExitPip);
        assert_eq!(action(WM_LBUTTONDBLCLK, 0, true, true), Action::ExitPip);
        assert_eq!(action(WM_KEYDOWN, esc, true, true), Action::ExitPip);
        assert_eq!(
            action(WM_KEYDOWN, f11, true, true),
            Action::Default,
            "F11 não abre tela cheia no PiP"
        );
        assert_eq!(
            action(WM_CLOSE, 0, true, true),
            Action::ExitPip,
            "fechar o PiP só volta"
        );
    }

    #[test]
    fn picture_in_picture_abre_no_canto_e_redimensiona_na_proporcao() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let r = default_pip_rect(work, 16.0 / 9.0);
        assert_eq!((r.right - r.left, r.bottom - r.top), (480, 270));
        assert_eq!((r.right, r.bottom), (1920 - PIP_MARGIN, 1040 - PIP_MARGIN));
        // Tela pequena: no mínimo 320 de largura; vídeo 4:3 fica mais alto.
        let small = RECT {
            right: 1000,
            bottom: 700,
            ..work
        };
        let r = default_pip_rect(small, 4.0 / 3.0);
        assert_eq!((r.right - r.left, r.bottom - r.top), (320, 240));

        // Bordas de 16×16 px (o redimensionável): a área do vídeo fica em 16:9.
        let frame = (16, 16);
        let mut pelo_lado = RECT {
            left: 0,
            top: 0,
            right: 656,
            bottom: 999,
        };
        fit_sizing(&mut pelo_lado, 2, 16.0 / 9.0, frame);
        assert_eq!(pelo_lado.bottom - pelo_lado.top - 16, 360);
        let mut pelo_topo = RECT {
            left: 0,
            top: 100,
            right: 999,
            bottom: 476,
        };
        fit_sizing(&mut pelo_topo, WMSZ_TOP, 16.0 / 9.0, frame);
        assert_eq!(pelo_topo.right - pelo_topo.left - 16, 640);
        let mut canto_de_cima = RECT {
            left: 0,
            top: 0,
            right: 336,
            bottom: 500,
        };
        fit_sizing(&mut canto_de_cima, WMSZ_TOPLEFT, 16.0 / 9.0, frame);
        assert_eq!(
            (canto_de_cima.bottom, canto_de_cima.bottom - canto_de_cima.top - 16),
            (500, 180)
        );
    }

    #[test]
    fn video_embutido_entra_e_sai_do_picture_in_picture() {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW as long;
        let app = solta("SquadView (app, PiP)");
        let video = VideoWindow::open(WindowTarget::Embedded { parent: app.hwnd }, Size::new(1920, 1080))
            .expect("abrir o vídeo embutido");
        let control = video.control();
        let lugar = Bounds {
            x: 10,
            y: 20,
            width: 320,
            height: 180,
        };
        control.set_bounds(lugar);
        assert!(eventually(|| visible(&video)));
        let hwnd = HWND(video.hwnd as *mut _);
        // SAFETY: janelas vivas durante o teste.
        let parent = || unsafe { GetParent(hwnd).map(|p| p.0 as isize).unwrap_or(0) };
        let ex = || unsafe { long(hwnd, GWL_EXSTYLE) } as u32;
        assert_eq!(parent(), app.hwnd);

        control.set_pip(true);
        assert!(eventually(|| parent() == 0), "não se soltou do app");
        assert!(eventually(|| ex() & WS_EX_TOPMOST.0 != 0), "não ficou sempre no topo");
        assert!(visible(&video));
        assert!(eventually(
            || video.shared.width.load(Ordering::Relaxed) >= PIP_MIN.0 as u32
        ));
        // No PiP, a interface mexendo no layout não move nem esconde o vídeo.
        let novo = Bounds {
            x: 30,
            y: 40,
            width: 400,
            height: 225,
        };
        control.set_bounds(novo);
        control.set_visible(false);
        control.set_visible(true);
        assert!(visible(&video));

        // Fechar o PiP (ou dois cliques, Esc) só avisa o app; ele decide voltar.
        let (tx, rx) = mpsc::channel();
        control.on_event(move |e| {
            let _ = tx.send(e);
        });
        post(&video, WM_CLOSE, 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), WindowEvent::ExitPip);
        assert!(!video.is_closed());

        control.set_pip(false);
        assert!(eventually(|| parent() == app.hwnd), "não voltou para o app");
        assert_eq!(ex() & WS_EX_TOPMOST.0, 0);
        // Volta no último lugar pedido durante o PiP.
        assert!(eventually(
            || video.shared.width.load(Ordering::Relaxed) == 400 && video.shared.height.load(Ordering::Relaxed) == 225
        ));
        assert!(eventually(|| visible(&video)));
        // O lugar do PiP fica guardado para a próxima vez.
        assert!(video.shared.pip_rect.lock().unwrap().is_some());

        // Descartar a janela no meio do PiP fecha de verdade.
        control.set_pip(true);
        assert!(eventually(|| parent() == 0));
        let start = Instant::now();
        drop(video);
        assert!(start.elapsed() < Duration::from_secs(2), "fechar no PiP travou");
        drop(app);
    }

    #[test]
    fn janela_solta_nao_entra_em_picture_in_picture() {
        let window = solta("SquadView (solta, PiP)");
        window.control().set_pip(true);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!window.shared.pip.load(Ordering::Relaxed));
    }

    #[test]
    fn janela_solta_abre_com_tamanho_e_fecha_ao_descartar() {
        let window = solta("SquadView (teste)");
        assert!(!window.is_closed());
        assert!(visible(&window));
        assert!(eventually(|| window.shared.width.load(Ordering::Relaxed) > 0));
        assert!(format!("{window:?}").contains("closed"));
        let start = Instant::now();
        drop(window);
        assert!(start.elapsed() < Duration::from_secs(2), "fechar a janela travou");
    }

    #[test]
    fn janela_fechada_pelo_usuario_ignora_comandos() {
        let window = solta("SquadView (fechar)");
        let control = window.control();
        post(&window, WM_CLOSE, 0);
        assert!(eventually(|| window.is_closed()));
        // Depois de fechada, mover ou esconder não faz nada (e não quebra).
        control.set_bounds(Bounds {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });
        control.set_visible(true);
        assert!(format!("{control:?}").contains("WindowControl"));
    }

    #[test]
    fn janela_solta_alterna_tela_cheia_com_f11_e_dois_cliques() {
        let window = solta("SquadView (tela cheia)");
        assert_ne!(style(&window) & WS_OVERLAPPEDWINDOW.0, 0);

        post(&window, WM_KEYDOWN, VK_F11.0 as usize);
        assert!(
            eventually(|| style(&window) & WS_POPUP.0 != 0),
            "não entrou em tela cheia"
        );
        assert!(window.shared.restore.lock().unwrap().is_some());

        post(&window, WM_LBUTTONDBLCLK, 0);
        assert!(
            eventually(|| style(&window) & WS_POPUP.0 == 0),
            "não voltou para a janela"
        );
        assert!(window.shared.restore.lock().unwrap().is_none());
    }

    #[test]
    fn video_embutido_segue_a_posicao_da_interface_e_avisa_a_tela_cheia() {
        let app = solta("SquadView (app)");
        let video = VideoWindow::open(WindowTarget::Embedded { parent: app.hwnd }, Size::new(1920, 1080))
            .expect("abrir o vídeo embutido");
        assert!(!visible(&video), "embutido começa escondido");

        let control = video.control();
        control.set_bounds(Bounds {
            x: 10,
            y: 20,
            width: 320,
            height: 180,
        });
        assert!(eventually(
            || video.shared.width.load(Ordering::Relaxed) == 320 && video.shared.height.load(Ordering::Relaxed) == 180
        ));
        assert!(eventually(|| visible(&video)));

        control.set_visible(false);
        assert!(eventually(|| !visible(&video)));
        control.set_visible(true);
        assert!(eventually(|| visible(&video)));
        control.set_bounds(Bounds::default());
        assert!(eventually(|| !visible(&video)), "tamanho zero esconde");

        let (tx, rx) = mpsc::channel();
        control.on_event(move |e| {
            let _ = tx.send(e);
        });
        let recv = || rx.recv_timeout(Duration::from_secs(3)).expect("evento");
        post(&video, WM_LBUTTONDOWN, 0);
        post(&video, WM_KEYDOWN, VK_F11.0 as usize);
        assert_eq!(recv(), WindowEvent::ToggleFullscreen);
        post(&video, WM_KEYDOWN, VK_ESCAPE.0 as usize);
        assert_eq!(recv(), WindowEvent::ExitFullscreen);
        post(&video, WM_LBUTTONDBLCLK, 0);
        assert_eq!(recv(), WindowEvent::ToggleFullscreen);
        // Embutida: quem decide o layout é a interface, a janela não muda de estilo.
        assert_eq!(style(&video) & WS_POPUP.0, 0);

        drop(video);
        drop(app);
    }

    /// Um vídeo por baixo da interface da janela `app`. `None` se esta máquina não tem
    /// DirectComposition (servidor sem área de trabalho): o teste é pulado.
    fn composto(app: &VideoWindow) -> Option<VideoWindow> {
        match VideoWindow::open(WindowTarget::Composed { parent: app.hwnd }, Size::new(1920, 1080)) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("pulado: sem DirectComposition aqui ({e})");
                None
            }
        }
    }

    #[test]
    fn video_por_baixo_da_interface_fica_so_na_area_da_imagem() {
        let app = solta("SquadView (app, composição)");
        let Some(video) = composto(&app) else { return };
        let control = video.control();
        let layer = control.layer().expect("modo de composição").clone();
        // Sem área nem quadro ainda: nada aparece.
        assert_eq!(layer.placed(), None);
        let quadro = Bounds {
            x: 10,
            y: 20,
            width: 640,
            height: 360,
        };
        control.set_bounds(quadro);
        assert_eq!(layer.placed(), None, "sem quadro do vídeo ainda");
        layer.set_video(Size::new(1920, 1080));
        assert_eq!(layer.placed(), Some((app.hwnd, quadro)));
        control.set_corner_radius(8);

        // Imagem estreita: o vídeo continua cobrindo o quadro (as faixas ficam dentro dele).
        layer.set_video(Size::new(1000, 1000));
        assert_eq!(layer.placed(), Some((app.hwnd, quadro)));

        // Esconder e mostrar; tamanho zero também esconde.
        control.set_visible(false);
        assert_eq!(layer.placed(), None);
        control.set_visible(true);
        assert!(layer.placed().is_some());
        control.set_bounds(Bounds::default());
        assert_eq!(layer.placed(), None);
        control.set_bounds(quadro);

        // Recortes e o picture in picture antigo não fazem nada aqui (a interface já fica por cima).
        control.set_holes(&[Hole {
            bounds: quadro,
            radius: 4,
        }]);
        control.set_pip(true);
        assert!(layer.placed().is_some());
        assert!(!video.is_closed());

        // Descartar o vídeo tira da tela, mesmo com o controle ainda guardado.
        drop(video);
        assert_eq!(layer.placed(), None);
        control.set_visible(true);
        assert!(format!("{layer:?}").contains("Layer"));
        drop(app);
    }

    #[test]
    fn video_por_baixo_vai_para_a_janela_do_picture_in_picture_e_volta() {
        let app = solta("SquadView (app, PiP composto)");
        let pip = solta("SquadView (PiP composto)");
        let Some(video) = composto(&app) else { return };
        let Some(outro) = composto(&app) else { return };
        let control = video.control();
        let layer = control.layer().unwrap().clone();
        let lugar = Bounds {
            x: 0,
            y: 0,
            width: 320,
            height: 180,
        };
        control.set_bounds(lugar);
        layer.set_video(Size::new(1920, 1080));
        // Dois vídeos na mesma janela.
        outro.control().set_bounds(lugar);
        outro.control().layer().unwrap().set_video(Size::new(1280, 720));
        assert!(outro.control().layer().unwrap().placed().is_some());

        assert!(control.set_host(Some(pip.hwnd)));
        assert_eq!(layer.placed().map(|(h, _)| h), Some(pip.hwnd));
        // De volta para o app, no mesmo lugar.
        assert!(control.set_host(None));
        assert_eq!(layer.placed(), Some((app.hwnd, lugar)));
        assert!(control.set_host(None), "já está no app: não muda nada");
        // O outro vídeo não foi afetado.
        assert!(outro.control().layer().unwrap().placed().is_some());

        // Uma janela solta (sem composição) não tem para onde levar.
        assert!(!app.control().set_host(Some(pip.hwnd)));
        assert!(app.control().layer().is_none());
        app.control().set_corner_radius(4);
        drop((video, outro, pip, app));
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop"]
    fn renderer_por_baixo_desenha_do_tamanho_do_quadro() {
        let gpu = Gpu::new().unwrap();
        let app = solta("SquadView (app, render composto)");
        let video = composto(&app).expect("DirectComposition");
        let control = video.control();
        control.set_bounds(Bounds {
            x: 0,
            y: 0,
            width: 640,
            height: 360,
        });
        let mut renderer = video.renderer(&gpu).expect("swap chain de composição");
        // O swap chain tem o tamanho do quadro, qualquer que seja a imagem.
        renderer.present(&nv12_frame(&gpu, Size::new(512, 512))).unwrap();
        assert_eq!(renderer.size, Size::new(640, 360));
        control.set_bounds(Bounds {
            x: 0,
            y: 0,
            width: 320,
            height: 180,
        });
        renderer.present(&nv12_frame(&gpu, Size::new(1920, 1080))).unwrap();
        assert_eq!(renderer.size, Size::new(320, 180));
        // Escondido: não desenha nada.
        control.set_visible(false);
        renderer.present(&nv12_frame(&gpu, Size::new(1920, 1080))).unwrap();
    }

    fn nv12_frame(gpu: &Gpu, size: Size) -> GpuFrame {
        use sv_gpu::testing::texture;
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
        GpuFrame {
            texture: texture(gpu, size, DXGI_FORMAT_NV12, None).unwrap(),
            slice: 0,
            size,
            timestamp_us: 0,
        }
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop"]
    fn renderer_desenha_frames_e_acompanha_o_tamanho_da_janela() {
        let gpu = Gpu::new().unwrap();
        let window = solta("SquadView (render)");
        let mut renderer = window.renderer(&gpu).expect("swapchain");
        assert!(format!("{renderer:?}").contains("Renderer"));
        let frame = nv12_frame(&gpu, Size::new(1280, 720));
        assert!(eventually(|| window.shared.width.load(Ordering::Relaxed) > 0));
        for _ in 0..3 {
            renderer.present(&frame).expect("present");
        }
        let first = renderer.size;
        assert_eq!(first.width, window.shared.width.load(Ordering::Relaxed));

        // A janela cresceu (tela cheia): a swapchain acompanha no próximo frame.
        post(&window, WM_KEYDOWN, VK_F11.0 as usize);
        assert!(eventually(|| window.shared.width.load(Ordering::Relaxed) != first.width));
        renderer.present(&frame).expect("present depois de redimensionar");
        assert_ne!(renderer.size, first);

        // Vídeo de outro tamanho (o streamer mudou a resolução).
        renderer
            .present(&nv12_frame(&gpu, Size::new(640, 360)))
            .expect("present com vídeo menor");

        post(&window, WM_CLOSE, 0);
        assert!(eventually(|| window.is_closed()));
        renderer.present(&frame).expect("janela fechada: não faz nada");
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn renderer_nao_desenha_com_a_area_escondida() {
        let gpu = Gpu::new().unwrap();
        let app = solta("SquadView (app render)");
        let video = VideoWindow::open(WindowTarget::Embedded { parent: app.hwnd }, Size::new(64, 64)).unwrap();
        let mut renderer = video.renderer(&gpu).unwrap();
        renderer.present(&nv12_frame(&gpu, Size::new(64, 64))).unwrap();
        assert_eq!(renderer.size, Size::new(1, 1), "escondida (0×0): nada muda");
    }
}
