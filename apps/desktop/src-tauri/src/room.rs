//! Comandos da sala (Fase 2): criar/entrar, ver quem está, assistir um amigo.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;
use sv_session::{DiscordUser, JoinAs, Member, RemoteView, Room, RoomUpdate, SpeakingEffect, WindowEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::diagnostics::Diagnostics;
use crate::preview::{self, FULLSCREEN_EVENT};

/// Lista de membros (nós primeiro).
pub const MEMBERS_EVENT: &str = "room-members";
/// O picture in picture pediu para voltar ao app (`false`).
pub const PIP_EVENT: &str = "video-pip";
/// Estado da ligação com a sala: "reconnecting", "connected" (voltou) ou "lost" (saímos).
pub const ROOM_CONNECTION_EVENT: &str = "room-connection";
/// Nome da sala (texto, ou `null` sem nome) quando alguém renomeia.
pub const ROOM_NAME_EVENT: &str = "room-name";
/// Um amigo de alguém da sala quer entrar: `{id, from: {id, name, avatar}}`.
pub const JOIN_REQUEST_EVENT: &str = "room-join-request";
/// O pedido de entrada foi respondido por alguém ou venceu (o id).
pub const JOIN_REQUEST_CLOSED_EVENT: &str = "room-join-request-closed";
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
pub(crate) fn signaling_url() -> String {
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

/// Conta do Discord no formato que o Worker aceita (id numérico, hash da foto em hex). Fora
/// disso vai sem conta, em vez de o Worker recusar a entrada.
fn clean_discord(user: DiscordUser) -> Option<DiscordUser> {
    let id_ok = (15..=21).contains(&user.id.len()) && user.id.bytes().all(|b| b.is_ascii_digit());
    let avatar_ok = user.avatar.as_deref().is_none_or(|a| {
        let hash = a.strip_prefix("a_").unwrap_or(a);
        hash.len() == 32 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    });
    (id_ok && avatar_ok).then_some(user)
}

pub(crate) fn lock(state: &RoomState) -> std::sync::MutexGuard<'_, Running> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Entra na sala `code` (ou cria uma nova, se `code` vier vazio) com o nome `name`. Ao criar,
/// `room_name` dá nome à sala.
#[tauri::command]
pub async fn room_join<R: Runtime>(
    app: AppHandle<R>,
    code: Option<String>,
    name: String,
    discord: Option<DiscordUser>,
    room_name: Option<String>,
    voice: Option<crate::voice::VoicePrefs>,
    effect: Option<SpeakingEffect>,
) -> Result<RoomInfo, String> {
    let code = match code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => sv_net::signaling::normalize_room_code(c).ok_or("Código inválido: são 6 letras e números.")?,
        None => sv_net::signaling::new_room_code(),
    };
    let name = clean_name(&name).ok_or("Escolha um nome de 1 a 32 caracteres para a galera te reconhecer.")?;
    let room_name = room_name.as_deref().and_then(clean_name);
    let discord = discord.and_then(clean_discord);
    // Com a sessão, a sala sabe quem somos de verdade (recentes e pedidos de entrada).
    let session = crate::friends::session(&app);
    let url = signaling_url();
    let events_app = app.clone();
    let (join_code, join_url) = (code.clone(), url.clone());
    let room = tauri::async_runtime::spawn_blocking(move || {
        let settings = voice.as_ref().map(crate::voice::VoicePrefs::settings);
        let joined = Room::join(
            &join_url,
            &join_code,
            JoinAs { name, discord, session },
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
        // E com o efeito de fala escolhido nas configurações (ADR 0015).
        joined.set_effect(effect.as_ref().and_then(clean_effect));
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

/// Vinculou ou desvinculou o Discord já na sala: troca o nome e a foto para todos.
#[tauri::command]
pub fn room_set_profile(state: State<'_, RoomState>, name: String, discord: Option<DiscordUser>) -> Result<(), String> {
    let name = clean_name(&name).ok_or("O nome precisa ter de 1 a 32 caracteres.")?;
    let running = lock(&state);
    // Fora de uma sala não há o que avisar.
    if let Some(room) = running.room.as_ref() {
        room.set_profile(&name, discord.and_then(clean_discord));
    }
    Ok(())
}

/// Trocou o efeito de fala nas configurações (`None`: o anel padrão): a sala inteira passa a vê-lo.
#[tauri::command]
pub fn room_set_effect(state: State<'_, RoomState>, effect: Option<SpeakingEffect>) -> Result<(), String> {
    // Fora de uma sala não há o que avisar: o efeito vai junto ao entrar.
    if let Some(room) = lock(&state).room.as_ref() {
        room.set_effect(effect.as_ref().and_then(clean_effect));
    }
    Ok(())
}

/// Só os efeitos que conhecemos e com a cor no formato; o resto vira o anel padrão.
fn clean_effect(effect: &SpeakingEffect) -> Option<SpeakingEffect> {
    SpeakingEffect::cleaned(&effect.kind, &effect.color)
}

/// Entramos com o Discord já dentro da sala: a sala confere a sessão (vira conferido para os
/// outros e entra nos recentes deles), sem sair e entrar.
pub fn verify_in_room<R: Runtime>(app: &AppHandle<R>, session: &str) {
    if let Some(room) = lock(&app.state::<RoomState>()).room.as_ref() {
        room.verify(session);
    }
}

/// Aceita ou recusa o pedido de entrada `id`.
#[tauri::command]
pub fn room_answer_join(state: State<'_, RoomState>, id: String, accept: bool) -> Result<(), String> {
    let running = lock(&state);
    running
        .room
        .as_ref()
        .ok_or("Entre numa sala primeiro.")?
        .answer_join(&id, accept);
    Ok(())
}

/// Sai da sala (para de assistir e, se estava transmitindo, os amigos são desconectados).
#[tauri::command]
pub async fn room_leave<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
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
pub async fn room_stop_watching<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    preview::off_main(move || {
        let state = app.state::<RoomState>();
        let Some(watching) = lock(&state).watching.take() else {
            return;
        };
        preview::hand_off_exit(&app, watching.view.snapshot());
        drop(watching);
        preview::set_watching(&app, false);
        announce_watching(&app, None);
    })
    .await
}

/// Conta para a sala quem estamos assistindo (quem transmite vê quantos e quem assistem) e deixa a
/// sala repassar esse stream a quem não alcança quem transmite (ADR 0014).
fn announce_watching<R: Runtime>(app: &AppHandle<R>, watching: Option<sv_session::RelaySource>) {
    if let Some(room) = lock(&app.state::<RoomState>()).room.as_ref() {
        room.set_watching(watching.as_ref().map(|s| s.streamer.as_str()));
        room.set_relay_source(watching);
    }
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
pub fn tap<R: Runtime>(app: &AppHandle<R>) -> Option<std::sync::Arc<dyn sv_session::StreamTap>> {
    lock(&app.state::<RoomState>()).room.as_ref().map(Room::tap)
}

/// A prévia começou ou parou: avisa a sala (aparece "AO VIVO" para os amigos).
pub fn stream_changed<R: Runtime>(app: &AppHandle<R>, keyframes: Option<sv_session::KeyframeHandle>) {
    let state = app.state::<RoomState>();
    let running = lock(&state);
    if let Some(room) = running.room.as_ref() {
        match keyframes {
            Some(k) => room.stream_started(k),
            None => room.stream_stopped(),
        }
    }
}

/// Solta o vídeo de quem estamos assistindo numa janela pequena, sempre no topo (picture in
/// picture), ou devolve ao app.
#[tauri::command]
pub fn room_set_pip(state: State<'_, RoomState>, pip: bool) -> Result<(), String> {
    let running = lock(&state);
    match running.watching.as_ref() {
        Some(w) => {
            w.view.set_video_pip(pip);
            Ok(())
        }
        // Voltar sem estar assistindo não tem o que fazer (o vídeo já fechou).
        None if !pip => Ok(()),
        None => Err("Assista a alguém primeiro.".to_owned()),
    }
}

/// Repassa a posição do vídeo para quem estamos assistindo.
pub fn apply_layout<R: Runtime>(app: &AppHandle<R>, layout: &preview::Layout) -> bool {
    let state = app.state::<RoomState>();
    let running = lock(&state);
    match running.watching.as_ref() {
        Some(w) => {
            w.view.set_video_bounds(layout.bounds);
            w.view.set_video_hidden(layout.hidden);
            w.view.set_video_holes(layout.holes.clone());
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

fn on_update<R: Runtime>(app: &AppHandle<R>, update: RoomUpdate) {
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
        RoomUpdate::WatchReady {
            member,
            conn,
            desc,
            path,
        } => start_watching(app, member, conn, desc, path),
        // O repassador TURN da sala (ADR 0014): só para o registro, a interface não muda.
        RoomUpdate::Relay(result) => {
            let detail = match &result {
                Ok(addr) => json!({ "ready": true, "addr": addr.to_string() }),
                Err(reason) => json!({ "ready": false, "reason": reason }),
            };
            app.state::<Diagnostics>().write("relay", &detail);
        }
        RoomUpdate::WatchFailed { member_id, reason } => {
            app.state::<Diagnostics>()
                .write("watch-failed", &json!({ "reason": reason }));
            let _ = app.emit(WATCH_ENDED_EVENT, WatchEnded { member_id, reason });
        }
        RoomUpdate::JoinRequest { id, from } => {
            let _ = app.emit(JOIN_REQUEST_EVENT, json!({ "id": id, "from": from }));
        }
        RoomUpdate::JoinRequestClosed(id) => {
            let _ = app.emit(JOIN_REQUEST_CLOSED_EVENT, id);
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

fn start_watching<R: Runtime>(
    app: &AppHandle<R>,
    member: Member,
    conn: quinn::Connection,
    desc: sv_net::protocol::StreamDescription,
    path: sv_session::WatchPath,
) {
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
                    announce_watching(&app, None);
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
        if preview::emit_pointer(&event_app, event) {
            return;
        }
        let kind = match event {
            WindowEvent::ToggleFullscreen => "toggle",
            WindowEvent::ExitFullscreen => "exit",
            // Dois cliques, Esc ou fechar no picture in picture: a interface traz o vídeo de volta.
            WindowEvent::ExitPip => {
                let _ = event_app.emit(PIP_EVENT, false);
                return;
            }
            _ => return,
        };
        let _ = event_app.emit(FULLSCREEN_EVENT, kind);
    });
    let layout = preview::layout(app);
    view.set_video_bounds(layout.bounds);
    view.set_video_hidden(layout.hidden);
    view.set_video_holes(layout.holes);

    app.state::<Diagnostics>().write(
        "watch-start",
        &json!({ "member": member.name, "desc": format!("{desc:?}"), "path": path.label() }),
    );
    let relay = view.relay_source(&member.id);
    let state = app.state::<RoomState>();
    lock(&state).watching = Some(Watching {
        member: member.clone(),
        view,
    });
    preview::set_watching(app, true);
    announce_watching(app, Some(relay));
    let _ = app.emit(WATCH_STARTED_EVENT, member);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestApp, test_app, worker};

    fn join(t: &TestApp, code: Option<&str>, name: &str) -> Result<RoomInfo, String> {
        tauri::async_runtime::block_on(room_join(
            t.handle().clone(),
            code.map(str::to_owned),
            name.into(),
            None,
            Some("Galera".into()),
            None,
            None,
        ))
    }

    fn members_named(v: &serde_json::Value) -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn conta_do_discord_no_formato_do_worker() {
        let ok = DiscordUser {
            id: "123456789012345678".into(),
            avatar: Some(format!("a_{}", "f".repeat(32))),
        };
        assert_eq!(clean_discord(ok.clone()), Some(ok));
        let sem_foto = DiscordUser {
            id: "123456789012345678".into(),
            avatar: None,
        };
        assert!(clean_discord(sem_foto).is_some());
        for (id, avatar) in [
            ("123", None),
            ("12345678901234567x", None),
            ("123456789012345678", Some("https://exemplo.com/foto.png")),
            ("123456789012345678", Some("ABCDEF")),
        ] {
            let user = DiscordUser {
                id: id.into(),
                avatar: avatar.map(str::to_owned),
            };
            assert_eq!(clean_discord(user), None, "{id} {avatar:?}");
        }
    }

    #[test]
    fn sem_sala_os_comandos_pedem_para_entrar_antes() {
        let t = test_app();
        let state = || t.app.state::<RoomState>();
        let entre = "Entre numa sala primeiro.";
        assert_eq!(room_rename(state(), "Squad".into()).unwrap_err(), entre);
        assert_eq!(room_set_name(state(), "Ana".into()).unwrap_err(), entre);
        assert_eq!(room_answer_join(state(), "aa".into(), true).unwrap_err(), entre);
        assert_eq!(room_watch(state(), "aa".into()).unwrap_err(), entre);
        // Picture in picture sem estar assistindo: pedir avisa; voltar não faz nada.
        assert_eq!(room_set_pip(state(), true).unwrap_err(), "Assista a alguém primeiro.");
        assert!(room_set_pip(state(), false).is_ok());
        // Sem sala e sem ninguém assistido, não acontece nada (nem cai).
        assert!(room_set_profile(state(), "Ana".into(), None).is_ok());
        assert!(room_set_effect(state(), None).is_ok());
        set_watch_listening(state(), false);
        set_watch_volume(state(), 1.5);
        assert!(tap(t.handle()).is_none());
        stream_changed(t.handle(), None);
        assert!(!apply_layout(t.handle(), &preview::Layout::default()));
        tauri::async_runtime::block_on(room_stop_watching(t.handle().clone())).unwrap();
        tauri::async_runtime::block_on(room_leave(t.handle().clone())).unwrap();
    }

    #[test]
    fn nomes_e_codigos_invalidos_sao_recusados_antes_de_conectar() {
        let t = test_app();
        assert!(join(&t, Some("AB"), "Ana").unwrap_err().contains("Código inválido"));
        assert!(join(&t, None, "   ").unwrap_err().contains("1 a 32"));
        let state = || t.app.state::<RoomState>();
        assert!(room_rename(state(), " ".into()).unwrap_err().contains("1 a 32"));
        assert!(room_set_name(state(), "".into()).unwrap_err().contains("1 a 32"));
        assert!(
            room_set_profile(state(), "".into(), None)
                .unwrap_err()
                .contains("1 a 32")
        );
    }

    #[test]
    fn dois_amigos_na_sala_pelos_comandos_do_app() {
        if worker().is_none() {
            return;
        }
        let ana = test_app();
        let beto = test_app();
        let ana_members = ana.events(MEMBERS_EVENT);
        let beto_members = beto.events(MEMBERS_EVENT);
        let beto_names = beto.events(ROOM_NAME_EVENT);
        let beto_watch = beto.events(WATCH_ENDED_EVENT);

        // Ana cria (código sorteado), Beto entra pelo código (com hífen e minúsculas).
        let info = join(&ana, None, "Ana").unwrap();
        assert_eq!(info.room_name.as_deref(), Some("Galera"));
        assert!(info.url.starts_with("ws"));
        let typed = format!("{}-{}", &info.code[..3], &info.code[3..]).to_lowercase();
        let joined = join(&beto, Some(&typed), "Beto").unwrap();
        assert_eq!(joined.code, info.code);
        assert_eq!(joined.room_name.as_deref(), Some("Galera"));
        let list = ana_members.wait("Beto entrou", |v| members_named(v) == ["Ana", "Beto"]);
        // A lista vem com a gente primeiro.
        let ana_id = list[0]["id"].as_str().unwrap().to_owned();
        assert!(tap(ana.handle()).is_some());

        // Renomear a sala, trocar o apelido e vincular o Discord chegam ao outro.
        room_rename(ana.app.state::<RoomState>(), "Squad".into()).unwrap();
        beto_names.wait("sala renomeada", |v| v == "Squad");
        room_set_name(ana.app.state::<RoomState>(), "Aninha".into()).unwrap();
        beto_members.wait("apelido", |v| members_named(v).contains(&"Aninha".to_owned()));
        let conta = DiscordUser {
            id: "123456789012345678".into(),
            avatar: None,
        };
        room_set_profile(ana.app.state::<RoomState>(), "Ana".into(), Some(conta)).unwrap();
        beto_members.wait("foto do Discord", |v| {
            v.as_array()
                .is_some_and(|a| a.iter().any(|m| m["discord"]["id"] == "123456789012345678"))
        });
        // O efeito de fala escolhido chega ao outro; um inválido vira o anel padrão.
        let ondas = SpeakingEffect {
            kind: "waves".into(),
            color: "#FF8800".into(),
        };
        room_set_effect(ana.app.state::<RoomState>(), Some(ondas)).unwrap();
        beto_members.wait("efeito de fala", |v| {
            v.as_array().is_some_and(|a| {
                a.iter()
                    .any(|m| m["name"] == "Ana" && m["effect"] == json!({"kind": "waves", "color": "#ff8800"}))
            })
        });
        let invalido = SpeakingEffect {
            kind: "fogo".into(),
            color: "#ff8800".into(),
        };
        room_set_effect(ana.app.state::<RoomState>(), Some(invalido)).unwrap();
        beto_members.wait("efeito padrão", |v| {
            v.as_array()
                .is_some_and(|a| a.iter().any(|m| m["name"] == "Ana" && m["effect"].is_null()))
        });
        // Responder um pedido que não existe não faz nada.
        room_answer_join(ana.app.state::<RoomState>(), "0000000000000000".into(), true).unwrap();

        // Pedir para assistir quem não está ao vivo termina com o motivo.
        room_watch(beto.app.state::<RoomState>(), ana_id).unwrap();
        let ended = beto_watch.wait("assistir recusado", |v| v["reason"].is_string());
        assert!(!ended["reason"].as_str().unwrap().is_empty());

        // Ana sai: o Beto fica sozinho na lista.
        tauri::async_runtime::block_on(room_leave(ana.handle().clone())).unwrap();
        beto_members.wait("Ana saiu", |v| members_named(v) == ["Beto"]);
        tauri::async_runtime::block_on(room_leave(beto.handle().clone())).unwrap();
    }

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
