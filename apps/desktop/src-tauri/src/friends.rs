//! Amigos e presença (ver `docs/adr/0011-amigos-e-presenca.md`).
//!
//! No login, o token do Discord é trocado no Worker por uma sessão do SquadView, guardada em
//! `session.json`; o token do Discord é descartado. Com a sessão, o app fica conectado ao próprio
//! registro (amigos, pedidos, presença) e reconecta sozinho quando a rede cai.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sv_net::friends::{FriendAction, FriendsClient, FriendsConnectError, FriendsEvent};
use tauri::{AppHandle, Emitter, Manager, State};

/// Mensagem do registro (JSON), repassada à interface.
pub const FRIENDS_EVENT: &str = "friends-event";
/// Estado da ligação: "signed-out", "connecting" ou "online".
pub const FRIENDS_STATUS_EVENT: &str = "friends-status";

/// Espera entre tentativas de reconectar, em segundos.
const RECONNECT_DELAYS_S: [u64; 6] = [1, 2, 4, 8, 15, 30];

#[derive(Default)]
pub struct Friends(Mutex<Running>);

#[derive(Default)]
struct Running {
    client: Option<FriendsClient>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    status: Status,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    SignedOut,
    Connecting,
    Online,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    session: String,
}

fn lock(state: &Friends) -> std::sync::MutexGuard<'_, Running> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Endereço HTTP do Worker (o da sinalização, trocando `ws` por `http`).
fn http_base() -> String {
    let url = crate::room::signaling_url();
    match url.split_once("://") {
        Some(("wss", rest)) => format!("https://{rest}"),
        Some(("ws", rest)) => format!("http://{rest}"),
        _ => url,
    }
}

/// Troca o token do Discord por uma sessão do SquadView. `None` se o Worker não aceitar (sem o
/// segredo configurado ou fora do ar): o login segue, só os amigos ficam de fora.
pub async fn exchange(http: &reqwest::Client, access_token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Reply {
        session: String,
    }
    let reply: Reply = http
        .post(format!("{}/auth/discord", http_base()))
        .json(&serde_json::json!({ "accessToken": access_token }))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    Some(reply.session)
}

fn session_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("session.json"))
}

/// Sessão salva, se houver.
pub fn session(app: &AppHandle) -> Option<String> {
    let data = std::fs::read(session_path(app)?).ok()?;
    serde_json::from_slice::<SavedSession>(&data).ok().map(|s| s.session)
}

/// Guarda a sessão (ou apaga, com `None`).
pub fn save_session(app: &AppHandle, session: Option<&str>) {
    let Some(path) = session_path(app) else { return };
    match session {
        Some(s) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_vec(&SavedSession { session: s.to_owned() }) {
                let _ = std::fs::write(&path, json);
            }
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn set_status(app: &AppHandle, status: Status) {
    lock(&app.state::<Friends>()).status = status;
    let _ = app.emit(FRIENDS_STATUS_EVENT, status);
}

/// Conecta ao registro (ou reconecta, depois de um login). Sem sessão, fica "signed-out".
pub fn start(app: &AppHandle) {
    stop(app);
    if session(app).is_none() {
        set_status(app, Status::SignedOut);
        return;
    }
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move { run(task_app).await });
    lock(&app.state::<Friends>()).task = Some(task);
}

/// Desconecta (ao sair da conta).
pub fn stop(app: &AppHandle) {
    let (task, client) = {
        let state = app.state::<Friends>();
        let mut running = lock(&state);
        (running.task.take(), running.client.take())
    };
    if let Some(task) = task {
        task.abort();
    }
    drop(client);
    set_status(app, Status::SignedOut);
}

async fn run(app: AppHandle) {
    let url = crate::room::signaling_url();
    let mut attempt = 0usize;
    loop {
        let Some(session) = session(&app) else {
            set_status(&app, Status::SignedOut);
            return;
        };
        set_status(&app, Status::Connecting);
        match FriendsClient::connect(&url, &session).await {
            Ok((client, mut events)) => {
                attempt = 0;
                lock(&app.state::<Friends>()).client = Some(client);
                set_status(&app, Status::Online);
                while let Some(event) = events.recv().await {
                    match event {
                        FriendsEvent::Message(value) => {
                            let _ = app.emit(FRIENDS_EVENT, value);
                        }
                        FriendsEvent::Session(fresh) => save_session(&app, Some(&fresh)),
                        FriendsEvent::Disconnected => break,
                    }
                }
                lock(&app.state::<Friends>()).client = None;
            }
            // Sessão vencida ou recusada: é preciso entrar com o Discord de novo.
            Err(FriendsConnectError::Unauthorized) => {
                save_session(&app, None);
                set_status(&app, Status::SignedOut);
                return;
            }
            Err(FriendsConnectError::Unavailable(_)) => {}
        }
        set_status(&app, Status::Connecting);
        let delay = RECONNECT_DELAYS_S[attempt.min(RECONNECT_DELAYS_S.len() - 1)];
        attempt += 1;
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

/// Uma ação da interface (pedir amizade, entrar na sala de um amigo, avisar a sala atual...).
#[tauri::command]
pub fn friends_action(state: State<'_, Friends>, action: FriendAction) -> Result<(), String> {
    let running = lock(&state);
    let client = running
        .client
        .as_ref()
        .ok_or("Sem conexão com os amigos agora. Tente de novo.")?;
    client.send(&action);
    Ok(())
}

/// Estado da ligação agora (a interface pergunta ao abrir).
#[tauri::command]
pub fn friends_status(state: State<'_, Friends>) -> Status {
    lock(&state).status
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_vai_como_texto_para_a_interface() {
        assert_eq!(
            serde_json::to_string(&super::Status::SignedOut).unwrap(),
            r#""signed-out""#
        );
        assert_eq!(serde_json::to_string(&super::Status::Online).unwrap(), r#""online""#);
    }
}
