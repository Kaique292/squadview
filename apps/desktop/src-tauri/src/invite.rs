//! Convites por link: `squadview://join/CODIGO` abre o app (ou traz a janela para a frente) e
//! mostra o modal de entrar na sala já com o código. Nunca entra sozinho: quem decide é a pessoa,
//! para um link qualquer não colocar ninguém numa sala sem querer.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Evento para a interface com o código do convite.
pub const INVITE_EVENT: &str = "invite";

/// Convite que chegou antes de a interface estar pronta para ouvir.
#[derive(Default)]
pub struct PendingInvite(Mutex<Option<String>>);

/// A interface pega o convite que abriu o app (se houver).
#[tauri::command]
pub fn take_pending_invite(state: tauri::State<'_, PendingInvite>) -> Option<String> {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Trata as URLs recebidas pelo sistema (ao abrir o app ou com ele já aberto).
pub fn handle_urls<'a, R: Runtime>(app: &AppHandle<R>, urls: impl IntoIterator<Item = &'a str>) {
    let Some(code) = urls.into_iter().find_map(parse_invite) else {
        return;
    };
    *app.state::<PendingInvite>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.clone());
    let _ = app.emit(INVITE_EVENT, code);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

/// `squadview://join/ABC234` (ou com `/` no fim, hífen, minúsculas) → código normalizado.
fn parse_invite(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("squadview://")?;
    let rest = rest.strip_prefix("join/")?;
    let code = rest.split(['/', '?', '#']).next()?;
    sv_net::signaling::normalize_room_code(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn link_de_convite_guarda_o_codigo_e_avisa_a_interface() {
        let t = test_app();
        let events = t.events(INVITE_EVENT);
        handle_urls(t.handle(), ["https://outra.coisa", "squadview://join/f5b-6s9"]);
        events.wait("convite", |v| v == "F5B6S9");
        let pending = t.app.state::<PendingInvite>();
        assert_eq!(take_pending_invite(pending).as_deref(), Some("F5B6S9"));
        // Pegou uma vez: não fica repetindo.
        assert_eq!(take_pending_invite(t.app.state::<PendingInvite>()), None);
        // Sem convite nas URLs: nada muda.
        handle_urls(t.handle(), ["squadview://outra"]);
        assert_eq!(take_pending_invite(t.app.state::<PendingInvite>()), None);
    }

    #[test]
    fn le_o_codigo_do_link() {
        assert_eq!(parse_invite("squadview://join/F5B6S9").as_deref(), Some("F5B6S9"));
        assert_eq!(parse_invite("squadview://join/f5b-6s9/").as_deref(), Some("F5B6S9"));
    }

    #[test]
    fn recusa_links_que_nao_sao_convite() {
        assert_eq!(parse_invite("squadview://join/"), None);
        assert_eq!(parse_invite("squadview://join/ABC"), None);
        assert_eq!(parse_invite("squadview://outra/F5B6S9"), None);
        assert_eq!(parse_invite("https://join/F5B6S9"), None);
        assert_eq!(parse_invite("squadview://join/F5B6S9X"), None);
    }
}
