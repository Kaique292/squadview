//! Janela de verdade para os testes (captura, "a fonte ainda existe?"). Só existe nos testes.

use std::sync::{Once, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW, PostQuitMessage,
    RegisterClassExW, SW_HIDE, SW_SHOW, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE,
    WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, w};

/// Janela comum, visível, com a própria fila de mensagens. Fecha ao ser descartada.
#[derive(Debug)]
pub(crate) struct TestWindow {
    pub hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

impl TestWindow {
    pub fn open(title: &str, width: i32, height: i32) -> Self {
        let (tx, rx) = mpsc::channel();
        let title = HSTRING::from(title);
        let thread = std::thread::spawn(move || {
            static REGISTER: Once = Once::new();
            // SAFETY: registro de classe, criação da janela e fila de mensagens na mesma thread.
            unsafe {
                let instance = GetModuleHandleW(None).unwrap();
                REGISTER.call_once(|| {
                    let class = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        lpfnWndProc: Some(wnd_proc),
                        hInstance: instance.into(),
                        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
                        lpszClassName: w!("SquadViewTesteCaptura"),
                        ..Default::default()
                    };
                    RegisterClassExW(&class);
                });
                let hwnd = CreateWindowExW(
                    Default::default(),
                    w!("SquadViewTesteCaptura"),
                    &title,
                    WS_OVERLAPPEDWINDOW,
                    100,
                    100,
                    width,
                    height,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .unwrap();
                let _ = ShowWindow(hwnd, SW_SHOW);
                tx.send(hwnd.0 as isize).unwrap();
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
        Self {
            hwnd: rx.recv().unwrap(),
            thread: Some(thread),
        }
    }

    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    pub fn resize(&self, width: i32, height: i32) {
        // SAFETY: janela viva; o Windows repassa a mudança para a thread dela.
        unsafe { SetWindowPos(self.handle(), None, 0, 0, width, height, SWP_NOMOVE | SWP_NOZORDER).unwrap() };
    }

    pub fn hide(&self) {
        // SAFETY: janela viva.
        unsafe {
            let _ = ShowWindow(self.handle(), SW_HIDE);
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: a janela ainda existe; WM_CLOSE é tratado na thread dela.
        unsafe {
            let _ = PostMessageW(Some(self.handle()), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
