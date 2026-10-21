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
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// Mensagem do registro (JSON), repassada à interface.
pub const FRIENDS_EVENT: &str = "friends-event";
/// Estado da ligação: "signed-out", "connecting" ou "online".
pub const FRIENDS_STATUS_EVENT: &str = "friends-status";

/// Espera entre tentativas de reconectar, em segundos.
const RECONNECT_DELAYS_S: [u64; 6] = [1, 2, 4, 8, 15, 30];
/// Espera entre tentativas de trocar o token do Discord pela sessão, em segundos (uns 5 minutos
/// no total). O Discord já aceitou o token, então a falha é passageira: o Worker fora do ar ou o
/// Discord limitando os pedidos vindos da Cloudflare.
const EXCHANGE_DELAYS_S: [u64; 9] = [2, 5, 10, 20, 30, 60, 60, 60, 60];

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
    http_base_of(&crate::room::signaling_url())
}

fn http_base_of(url: &str) -> String {
    match url.split_once("://") {
        Some(("wss", rest)) => format!("https://{rest}"),
        Some(("ws", rest)) => format!("http://{rest}"),
        _ => url.to_owned(),
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

/// Depois do login com o Discord: troca o token pela sessão e liga os amigos e a conferência na
/// sala atual. Se a troca falhar, o login vale do mesmo jeito e o app continua tentando em
/// segundo plano (os amigos ficam em "conectando"), em vez de pedir para a pessoa entrar de novo.
pub async fn activate<R: Runtime>(app: &AppHandle<R>, http: reqwest::Client, token: String) {
    stop(app);
    // A sessão antiga pode ser de outra conta: só vale a desta conta.
    save_session(app, None);
    if let Some(session) = exchange(&http, &token).await {
        return signed_in(app, &session);
    }
    set_status(app, Status::Connecting);
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let delays = EXCHANGE_DELAYS_S.map(Duration::from_secs);
        let session = retry(&delays, || exchange(&http, &token)).await;
        drop(token);
        // Esta tarefa termina aqui: sai do lugar antes de `start` ocupá-lo com a dos amigos.
        lock(&task_app.state::<Friends>()).task = None;
        match session {
            Some(session) => signed_in(&task_app, &session),
            None => set_status(&task_app, Status::SignedOut),
        }
    });
    lock(&app.state::<Friends>()).task = Some(task);
}

/// Com a sessão: guarda, conecta ao registro e confere a conta na sala em que estamos (os outros
/// passam a nos ver como conferidos e entramos nos recentes deles, sem sair da sala).
fn signed_in<R: Runtime>(app: &AppHandle<R>, session: &str) {
    save_session(app, Some(session));
    start(app);
    crate::room::verify_in_room(app, session);
}

/// Tenta de novo depois de cada espera, até dar certo ou acabarem as esperas.
async fn retry<T, F, Fut>(delays: &[Duration], mut attempt: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for delay in delays {
        tokio::time::sleep(*delay).await;
        if let Some(value) = attempt().await {
            return Some(value);
        }
    }
    None
}

fn session_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("session.json"))
}

/// Sessão salva, se houver.
pub fn session<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let data = std::fs::read(session_path(app)?).ok()?;
    serde_json::from_slice::<SavedSession>(&data).ok().map(|s| s.session)
}

