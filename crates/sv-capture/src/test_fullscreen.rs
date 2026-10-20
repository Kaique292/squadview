//! "Jogo" de teste em tela cheia: uma janela que desenha com D3D11 sem parar, como um jogo, em
//! tela cheia sem bordas ou exclusiva (`SetFullscreenState`). Só existe nos testes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use sv_gpu::Gpu;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT, DXGI_SWAP_EFFECT_DISCARD, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGIFactory2, IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassExW,
    SW_SHOW, SetForegroundWindow, ShowWindow, TranslateMessage, WM_DESTROY, WM_QUIT, WNDCLASSEXW, WS_POPUP,
};
use windows::core::w;

/// Como o "jogo" ocupa a tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Janela sem bordas do tamanho do monitor (swapchain flip, como jogos DX12).
    Borderless,
    /// Tela cheia exclusiva de verdade (`SetFullscreenState`, swapchain de modelo antigo,
    /// como muitos jogos DX11). O Windows 10/11 pode transformá-la em "tela cheia otimizada".
    Exclusive,
}

pub(crate) struct FullscreenGame {
    pub hwnd: isize,
    /// A tela cheia exclusiva foi aceita pelo DXGI.
    pub exclusive: bool,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FullscreenGame {
    pub fn open(mode: Mode) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_render = stop.clone();
        let thread = std::thread::spawn(move || {
            // SAFETY: janela, swapchain e fila de mensagens criadas e usadas só nesta thread; os
            // ponteiros passados ao Windows são locais que vivem durante cada chamada.
            unsafe {
                let instance = GetModuleHandleW(None).unwrap();
                let class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance.into(),
                    lpszClassName: w!("SquadViewTesteTelaCheia"),
                    ..Default::default()
                };
                RegisterClassExW(&class);
                let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(monitor, &mut info);
                let r = info.rcMonitor;
                let (width, height) = (r.right - r.left, r.bottom - r.top);
                let hwnd = CreateWindowExW(
                    Default::default(),
                    w!("SquadViewTesteTelaCheia"),
                    w!("SquadView (jogo em tela cheia)"),
                    WS_POPUP,
                    r.left,
                    r.top,
                    width,
                    height,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .unwrap();
                let _ = ShowWindow(hwnd, SW_SHOW);
                bring_to_front(hwnd);

                let gpu = Gpu::new().unwrap();
                let effect: DXGI_SWAP_EFFECT = match mode {
                    Mode::Borderless => DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    Mode::Exclusive => DXGI_SWAP_EFFECT_DISCARD,
                };
                let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: width as u32,
                    Height: height as u32,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: effect,
                    AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
                    ..Default::default()
                };
                let swapchain: IDXGISwapChain1 = factory
                    .CreateSwapChainForHwnd(&gpu.device, hwnd, &desc, None, None)
                    .unwrap();
                let exclusive = mode == Mode::Exclusive && swapchain.SetFullscreenState(true, None).is_ok();
                if exclusive {
                    let _ = swapchain.ResizeBuffers(0, 0, 0, DXGI_FORMAT_B8G8R8A8_UNORM, Default::default());
                }
                tx.send((hwnd.0 as isize, exclusive)).unwrap();

                let mut frame = 0u32;
                let mut msg = MSG::default();
                while !stop_render.load(Ordering::Relaxed) {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        if msg.message == WM_QUIT {
                            return;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let back: ID3D11Texture2D = swapchain.GetBuffer(0).unwrap();
                    let mut rtv: Option<ID3D11RenderTargetView> = None;
                    gpu.device.CreateRenderTargetView(&back, None, Some(&mut rtv)).unwrap();
                    // Cor muda a cada quadro, como um jogo (a captura só recebe quadros quando a imagem muda).
                    let t = (frame % 120) as f32 / 120.0;
                    gpu.context
                        .ClearRenderTargetView(rtv.as_ref().unwrap(), &[t, 0.2, 1.0 - t, 1.0]);
                    let _ = swapchain.Present(1, DXGI_PRESENT(0));
                    frame = frame.wrapping_add(1);
                }
                if exclusive {
                    let _ = swapchain.SetFullscreenState(false, None);
                }
                drop(swapchain);
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
        });
        let (hwnd, exclusive) = rx.recv().unwrap();
        Self {
            hwnd,
            exclusive,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for FullscreenGame {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Põe a janela na frente. O Windows só deixa quem recebeu a última entrada do usuário mudar o
/// primeiro plano; um toque de Alt sintético conta como entrada (o truque de sempre dos testes).
/// Sem isso o DXGI recusa a tela cheia exclusiva.
fn bring_to_front(hwnd: HWND) {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MENU,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: `inputs` é um array local com o tamanho informado; HWND vivo, desta thread.
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: repassa para o padrão do Windows; WM_DESTROY encerra a fila desta thread.
    unsafe {
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
