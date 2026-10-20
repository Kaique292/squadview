//! Superfície nativa que mostra o vídeo (flip-model do DXGI, sem passar pela WebView).
//!
//! Dois jeitos de abrir ([`WindowTarget`]):
//! - **embutida**: uma janela filha, sem borda, grudada dentro da janela do app, exatamente sobre a
//!   área que a interface reserva para o stream (como no Discord). A interface informa a posição
//!   com [`WindowControl::set_bounds`].
//! - **solta**: uma janela própria, com título (usada pelo exemplo de terminal).
//!
//! A janela vive na própria thread (fila de mensagens do Windows). O [`Renderer`] desenha a partir
//! da thread do vídeo: converte a textura NV12 direto no back buffer, centralizada e sem distorcer.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once, mpsc};
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
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, RGN_DIFF, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_F11};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GWL_STYLE, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowRect, HWND_TOP,
    IDC_ARROW, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SET_WINDOW_POS_FLAGS, SW_SHOW,
    SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_STYLE, WM_CLOSE, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS,
    WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{HSTRING, Result, w};

/// Onde o vídeo aparece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowTarget {
    /// Janela própria, com título e borda.
    Standalone { title: String },
    /// Dentro da janela do app (`parent` é o HWND dela). Começa escondida até receber a posição.
    Embedded { parent: isize },
}

/// O que o usuário fez dentro da área do vídeo embutida (na janela solta, a própria janela
/// cuida da tela cheia).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    /// Dois cliques ou F11.
    ToggleFullscreen,
    /// Esc.
    ExitFullscreen,
}

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
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl Shared {
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

/// Controla a superfície de vídeo a partir de qualquer thread.
#[derive(Debug, Clone)]
pub struct WindowControl {
    hwnd: isize,
    shared: Arc<Shared>,
}

impl WindowControl {
    /// Move e redimensiona a área do vídeo. Tamanho zero esconde.
    pub fn set_bounds(&self, b: Bounds) {
        let show = show_flag(b.width > 0 && b.height > 0);
        self.position(b.x, b.y, b.width as i32, b.height as i32, show);
    }

    /// Esconde (ex.: um modal abriu por cima) ou mostra de novo, na última posição.
    pub fn set_visible(&self, visible: bool) {
        self.position(0, 0, 0, 0, show_flag(visible) | SWP_NOMOVE | SWP_NOSIZE);
    }

    /// Recorta da área do vídeo os retângulos `holes` (relativos ao canto do próprio vídeo): ali a
    /// interface aparece por cima (menus, dicas, avisos) sem precisar esconder o vídeo inteiro.
    /// Vazio: o vídeo volta a ocupar a área toda. Não mexe no desenho (o swap chain continua igual).
    pub fn set_holes(&self, holes: &[Bounds]) {
        if self.shared.closed.load(Ordering::Relaxed) {
            return;
        }
        let hwnd = HWND(self.hwnd as *mut _);
        // SAFETY: HWND vivo (não fechado). As regiões criadas aqui são apagadas aqui, menos a final,
        // que passa a ser da janela (o Windows apaga quando trocar ou fechar).
        unsafe {
            if holes.is_empty() {
                let _ = SetWindowRgn(hwnd, None, true);
                return;
            }
            // Retângulo grande o bastante para qualquer tamanho: a região não depende do tamanho.
            let region = CreateRectRgn(0, 0, i16::MAX as i32, i16::MAX as i32);
            for h in holes {
                let hole = CreateRectRgn(h.x, h.y, h.x + h.width as i32, h.y + h.height as i32);
                let _ = CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF);
                let _ = DeleteObject(HGDIOBJ(hole.0));
            }
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
            }
        }
    }

    /// Recebe os eventos de tela cheia (substitui o callback anterior).
    pub fn on_event(&self, cb: impl Fn(WindowEvent) + Send + Sync + 'static) {
        *self.shared.on_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(cb));
    }

    fn position(&self, x: i32, y: i32, w: i32, h: i32, flags: SET_WINDOW_POS_FLAGS) {
        if self.shared.closed.load(Ordering::Relaxed) {
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
        })
    }
}

impl Drop for VideoWindow {
    fn drop(&mut self) {
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
        let window = Size::new(
            self.shared.width.load(Ordering::Relaxed),
            self.shared.height.load(Ordering::Relaxed),
        );
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
                WindowTarget::Embedded { parent } => (
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
    ToggleFullscreen,
    ExitFullscreen,
    Close,
    Destroyed,
    /// O padrão do Windows.
    Default,
}

fn action(msg: u32, key: usize, embedded: bool) -> Action {
    match msg {
        WM_SIZE => Action::StoreSize,
        WM_ERASEBKGND => Action::SkipErase,
        WM_LBUTTONDOWN if embedded => Action::Focus,
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
        match action(msg, wparam.0, embedded) {
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
        assert_eq!(action(WM_SIZE, 0, false), Action::StoreSize);
        assert_eq!(action(WM_ERASEBKGND, 0, true), Action::SkipErase);
        assert_eq!(action(WM_LBUTTONDOWN, 0, true), Action::Focus);
        assert_eq!(
            action(WM_LBUTTONDOWN, 0, false),
            Action::Default,
            "solta: clique normal"
        );
        assert_eq!(action(WM_KEYDOWN, f11, false), Action::ToggleFullscreen);
        assert_eq!(action(WM_KEYDOWN, f11, true), Action::ToggleFullscreen);
        assert_eq!(action(WM_KEYDOWN, esc, true), Action::ExitFullscreen);
        assert_eq!(
            action(WM_KEYDOWN, esc, false),
            Action::Default,
            "solta: Esc não faz nada"
        );
        assert_eq!(action(WM_KEYDOWN, letra_a, true), Action::Default);
        assert_eq!(action(WM_LBUTTONDBLCLK, 0, false), Action::ToggleFullscreen);
        assert_eq!(action(WM_CLOSE, 0, false), Action::Close);
        assert_eq!(action(WM_DESTROY, 0, false), Action::Destroyed);
        assert_eq!(action(0x0200, 0, false), Action::Default);
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
