//! "Controle total" do fliperama (ADR 0024): o teclado e o mouse de verdade de um jogador, com a
//! permissão do host, injetados no PC do host só enquanto a janela do jogo está em primeiro plano.
//!
//! - [`vk_from_code`]: a tecla da página (`KeyboardEvent.code`) no código virtual do Windows.
//! - [`KbmGuard`]: lógica pura. Bloqueia as teclas do sistema e lembra o que está apertado, para
//!   soltar tudo ao cortar, cair ou trocar de janela.
//! - [`Injector`]: manda os eventos ao Windows (`SendInput`), conferindo o primeiro plano.
//! - [`PanicHotkey`]: o atalho global Ctrl+Alt+F12 que corta o controle total na hora.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MapVirtualKeyW, RegisterHotKey, SendInput, UnregisterHotKey,
    VIRTUAL_KEY, VK_F12,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ClipCursor, GA_ROOT, GetAncestor, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW, MSG,
    PostThreadMessageW, WM_HOTKEY, WM_QUIT, WindowFromPoint,
};

use crate::wire::{KbmEvent, MouseButton};

// Códigos virtuais usados nas regras (os mesmos do Windows).
const VK_TAB: u16 = 0x09;
const VK_SHIFT_L: u16 = 0xA0;
const VK_SHIFT_R: u16 = 0xA1;
const VK_CTRL_L: u16 = 0xA2;
const VK_CTRL_R: u16 = 0xA3;
const VK_ALT_L: u16 = 0xA4;
const VK_ALT_R: u16 = 0xA5;
const VK_ESC: u16 = 0x1B;
const VK_F4: u16 = 0x73;
const VK_WIN_L: u16 = 0x5B;
const VK_WIN_R: u16 = 0x5C;
const VK_APPS: u16 = 0x5D;

/// A tecla da página (`KeyboardEvent.code`) no código virtual do Windows, com a marca de tecla
/// "estendida" (setas, Insert, Ctrl e Alt da direita...). `None` para teclas que não repassamos.
pub fn vk_from_code(code: &str) -> Option<(u16, bool)> {
    let plain = |vk: u16| Some((vk, false));
    let ext = |vk: u16| Some((vk, true));
    if let Some(letter) = code.strip_prefix("Key")
        && letter.len() == 1
        && letter.as_bytes()[0].is_ascii_uppercase()
    {
        return plain(u16::from(letter.as_bytes()[0]));
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return plain(u16::from(digit.as_bytes()[0]));
    }
    if let Some(n) = code.strip_prefix("Numpad").and_then(|d| d.parse::<u16>().ok())
        && n <= 9
    {
        return plain(0x60 + n);
    }
    if let Some(n) = code.strip_prefix('F').and_then(|d| d.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return plain(0x6F + n);
    }
    match code {
        "Space" => plain(0x20),
        "Enter" => plain(0x0D),
        "NumpadEnter" => ext(0x0D),
        "Escape" => plain(VK_ESC),
        "Tab" => plain(VK_TAB),
        "Backspace" => plain(0x08),
        "CapsLock" => plain(0x14),
        "ShiftLeft" => plain(VK_SHIFT_L),
        "ShiftRight" => plain(VK_SHIFT_R),
        "ControlLeft" => plain(VK_CTRL_L),
        "ControlRight" => ext(VK_CTRL_R),
        "AltLeft" => plain(VK_ALT_L),
        "AltRight" => ext(VK_ALT_R),
        "MetaLeft" => ext(VK_WIN_L),
        "MetaRight" => ext(VK_WIN_R),
        "ContextMenu" => ext(VK_APPS),
        "ArrowLeft" => ext(0x25),
        "ArrowUp" => ext(0x26),
        "ArrowRight" => ext(0x27),
        "ArrowDown" => ext(0x28),
        "Insert" => ext(0x2D),
        "Delete" => ext(0x2E),
        "Home" => ext(0x24),
        "End" => ext(0x23),
        "PageUp" => ext(0x21),
        "PageDown" => ext(0x22),
        "Minus" => plain(0xBD),
        "Equal" => plain(0xBB),
        "BracketLeft" => plain(0xDB),
        "BracketRight" => plain(0xDD),
        "Backslash" => plain(0xDC),
        "Semicolon" => plain(0xBA),
        "Quote" => plain(0xDE),
        "Backquote" => plain(0xC0),
        "Comma" => plain(0xBC),
        "Period" => plain(0xBE),
        "Slash" => plain(0xBF),
        "IntlBackslash" => plain(0xE2),
        // "/" do teclado ABNT2.
        "IntlRo" => plain(0xC1),
        "NumpadAdd" => plain(0x6B),
        "NumpadSubtract" => plain(0x6D),
        "NumpadMultiply" => plain(0x6A),
        "NumpadDivide" => ext(0x6F),
        "NumpadDecimal" => plain(0x6E),
        "NumpadComma" => plain(0xC2),
        _ => None,
    }
}

