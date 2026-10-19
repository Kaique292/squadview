//! Comandos da sala (Fase 2): criar/entrar, ver quem está, assistir um amigo.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;
use sv_session::{Member, RemoteView, Room, RoomUpdate, WindowEvent};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::diagnostics::Diagnostics;
use crate::preview::{self, FULLSCREEN_EVENT};

/// Lista de membros (nós primeiro).
pub const MEMBERS_EVENT: &str = "room-members";
/// Estado da ligação com a sala: "reconnecting", "connected" (voltou) ou "lost" (saímos).
pub const ROOM_CONNECTION_EVENT: &str = "room-connection";
/// Nome da sala (texto, ou `null` sem nome) quando alguém renomeia.
pub const ROOM_NAME_EVENT: &str = "room-name";
/// Erro ou queda da sala (texto para o usuário).
pub const ROOM_ERROR_EVENT: &str = "room-error";
/// Começamos a assistir alguém.
pub const WATCH_STARTED_EVENT: &str = "watch-started";
/// Não deu para assistir, ou o stream acabou (texto para o usuário).
pub const WATCH_ENDED_EVENT: &str = "watch-ended";
/// Estatísticas de quem estamos assistindo, a cada 500 ms.
pub const WATCH_STATS_EVENT: &str = "watch-stats";

/// Endereço do serviço de salas. Ordem: variável de ambiente (testes), o configurado no build
/// da release e, por fim, o Worker local (`yarn workspace @squadview/signaling dev`).
fn signaling_url() -> String {
    std::env::var("SQUADVIEW_SIGNALING_URL")
        .ok()
        .or_else(|| option_env!("SQUADVIEW_SIGNALING_URL").map(str::to_owned))
        .unwrap_or_else(|| "ws://127.0.0.1:8787".to_owned())
}

#[derive(Default)]
pub struct RoomState(Mutex<Running>);

#[derive(Default)]
pub(crate) struct Running {
    pub(crate) room: Option<Room>,
    watching: Option<Watching>,
}

struct Watching {
    member: Member,
    view: RemoteView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    pub code: String,
    pub url: String,
    /// Nome da sala (`None`: sem nome, a interface mostra o código).
    pub room_name: Option<String>,
}