/// Guarda a sessão (ou apaga, com `None`).
pub fn save_session<R: Runtime>(app: &AppHandle<R>, session: Option<&str>) {
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

fn set_status<R: Runtime>(app: &AppHandle<R>, status: Status) {
    lock(&app.state::<Friends>()).status = status;
    let _ = app.emit(FRIENDS_STATUS_EVENT, status);
}

/// Conecta ao registro (ou reconecta, depois de um login). Sem sessão, fica "signed-out".
pub fn start<R: Runtime>(app: &AppHandle<R>) {
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
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    let (task, client) = {
        let state = app.state::<Friends>();
        let mut running = lock(&state);
        (running.task.take(), running.client.take())
    };
    if let Some(task) = task {
        task.abort();
    }
    drop(client);
    crate::dm::stop(app);
    set_status(app, Status::SignedOut);
}

async fn run<R: Runtime>(app: AppHandle<R>) {
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
                        // O sinal das conversas vai para o motor delas, não para a interface.
                        FriendsEvent::Message(value) => {
                            if !crate::dm::on_registry_message(&app, &value) {
                                let _ = app.emit(FRIENDS_EVENT, value);
                            }
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

/// Manda uma ação pelo registro (as conversas usam para o sinal). `false` sem conexão agora.
pub fn send_action<R: Runtime>(app: &AppHandle<R>, action: &FriendAction) -> bool {
    let state = app.state::<Friends>();
    let running = lock(&state);
    running.client.as_ref().map(|c| c.send(action)).is_some()
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
    use super::*;
    use crate::test_support::{fresh_discord_id, session as signed, test_app, worker};

    #[test]
    fn troca_da_sessao_tenta_de_novo_ate_dar_certo() {
        let mut tries = 0;
        let got = tauri::async_runtime::block_on(retry(&[Duration::ZERO; 5], || {
            tries += 1;
            let ok = tries == 3;
            async move { ok.then_some("sessao") }
        }));
        assert_eq!(got, Some("sessao"));
        assert_eq!(tries, 3);
    }

    #[test]
    fn troca_da_sessao_desiste_quando_acabam_as_esperas() {
        let mut tries = 0;
        let got: Option<()> = tauri::async_runtime::block_on(retry(&[Duration::ZERO; 4], || {
            tries += 1;
            async { None }
        }));
        assert_eq!(got, None);
        assert_eq!(tries, 4);
    }

    #[test]
    fn esperas_da_troca_somam_uns_cinco_minutos() {
        let total: u64 = EXCHANGE_DELAYS_S.iter().sum();
        assert!((240..=420).contains(&total), "{total} s");
    }

    #[test]
    fn status_vai_como_texto_para_a_interface() {
        assert_eq!(serde_json::to_string(&Status::SignedOut).unwrap(), r#""signed-out""#);
        assert_eq!(serde_json::to_string(&Status::Online).unwrap(), r#""online""#);
    }

    #[test]
    fn endereco_http_do_worker_vem_do_da_sinalizacao() {
        assert_eq!(http_base_of("wss://sv.workers.dev"), "https://sv.workers.dev");
        assert_eq!(http_base_of("ws://127.0.0.1:8787"), "http://127.0.0.1:8787");
        assert_eq!(http_base_of("estranho"), "estranho");
    }

    #[test]
    fn guarda_le_e_apaga_a_sessao() {
        let t = test_app();
        assert_eq!(session(t.handle()), None);
        save_session(t.handle(), Some("abc.def"));
        assert_eq!(session(t.handle()).as_deref(), Some("abc.def"));
        save_session(t.handle(), None);
        assert_eq!(session(t.handle()), None);
    }

    #[test]
    fn sem_sessao_fica_fora_e_recusa_acoes() {
        let t = test_app();
        let status = t.events(FRIENDS_STATUS_EVENT);
        start(t.handle());
        status.wait("signed-out", |v| v == "signed-out");
        assert_eq!(friends_status(t.app.state::<Friends>()), Status::SignedOut);
        let err = friends_action(t.app.state::<Friends>(), FriendAction::FriendRequest { id: "1".into() }).unwrap_err();
        assert!(err.contains("Sem conexão"), "{err}");
    }

    #[test]
    fn com_sessao_valida_conecta_e_repassa_o_registro() {
        let Some((_, secret)) = worker() else { return };
        let t = test_app();
        let status = t.events(FRIENDS_STATUS_EVENT);
        let events = t.events(FRIENDS_EVENT);
        save_session(t.handle(), Some(&signed(&secret, &fresh_discord_id())));
        start(t.handle());
        status.wait("online", |v| v == "online");
        events.wait("estado", |v| v["type"] == "state");
        friends_action(
            t.app.state::<Friends>(),
            FriendAction::Profile {
                name: "Ana".into(),
                avatar: None,
            },
        )
        .unwrap();
        events.wait("perfil novo", |v| v["type"] == "state" && v["me"]["name"] == "Ana");
        // A sessão renovada pelo registro fica salva.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while session(t.handle()).is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(session(t.handle()).is_some());
        // Sair da conta desconecta.
        stop(t.handle());
        assert_eq!(friends_status(t.app.state::<Friends>()), Status::SignedOut);
    }

    #[test]
    fn sessao_recusada_e_apagada() {
        if worker().is_none() {
            return;
        }
        let t = test_app();
        let status = t.events(FRIENDS_STATUS_EVENT);
        save_session(t.handle(), Some("sessao.falsa"));
        start(t.handle());
        // O `start` desliga antes de conectar e já avisa "signed-out": esse não é a recusa. Espera
        // tentar conectar e só então o "signed-out" de verdade (a sessão é apagada antes dele).
        status.wait("connecting", |v| v == "connecting");
        status.wait("signed-out", |v| v == "signed-out");
        assert_eq!(session(t.handle()), None, "a sessão recusada continuou salva");
    }

    #[test]
    fn worker_recusa_token_do_discord_falso() {
        if worker().is_none() {
            return;
        }
        // Como no `run()` do app: o HTTPS usa o rustls com o provedor ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::new();
        assert_eq!(tauri::async_runtime::block_on(exchange(&http, "token-falso")), None);
    }

    /// Regressão: o Worker não trocou o token na hora e o login seguia sem amigos, até a pessoa
    /// entrar com o Discord de novo. Agora fica "conectando" e segue tentando sozinho.
    #[test]
    fn troca_que_falha_no_login_continua_tentando_em_vez_de_desligar_os_amigos() {
        if worker().is_none() {
            return;
        }
        let _ = rustls::crypto::ring::default_provider().install_default();
        let t = test_app();
        let status = t.events(FRIENDS_STATUS_EVENT);
        save_session(t.handle(), Some("sessao.de.outra.conta"));
        tauri::async_runtime::block_on(activate(t.handle(), reqwest::Client::new(), "token-falso".into()));
        status.wait("connecting", |v| v == "connecting");
        assert_eq!(friends_status(t.app.state::<Friends>()), Status::Connecting);
        assert_eq!(session(t.handle()), None, "ficou a sessão de outra conta");
        // Sair da conta cancela as tentativas.
        stop(t.handle());
        assert_eq!(friends_status(t.app.state::<Friends>()), Status::SignedOut);
        assert!(lock(&t.app.state::<Friends>()).task.is_none());
    }
}