/// Regras do controle total, sem tocar no Windows: o que pode passar e o que está apertado.
#[derive(Debug, Default)]
pub struct KbmGuard {
    keys: HashSet<(u16, bool)>,
    buttons: HashSet<MouseButton>,
}

impl KbmGuard {
    fn held(&self, vk: u16) -> bool {
        self.keys.iter().any(|(k, _)| *k == vk)
    }

    fn alt(&self) -> bool {
        self.held(VK_ALT_L) || self.held(VK_ALT_R)
    }

    fn ctrl(&self) -> bool {
        self.held(VK_CTRL_L) || self.held(VK_CTRL_R)
    }

    /// O evento pode ir ao Windows? Bloqueia as teclas do sistema (Windows e menu) e os atalhos que
    /// tiram o foco do jogo (Alt+Tab, Alt+F4, Alt+Esc, Ctrl+Esc, Ctrl+Shift+Esc). Soltar uma tecla
    /// que passou sempre passa. Atualiza o que está apertado.
    pub fn filter(&mut self, event: KbmEvent) -> Option<KbmEvent> {
        match event {
            KbmEvent::Key { vk, extended, down } => {
                if !down {
                    return self.keys.remove(&(vk, extended)).then_some(event);
                }
                let blocked = matches!(vk, VK_WIN_L | VK_WIN_R | VK_APPS)
                    || (self.alt() && matches!(vk, VK_TAB | VK_F4 | VK_ESC))
                    || (self.ctrl() && vk == VK_ESC);
                if blocked {
                    return None;
                }
                self.keys.insert((vk, extended));
                Some(event)
            }
            KbmEvent::Button { button, down } => {
                if down {
                    self.buttons.insert(button);
                    Some(event)
                } else {
                    self.buttons.remove(&button).then_some(event)
                }
            }
            KbmEvent::Move { .. } | KbmEvent::Wheel { .. } => Some(event),
        }
    }

    /// Os eventos que soltam tudo que está apertado (e esquece o que estava).
    pub fn release_all(&mut self) -> Vec<KbmEvent> {
        let mut out: Vec<KbmEvent> = self
            .keys
            .drain()
            .map(|(vk, extended)| KbmEvent::Key {
                vk,
                extended,
                down: false,
            })
            .collect();
        out.extend(
            self.buttons
                .drain()
                .map(|button| KbmEvent::Button { button, down: false }),
        );
        out
    }

    pub fn is_idle(&self) -> bool {
        self.keys.is_empty() && self.buttons.is_empty()
    }
}

/// Manda o teclado e o mouse do jogador ao Windows, só com a janela do jogo em primeiro plano.
#[derive(Debug)]
pub struct Injector {
    /// A janela do jogo (a que o fliperama captura).
    game: isize,
    guard: KbmGuard,
}

impl Injector {
    pub fn new(game_window: isize) -> Self {
        Self {
            game: game_window,
            guard: KbmGuard::default(),
        }
    }

    /// A janela do jogo está em primeiro plano agora?
    pub fn game_in_front(&self) -> bool {
        // SAFETY: sem argumentos; só lê a janela em primeiro plano.
        let front = unsafe { GetForegroundWindow() };
        front.0 as isize == self.game
    }

    /// Repassa um evento do jogador. Com o jogo fora do primeiro plano, não passa nada e o que
    /// estava apertado é solto (ADR 0024). Devolve se o evento foi ao Windows.
    ///
    /// O mouse fica preso na janela do jogo enquanto o jogador mexe nele, e um clique com o ponteiro
    /// fora dela (na barra de tarefas, na área de trabalho, em outro programa) não passa: antes, o
    /// primeiro clique fora acontecia de verdade, e só depois o jogo deixava de estar na frente.
    pub fn handle(&mut self, event: KbmEvent) -> bool {
        if !self.game_in_front() {
            self.release_all();
            return false;
        }
        if matches!(event, KbmEvent::Move { .. } | KbmEvent::Button { down: true, .. }) {
            self.confine();
            if matches!(event, KbmEvent::Button { .. }) && !self.cursor_over_game() {
                return false;
            }
        }
        match self.guard.filter(event) {
            Some(event) => {
                send(&[event]);
                true
            }
            None => false,
        }
    }