/// Nome de exibição (pessoa ou sala), como o Worker aceita: sem espaços nas pontas, sem
/// caracteres de controle, 1 a 32 caracteres.
fn clean_name(value: &str) -> Option<String> {
    let name = value.trim();
    let len = name.chars().count();
    ((1..=32).contains(&len) && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

pub(crate) fn lock(state: &RoomState) -> std::sync::MutexGuard<'_, Running> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Entra na sala `code` (ou cria uma nova, se `code` vier vazio) com o nome `name`. Ao criar,
/// `room_name` dá nome à sala.
#[tauri::command]
pub async fn room_join(
    app: AppHandle,
    code: Option<String>,
    name: String,
    room_name: Option<String>,
    voice: Option<crate::voice::VoicePrefs>,
) -> Result<RoomInfo, String> {
    let code = match code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => sv_net::signaling::normalize_room_code(c).ok_or("Código inválido: são 6 letras e números.")?,
        None => sv_net::signaling::new_room_code(),
    };
    let name = clean_name(&name).ok_or("Escolha um nome de 1 a 32 caracteres para a galera te reconhecer.")?;
    let room_name = room_name.as_deref().and_then(clean_name);
    let url = signaling_url();
    let events_app = app.clone();
    let (join_code, join_url) = (code.clone(), url.clone());
    let room = tauri::async_runtime::spawn_blocking(move || {
        let settings = voice.as_ref().map(crate::voice::VoicePrefs::settings);
        let joined = Room::join(
            &join_url,
            &join_code,
            &name,
            room_name.as_deref(),
            settings,
            move |update| on_update(&events_app, update),
        )?;
        // Entra com o mudo e o surdo que estavam ligados (como no Discord).
        if let Some(v) = &voice {
            if v.muted {
                joined.set_muted(true);
            }
            if v.deafened {
                joined.set_deafened(true);
            }
        }
        Ok::<_, String>(joined)
    })
    .await
    .map_err(|e| e.to_string())??;
    // Tipo de NAT no registro (sem o IP): ajuda a entender quando alguém não consegue assistir.
    let nat = room.nat();
    let room_name = room.room_name();

    // Se já estávamos transmitindo, os amigos da sala nova passam a poder assistir.
    let state = app.state::<RoomState>();
    let (old_watching, old_room) = {
        let mut running = lock(&state);
        (running.watching.take(), running.room.replace(room))
    };
    let restart_app = app.clone();
    preview::off_main(move || {
        drop(old_watching);
        drop(old_room);
        preview::set_watching(&restart_app, false);
        preview::restart_for_room(&restart_app);
    })
    .await?;
    app.state::<Diagnostics>()
        .write("room-join", &json!({ "code": code, "nat": nat }));
    Ok(RoomInfo { code, url, room_name })
}

/// Renomeia a sala para todos.
#[tauri::command]
pub fn room_rename(state: State<'_, RoomState>, name: String) -> Result<(), String> {
    let name = clean_name(&name).ok_or("O nome da sala precisa ter de 1 a 32 caracteres.")?;
    let running = lock(&state);
    running
        .room
        .as_ref()
        .ok_or("Entre numa sala primeiro.")?
        .rename_room(&name);
    Ok(())
}

/// Troca o nosso apelido na sala.
#[tauri::command]
pub fn room_set_name(state: State<'_, RoomState>, name: String) -> Result<(), String> {
    let name = clean_name(&name).ok_or("O apelido precisa ter de 1 a 32 caracteres.")?;
    let running = lock(&state);
    running
        .room
        .as_ref()
        .ok_or("Entre numa sala primeiro.")?
        .set_name(&name);
    Ok(())
}

/// Sai da sala (para de assistir e, se estava transmitindo, os amigos são desconectados).
#[tauri::command]
pub async fn room_leave(app: AppHandle) -> Result<(), String> {
    preview::off_main(move || {
        let state = app.state::<RoomState>();
        let (watching, room) = {
            let mut running = lock(&state);
            (running.watching.take(), running.room.take())
        };
        drop(watching);
        drop(room);
        preview::set_watching(&app, false);
        preview::restart_for_room(&app);
    })
    .await
}

/// Pede para assistir `member_id`. A resposta chega pelos eventos `watch-started` ou `watch-ended`.
#[tauri::command]
pub fn room_watch(state: State<'_, RoomState>, member_id: String) -> Result<(), String> {
    let running = lock(&state);
    let room = running.room.as_ref().ok_or("Entre numa sala primeiro.")?;
    room.watch(&member_id);
    Ok(())
}

/// Para de assistir. Antes de fechar o vídeo, manda a foto do último quadro (evento `video-exit`).
#[tauri::command]
pub async fn room_stop_watching(app: AppHandle) -> Result<(), String> {
    preview::off_main(move || {
        let state = app.state::<RoomState>();
        let Some(watching) = lock(&state).watching.take() else {
            return;
        };
        preview::hand_off_exit(&app, watching.view.snapshot());
        drop(watching);
        preview::set_watching(&app, false);
    })
    .await
}

/// Liga ou desliga o som de quem estamos assistindo.
#[tauri::command]
pub fn set_watch_listening(state: State<'_, RoomState>, listening: bool) {
    if let Some(w) = lock(&state).watching.as_ref() {
        w.view.set_listening(listening);
    }
}

/// Volume de quem estamos assistindo, de 0 a 2 (200%).
#[tauri::command]
pub fn set_watch_volume(state: State<'_, RoomState>, volume: f32) {
    if let Some(w) = lock(&state).watching.as_ref() {
        w.view.set_volume(volume);
    }
}

/// O que a prévia entrega para a sala (se estivermos numa).
pub fn tap(app: &AppHandle) -> Option<std::sync::Arc<dyn sv_session::StreamTap>> {
    lock(&app.state::<RoomState>()).room.as_ref().map(Room::tap)
}

/// A prévia começou ou parou: avisa a sala (aparece "AO VIVO" para os amigos).
pub fn stream_changed(app: &AppHandle, keyframes: Option<sv_session::KeyframeHandle>) {
    let state = app.state::<RoomState>();
    let running = lock(&state);
    if let Some(room) = running.room.as_ref() {
        match keyframes {
            Some(k) => room.stream_started(k),
            None => room.stream_stopped(),
        }
    }
}

/// Repassa a posição do vídeo para quem estamos assistindo.
pub fn apply_layout(app: &AppHandle, bounds: sv_session::Bounds, hidden: bool) -> bool {
    let state = app.state::<RoomState>();
    let running = lock(&state);
    match running.watching.as_ref() {
        Some(w) => {
            w.view.set_video_bounds(bounds);
            w.view.set_video_hidden(hidden);
            true
        }
        None => false,
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WatchEnded {
    member_id: String,
    reason: String,
}

fn on_update(app: &AppHandle, update: RoomUpdate) {
    match update {
        RoomUpdate::Members(members) => {
            let _ = app.emit(MEMBERS_EVENT, members);
        }
        RoomUpdate::RoomName(name) => {
            let _ = app.emit(ROOM_NAME_EVENT, name);
        }
        RoomUpdate::Voice(event) => crate::voice::emit_event(app, event),
        RoomUpdate::VoicePeers(peers) => {
            let _ = app.emit(crate::voice::PEERS_EVENT, peers);
        }
        RoomUpdate::VoiceFailed { member_id, reason } => {
            app.state::<Diagnostics>()
                .write("voice-failed", &json!({ "reason": reason }));
            let _ = app.emit(
                crate::voice::FAILED_EVENT,
                crate::voice::VoiceFailed { member_id, reason },
            );
        }
        RoomUpdate::WatchReady { member, conn, desc } => start_watching(app, member, conn, desc),
        RoomUpdate::WatchFailed { member_id, reason } => {
            app.state::<Diagnostics>()
                .write("watch-failed", &json!({ "reason": reason }));
            let _ = app.emit(WATCH_ENDED_EVENT, WatchEnded { member_id, reason });
        }
        RoomUpdate::Error(message) => {
            let _ = app.emit(ROOM_ERROR_EVENT, message);
        }
        RoomUpdate::Reconnecting => {
            app.state::<Diagnostics>()
                .write("room-connection", &json!({ "state": "reconnecting" }));
            let _ = app.emit(ROOM_CONNECTION_EVENT, "reconnecting");
        }
        RoomUpdate::Reconnected => {
            app.state::<Diagnostics>()
                .write("room-connection", &json!({ "state": "connected" }));
            let _ = app.emit(ROOM_CONNECTION_EVENT, "connected");
        }
        RoomUpdate::Disconnected => {
            app.state::<Diagnostics>()
                .write("room-connection", &json!({ "state": "lost" }));
            // Não deu para voltar: sai da sala de verdade (fora desta thread de rede), para a
            // interface não mostrar uma sala em que a gente já não está.
            let app = app.clone();
            std::thread::spawn(move || {
                let state = app.state::<RoomState>();
                let (watching, room) = {
                    let mut running = lock(&state);
                    (running.watching.take(), running.room.take())
                };
                drop(watching);
                drop(room);
                preview::set_watching(&app, false);
                preview::restart_for_room(&app);
                let _ = app.emit(ROOM_CONNECTION_EVENT, "lost");
            });
        }
    }
}

fn start_watching(app: &AppHandle, member: Member, conn: quinn::Connection, desc: sv_net::protocol::StreamDescription) {
    let target = match preview::video_target(app) {
        Ok(t) => t,
        Err(e) => {
            let _ = app.emit(
                WATCH_ENDED_EVENT,
                WatchEnded {
                    member_id: member.id,
                    reason: e,
                },
            );
            return;
        }
    };
    let stats_app = app.clone();
    let closed_app = app.clone();
    let closed_id = member.id.clone();
    let view = RemoteView::start(
        conn,
        desc,
        &member.name,
        target,
        move |stats| {
            stats_app.state::<Diagnostics>().write("watch", &stats);
            let _ = stats_app.emit(WATCH_STATS_EVENT, stats);
        },
        move |_reason| {
            // Quem transmitia parou, ou a rede caiu. Tira da tela (fora desta thread de rede).
            let app = closed_app.clone();
            let id = closed_id.clone();
            std::thread::spawn(move || {
                let state = app.state::<RoomState>();
                let mut running = lock(&state);
                if running.watching.as_ref().is_some_and(|w| w.member.id == id) {
                    let ended = running.watching.take();
                    drop(running);
                    if let Some(w) = ended.as_ref() {
                        preview::hand_off_exit(&app, w.view.snapshot());
                    }
                    drop(ended);
                    preview::set_watching(&app, false);
                    let _ = app.emit(
                        WATCH_ENDED_EVENT,
                        WatchEnded {
                            member_id: id,
                            reason: "A transmissão terminou.".into(),
                        },
                    );
                }
            });
        },
    );
    let event_app = app.clone();
    view.on_video_event(move |event| {
        let kind = match event {
            WindowEvent::ToggleFullscreen => "toggle",
            WindowEvent::ExitFullscreen => "exit",
        };
        let _ = event_app.emit(FULLSCREEN_EVENT, kind);
    });
    let layout = preview::layout(app);
    view.set_video_bounds(layout.bounds);
    view.set_video_hidden(layout.hidden);

    app.state::<Diagnostics>().write(
        "watch-start",
        &json!({ "member": member.name, "desc": format!("{desc:?}") }),
    );
    let state = app.state::<RoomState>();
    lock(&state).watching = Some(Watching {
        member: member.clone(),
        view,
    });
    preview::set_watching(app, true);
    let _ = app.emit(WATCH_STARTED_EVENT, member);
}

#[cfg(test)]
mod tests {
    use super::clean_name;

    #[test]
    fn nome_segue_as_regras_do_worker() {
        assert_eq!(clean_name("  Galera do LoL  ").as_deref(), Some("Galera do LoL"));
        assert_eq!(clean_name("João 🎮").as_deref(), Some("João 🎮"));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(&"a".repeat(33)), None);
        assert!(clean_name(&"a".repeat(32)).is_some());
        assert_eq!(
            clean_name(
                "Ana
Beto"
            ),
            None
        );
    }
}
