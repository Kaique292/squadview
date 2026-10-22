//! Superfície nativa que mostra o vídeo (flip-model do DXGI, sem passar pela WebView).
//!
//! Dois jeitos de abrir ([`WindowTarget`]):
//! - **por baixo da interface** (ADR 0021): um visual do DirectComposition na árvore da janela do
//!   app, sem janela própria ([`Layer`]). A interface informa a área do quadro com
//!   [`WindowControl::set_bounds`] e fica por cima, com mistura de verdade. O picture in picture é
//!   outra janela do app: o visual muda de árvore ([`WindowControl::set_host`]).
//! - **solta**: uma janela própria, com título (usada pelo exemplo de terminal). Ela vive na própria
//!   thread (fila de mensagens do Windows): F11 e dois cliques alternam a tela cheia.
//!
//! O [`Renderer`] desenha a partir da thread do vídeo: converte a textura NV12 direto no back
//! buffer, centralizada e sem distorcer.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock, mpsc};
use std::thread::JoinHandle;

use sv_gpu::{Converter, Gpu, GpuFrame, Size, fit_inside};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
    IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_F11;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GWL_STYLE, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowRect, HWND_TOP,
    IDC_ARROW, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SW_SHOW, SWP_FRAMECHANGED,
    SWP_NOOWNERZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{HSTRING, Result, w};

use crate::composition::Layer;

/// Onde o vídeo aparece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowTarget {
    /// Janela própria, com título e borda.
    Standalone { title: String },
    /// Desenhado por baixo da interface da janela `parent`, pelo DirectComposition (ADR 0021): a
    /// interface fica por cima com mistura de verdade, sem recortes. Sem janela própria.
    Composed { parent: isize },
}

/// Estado compartilhado entre a thread da janela, a do vídeo e quem controla a posição.
#[derive(Default)]
struct Shared {
    width: AtomicU32,
    height: AtomicU32,
    closed: AtomicBool,
    /// Posição da janela solta antes de entrar em tela cheia.
    restore: Mutex<Option<RECT>>,
    /// No modo [`WindowTarget::Composed`]: o vídeo por baixo da interface.
    layer: OnceLock<Arc<Layer>>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
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

/// Controla a superfície de vídeo a partir de qualquer thread. Posição, cantos e picture in picture
/// valem para o vídeo por baixo da interface; a janela solta cuida de si mesma.
#[derive(Debug, Clone)]
pub struct WindowControl {
    hwnd: isize,
    shared: Arc<Shared>,
}

impl WindowControl {
    /// Move e redimensiona a área do vídeo. Tamanho zero esconde.
    pub fn set_bounds(&self, b: Bounds) {
        if let Some(layer) = self.shared.layer.get() {
            layer.set_area(b);
        }
    }

    /// Esconde ou mostra de novo, na última posição.
    pub fn set_visible(&self, visible: bool) {
        if let Some(layer) = self.shared.layer.get() {
            layer.set_visible(visible);
        }
    }

    /// Raio dos cantos do quadro (pixels físicos): o vídeo acompanha a borda arredondada.
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

    /// O HWND da janela solta (0 no modo [`WindowTarget::Composed`], que não tem janela).
    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// O vídeo por baixo da interface (modo [`WindowTarget::Composed`]).
    pub fn layer(&self) -> Option<&Arc<Layer>> {
        self.shared.layer.get()
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
    /// Abre a superfície. Solta: tamanho inicial com a proporção do vídeo. Por baixo da interface:
    /// escondida até a interface informar a área.
    pub fn open(target: WindowTarget, video: Size) -> Result<Self> {
        let shared = Arc::new(Shared::default());
        let title = match target {
            WindowTarget::Composed { parent } => {
                // Sem janela própria: o vídeo é um visual na árvore da janela do app.
                let _ = shared.layer.set(Layer::new(parent)?);
                return Ok(Self {
                    hwnd: 0,
                    shared,
                    thread: None,
                });
            }
            WindowTarget::Standalone { title } => title,
        };
        let (tx, rx) = mpsc::channel::<Result<isize>>();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sv-video-window".into())
            .spawn(move || window_thread(&title, video, thread_shared, tx))
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
                // Começa com 1×1 e cresce no primeiro frame, com o tamanho da janela.
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
    /// No modo [`WindowTarget::Composed`]: o visual do vídeo, que diz o tamanho do quadro.
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

fn window_thread(title: &str, video: Size, shared: Arc<Shared>, ready: mpsc::Sender<Result<isize>>) {
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

        let hwnd = match CreateWindowExW(
            Default::default(),
            w!("SquadViewVideo"),
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            STANDALONE_WIDTH,
            standalone_height(video),
            None,
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
        let _ = ShowWindow(hwnd, SW_SHOW);
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

/// O que a janela solta faz com cada mensagem do Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Guarda o tamanho novo da área interna (o renderer redimensiona a swapchain).
    StoreSize,
    /// Não apaga o fundo: o vídeo cobre tudo (evita piscar).
    SkipErase,
    ToggleFullscreen,
    Close,
    Destroyed,
    /// O padrão do Windows.
    Default,
}

fn action(msg: u32, key: usize) -> Action {
    match msg {
        WM_SIZE => Action::StoreSize,
        WM_ERASEBKGND => Action::SkipErase,
        WM_KEYDOWN if key == VK_F11.0 as usize => Action::ToggleFullscreen,
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
        match action(msg, wparam.0) {
            Action::StoreSize => {
                if let Some(shared) = shared {
                    store_client_size(hwnd, shared);
                }
                LRESULT(0)
            }
            Action::SkipErase => LRESULT(1),
            Action::ToggleFullscreen => {
                if let Some(shared) = shared {
                    toggle_fullscreen(hwnd, shared);
                }
                LRESULT(0)
            }
            Action::Close => {
                let _ = DestroyWindow(hwnd);
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

/// Alterna a janela solta entre normal e tela cheia sem bordas no monitor onde ela está.
///
/// # Safety
/// Chamar só na thread da janela, com `hwnd` vivo.
unsafe fn toggle_fullscreen(hwnd: HWND, shared: &Shared) {
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
    fn teclas_e_cliques_viram_acoes() {
        let f11 = VK_F11.0 as usize;
        let letra_a = 0x41;
        assert_eq!(action(WM_SIZE, 0), Action::StoreSize);
        assert_eq!(action(WM_ERASEBKGND, 0), Action::SkipErase);
        assert_eq!(action(WM_KEYDOWN, f11), Action::ToggleFullscreen);
        assert_eq!(action(WM_KEYDOWN, letra_a), Action::Default);
        assert_eq!(action(WM_LBUTTONDBLCLK, 0), Action::ToggleFullscreen);
        assert_eq!(action(WM_CLOSE, 0), Action::Close);
        assert_eq!(action(WM_DESTROY, 0), Action::Destroyed);
        assert_eq!(action(0x0200, 0), Action::Default, "o mouse se mexendo: o padrão");
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
        // Depois de fechada (e na janela solta), mover ou esconder não faz nada (e não quebra).
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
}