    /// Prende o ponteiro na área da janela do jogo. O Windows solta a trava quando outra janela vem
    /// para frente, então ela é refeita a cada movimento.
    fn confine(&self) {
        let hwnd = HWND(self.game as *mut _);
        let mut client = RECT::default();
        // SAFETY: `hwnd` é a janela do jogo (pode ter fechado: aí as chamadas só falham).
        unsafe {
            if GetClientRect(hwnd, &mut client).is_err() {
                return;
            }
            let mut top_left = POINT {
                x: client.left,
                y: client.top,
            };
            let mut bottom_right = POINT {
                x: client.right,
                y: client.bottom,
            };
            if !ClientToScreen(hwnd, &mut top_left).as_bool() || !ClientToScreen(hwnd, &mut bottom_right).as_bool() {
                return;
            }
            let area = RECT {
                left: top_left.x,
                top: top_left.y,
                right: bottom_right.x,
                bottom: bottom_right.y,
            };
            let _ = ClipCursor(Some(&area));
        }
    }

    /// O ponteiro está em cima da janela do jogo (ou de algo dentro dela)?
    fn cursor_over_game(&self) -> bool {
        let mut point = POINT::default();
        // SAFETY: só leituras da posição do ponteiro e da janela embaixo dele.
        unsafe {
            if GetCursorPos(&mut point).is_err() {
                return false;
            }
            let under = WindowFromPoint(point);
            GetAncestor(under, GA_ROOT).0 as isize == self.game
        }
    }

    /// Solta tudo que o jogador estava apertando (corte, queda ou troca de janela) e o ponteiro.
    pub fn release_all(&mut self) {
        let events = self.guard.release_all();
        if !events.is_empty() {
            send(&events);
        }
        // SAFETY: sem área: tira a trava do ponteiro.
        unsafe {
            let _ = ClipCursor(None);
        }
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
    }
}

fn send(events: &[KbmEvent]) {
    let inputs: Vec<INPUT> = events.iter().map(to_input).collect();
    // SAFETY: `inputs` é um slice válido de INPUT, e o tamanho é o de um INPUT.
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn to_input(event: &KbmEvent) -> INPUT {
    match *event {
        KbmEvent::Key { vk, extended, down } => {
            // Pelo scancode: muitos jogos (DirectInput) leem o scancode, não o código virtual.
            // SAFETY: só traduz o código; sem ponteiros.
            let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16;
            let mut flags = KEYEVENTF_SCANCODE;
            if extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if !down {
                flags |= KEYEVENTF_KEYUP;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        }
        KbmEvent::Move { dx, dy } => mouse(i32::from(dx), i32::from(dy), 0, MOUSEEVENTF_MOVE),
        KbmEvent::Button { button, down } => {
            let flags = match (button, down) {
                (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
            };
            mouse(0, 0, 0, flags)
        }
        // O "mouseData" da rolagem é um DWORD com o valor com sinal dentro.
        KbmEvent::Wheel { delta } => mouse(0, 0, i32::from(delta) as u32, MOUSEEVENTF_WHEEL),
    }
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// O atalho global Ctrl+Alt+F12 do host: corta o controle total mesmo com o jogo em tela cheia.
/// Para ao ser descartado.
#[derive(Debug)]
pub struct PanicHotkey {
    thread_id: std::sync::Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl PanicHotkey {
    /// Registra o atalho; `on_press` roda (na thread do atalho) a cada vez que ele é apertado.
    /// Falha se outro programa já usa Ctrl+Alt+F12.
    pub fn start(on_press: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let thread_id = std::sync::Arc::new(AtomicU32::new(0));
        let id_out = thread_id.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = std::thread::Builder::new()
            .name("sv-arcade-hotkey".into())
            .spawn(move || {
                // SAFETY: registro de atalho da thread atual (sem janela); desfeito no fim.
                let registered = unsafe {
                    id_out.store(GetCurrentThreadId(), Ordering::SeqCst);
                    RegisterHotKey(None, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, u32::from(VK_F12.0))
                };
                if let Err(e) = registered {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                let mut msg = MSG::default();
                // SAFETY: laço de mensagens da própria thread; `msg` vive durante cada chamada.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                    if msg.message == WM_HOTKEY {
                        on_press();
                    }
                }
                // SAFETY: o atalho foi registrado nesta thread.
                unsafe {
                    let _ = UnregisterHotKey(None, 1);
                }
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(std::io::Error::other(e))
            }
            Err(_) => Err(std::io::Error::other("o atalho não respondeu")),
        }
    }
}

impl Drop for PanicHotkey {
    fn drop(&mut self) {
        let id = self.thread_id.load(Ordering::SeqCst);
        if id != 0 {
            // SAFETY: manda WM_QUIT para a thread do atalho, que sai do laço.
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A janela em primeiro plano agora (para o host saber qual "a janela do jogo" quando não
/// captura uma janela específica, e para testes).
pub fn foreground_window() -> isize {
    // SAFETY: sem argumentos.
    let front: HWND = unsafe { GetForegroundWindow() };
    front.0 as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: u16, down: bool) -> KbmEvent {
        KbmEvent::Key {
            vk,
            extended: false,
            down,
        }
    }

    #[test]
    fn teclas_da_pagina_viram_codigos_do_windows() {
        assert_eq!(vk_from_code("KeyW"), Some((0x57, false)));
        assert_eq!(vk_from_code("Digit7"), Some((0x37, false)));
        assert_eq!(vk_from_code("F12"), Some((0x7B, false)));
        assert_eq!(vk_from_code("Numpad5"), Some((0x65, false)));
        assert_eq!(vk_from_code("ArrowLeft"), Some((0x25, true)), "seta é estendida");
        assert_eq!(vk_from_code("ControlRight"), Some((VK_CTRL_R, true)));
        assert_eq!(vk_from_code("IntlRo"), Some((0xC1, false)), "o / do ABNT2");
        assert_eq!(vk_from_code("Keyw"), None);
        assert_eq!(vk_from_code("F25"), None);
        assert_eq!(vk_from_code("Inventada"), None);
    }

    #[test]
    fn bloqueia_windows_e_atalhos_que_tiram_o_jogo_da_frente() {
        let mut guard = KbmGuard::default();
        assert_eq!(guard.filter(key(VK_WIN_L, true)), None);
        assert_eq!(guard.filter(key(VK_APPS, true)), None);
        assert!(
            guard.filter(key(VK_ALT_L, true)).is_some(),
            "Alt sozinho passa (jogos usam)"
        );
        assert_eq!(guard.filter(key(VK_TAB, true)), None, "Alt+Tab");
        assert_eq!(guard.filter(key(VK_F4, true)), None, "Alt+F4");
        assert_eq!(guard.filter(key(VK_ESC, true)), None, "Alt+Esc");
        guard.filter(key(VK_ALT_L, false));
        assert!(guard.filter(key(VK_TAB, true)).is_some(), "Tab sem Alt passa");
        guard.filter(key(VK_CTRL_L, true));
        assert_eq!(guard.filter(key(VK_ESC, true)), None, "Ctrl+Esc e Ctrl+Shift+Esc");
    }

    #[test]
    fn soltar_tecla_que_nao_passou_nao_vai_ao_windows() {
        let mut guard = KbmGuard::default();
        guard.filter(key(VK_WIN_L, true));
        assert_eq!(guard.filter(key(VK_WIN_L, false)), None);
        assert!(guard.filter(key(0x41, true)).is_some());
        assert!(guard.filter(key(0x41, false)).is_some());
        assert!(guard.is_idle());
    }

    #[test]
    fn soltar_tudo_solta_teclas_e_botoes_apertados() {
        let mut guard = KbmGuard::default();
        guard.filter(key(0x57, true));
        guard.filter(KbmEvent::Button {
            button: MouseButton::Left,
            down: true,
        });
        guard.filter(KbmEvent::Move { dx: 3, dy: 4 });
        let mut released = guard.release_all();
        released.sort_by_key(|e| matches!(e, KbmEvent::Button { .. }));
        assert_eq!(
            released,
            vec![
                key(0x57, false),
                KbmEvent::Button {
                    button: MouseButton::Left,
                    down: false
                }
            ]
        );
        assert!(guard.is_idle());
        assert!(guard.release_all().is_empty());
    }

    #[test]
    fn clique_fora_do_jogo_nao_passa() {
        // A "janela do jogo" que não existe nunca está embaixo do ponteiro, nem na frente.
        let mut injector = Injector::new(0x7fff_0003);
        assert!(!injector.cursor_over_game());
        assert!(!injector.handle(KbmEvent::Button {
            button: MouseButton::Left,
            down: true
        }));
        injector.release_all();
    }

    #[test]
    fn sem_o_jogo_na_frente_nada_passa() {
        // Uma "janela do jogo" que não existe nunca está em primeiro plano.
        let mut injector = Injector::new(0x7fff_0001);
        assert!(!injector.handle(key(0x41, true)));
        assert!(injector.guard.is_idle(), "nada ficou apertado");
    }

    #[test]
    fn atalho_de_corte_registra_e_solta() {
        // Outro programa pode já usar Ctrl+Alt+F12: aí o registro falha com erro, sem travar.
        if let Ok(hotkey) = PanicHotkey::start(|| {}) {
            drop(hotkey);
        }
    }
}
