//! Mensagens privadas (ADR 0017): liga o motor do `sv-session` à conexão de amigos (que leva o
//! sinal) e à interface (comandos e o evento `dm-event`).
//!
//! O motor nasce na primeira vez que a conversa é usada (ou quando chega o sinal de um amigo) e
//! morre ao sair da conta. Histórico em `conversas/`, anexos guardados pelo app em `anexos/`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sv_net::friends::FriendAction;
use sv_net::signaling::TurnCredentials;
use sv_session::dm::{Conversation, DmEngine, DmMessage};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

/// Evento com o que mudou nas conversas (`DmEvent` do `sv-session`).
pub const DM_EVENT: &str = "dm-event";

/// O motor de quem está logado (o id do Discord junto, para trocar ao trocar de conta).
#[derive(Default)]
pub struct Dms(Mutex<Option<(String, Arc<DmEngine>)>>);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Pasta de dados do app (`AppData\Roaming\...`).
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Onde ficam as conversas.
pub fn conversations_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    data_dir(app).ok().map(|d| d.join("conversas"))
}

/// Onde ficam os anexos guardados pelo app (cópias de quem manda e o que baixamos sem escolher pasta).
pub fn attachments_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    data_dir(app).ok().map(|d| d.join("anexos"))
}

/// Id de amigo no formato do Discord (a interface nunca manda outra coisa).
fn check_friend(friend: &str) -> Result<(), String> {
    let ok = (15..=21).contains(&friend.len()) && friend.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(()).ok_or_else(|| "Amigo inválido.".to_owned())
}

/// O motor de quem está logado, criado na primeira vez.
fn engine<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<DmEngine>, String> {
    let me = crate::discord::discord_profile(app.clone())
        .map(|p| p.id)
        .ok_or("Entre com o Discord para conversar.")?;
    let state = app.state::<Dms>();
    let mut running = lock(&state.0);
    if let Some((id, engine)) = running.as_ref()
        && *id == me
    {
        return Ok(engine.clone());
    }
    let dir = conversations_dir(app).ok_or("Sem pasta de dados.")?;
    let signal_app = app.clone();
    let event_app = app.clone();
    let engine = Arc::new(
        DmEngine::start(
            &me,
            dir,
            sv_net::stun::DEFAULT_SERVERS,
            Arc::new(move |to: &str, payload| {
                crate::friends::send_action(
                    &signal_app,
                    &FriendAction::DmSignal {
                        to: to.to_owned(),
                        payload,
                    },
                );
            }),
            Arc::new(move |event| {
                let _ = event_app.emit(DM_EVENT, event);
            }),
        )
        .map_err(|e| e.to_string())?,
    );
    *running = Some((me, engine.clone()));
    drop(running);
    // O repassador, para quem está atrás de CGNAT (chega como `turn`).
    crate::friends::send_action(app, &FriendAction::TurnRequest);
    Ok(engine)
}

/// Saiu da conta: fecha as conexões das conversas.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    let engine = lock(&app.state::<Dms>().0).take();
    drop(engine);
}

/// Mensagens do registro que são das conversas: vão para o motor, não para a interface.
/// `true` se a mensagem era daqui.
pub fn on_registry_message<R: Runtime>(app: &AppHandle<R>, value: &Value) -> bool {
    match value["type"].as_str() {
        Some("dm-signal") => {
            if let (Some(from), Ok(engine)) = (value["from"].as_str(), engine(app)) {
                engine.on_signal(from, value["payload"].clone());
            }
            true
        }
        Some("dm-unreachable") => {
            let running = lock(&app.state::<Dms>().0).as_ref().map(|(_, e)| e.clone());
            if let (Some(friend), Some(engine)) = (value["id"].as_str(), running) {
                engine.on_unreachable(friend);
            }
            true
        }
        Some("turn") => {
            let running = lock(&app.state::<Dms>().0).as_ref().map(|(_, e)| e.clone());
            if let (Ok(credentials), Some(engine)) = (serde_json::from_value::<TurnCredentials>(value.clone()), running)
            {
                engine.set_turn(credentials);
            }
            true
        }
        _ => false,
    }
}

/// As conversas, a mais recente primeiro.
#[tauri::command]
pub fn dm_conversations<R: Runtime>(app: AppHandle<R>) -> Result<Vec<Conversation>, String> {
    Ok(engine(&app)?.conversations())
}

/// As mensagens com o amigo.
#[tauri::command]
pub fn dm_messages<R: Runtime>(app: AppHandle<R>, friend: String) -> Result<Vec<DmMessage>, String> {
    check_friend(&friend)?;
    Ok(engine(&app)?.messages(&friend))
}

/// Se a conexão direta com o amigo está aberta.
#[tauri::command]
pub fn dm_online<R: Runtime>(app: AppHandle<R>, friend: String) -> Result<bool, String> {
    check_friend(&friend)?;
    Ok(engine(&app)?.online(&friend))
}

#[tauri::command]
pub fn dm_send_text<R: Runtime>(app: AppHandle<R>, friend: String, text: String) -> Result<DmMessage, String> {
    check_friend(&friend)?;
    engine(&app)?.send_text(&friend, &text).map_err(|e| e.to_string())
}

/// Manda um arquivo. `keep_copy`: o app guarda uma cópia (senão, só aponta para o original).
#[tauri::command]
pub fn dm_send_file<R: Runtime>(
    app: AppHandle<R>,
    friend: String,
    path: String,
    keep_copy: bool,
) -> Result<DmMessage, String> {
    check_friend(&friend)?;
    let copies = keep_copy
        .then(|| attachments_dir(&app).map(|d| d.join("enviados")))
        .flatten();
    engine(&app)?
        .send_file(&friend, std::path::Path::new(&path), copies.as_deref())
        .map_err(|e| e.to_string())
}

/// Baixa um anexo do amigo. Sem `dest`, vai para a pasta de anexos do app (fotos pequenas, que
/// aparecem sozinhas). Devolve onde vai ficar.
#[tauri::command]
pub fn dm_download<R: Runtime>(
    app: AppHandle<R>,
    friend: String,
    id: String,
    dest: Option<String>,
) -> Result<String, String> {
    check_friend(&friend)?;
    let engine = engine(&app)?;
    let message = engine
        .messages(&friend)
        .into_iter()
        .find(|m| m.id == id && !m.mine)
        .ok_or("Esse anexo não está nesta conversa.")?;
    let name = message.file.map(|f| f.name).ok_or("Essa mensagem não tem anexo.")?;
    let dest = match dest {
        Some(d) => PathBuf::from(d),
        None => {
            // O nome do arquivo vem do amigo: só a parte final, nunca um caminho.
            let safe = std::path::Path::new(&name)
                .file_name()
                .map_or_else(|| "anexo".into(), |n| n.to_string_lossy().into_owned());
            attachments_dir(&app)
                .ok_or("Sem pasta de dados.")?
                .join("recebidos")
                .join(format!("{id}-{safe}"))
        }
    };
    engine.download(&friend, &id, &dest);
    Ok(dest.to_string_lossy().into_owned())
}

/// Apaga o histórico de uma conversa (ou de todas, sem `friend`).
#[tauri::command]
pub fn dm_clear<R: Runtime>(app: AppHandle<R>, friend: Option<String>) -> Result<(), String> {
    if let Some(f) = &friend {
        check_friend(f)?;
    }
    clear_history(&app, friend.as_deref());
    Ok(())
}

/// Apaga o histórico (pelo motor, se ele existe, para a memória dele esquecer junto).
pub fn clear_history<R: Runtime>(app: &AppHandle<R>, friend: Option<&str>) {
    let running = lock(&app.state::<Dms>().0).as_ref().map(|(_, e)| e.clone());
    match running {
        Some(engine) => engine.clear(friend),
        None => {
            if let Some(dir) = conversations_dir(app) {
                sv_session::dm::History::new(dir).clear(friend);
            }
        }
    }
}

/// O arquivo de um anexo da conversa, se ele existe neste PC (o caminho vem do nosso histórico,
/// nunca da interface).
fn attachment_path<R: Runtime>(app: &AppHandle<R>, friend: &str, id: &str) -> Result<PathBuf, String> {
    check_friend(friend)?;
    let path = engine(app)?
        .messages(friend)
        .into_iter()
        .find(|m| m.id == id)
        .and_then(|m| m.file?.path)
        .map(PathBuf::from)
        .ok_or("Esse anexo não está neste PC.")?;
    if path.is_file() {
        Ok(path)
    } else {
        Err("O arquivo não está mais onde estava.".into())
    }
}

/// Libera só este anexo para a interface mostrar (imagem ou vídeo) e devolve o caminho. O
/// protocolo de arquivos do Tauri começa sem nenhuma pasta liberada: cada anexo é liberado aqui.
#[tauri::command]
pub fn dm_preview<R: Runtime>(app: AppHandle<R>, friend: String, id: String) -> Result<String, String> {
    let path = attachment_path(&app, &friend, &id)?;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Abre o anexo no programa padrão do Windows.
#[tauri::command]
pub fn dm_open<R: Runtime>(app: AppHandle<R>, friend: String, id: String) -> Result<(), String> {
    let path = attachment_path(&app, &friend, &id)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Mostra o anexo na pasta dele.
#[tauri::command]
pub fn dm_reveal<R: Runtime>(app: AppHandle<R>, friend: String, id: String) -> Result<(), String> {
    let path = attachment_path(&app, &friend, &id)?;
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// Escolher arquivos para mandar (vazio: cancelou).
#[tauri::command]
pub async fn dm_pick_files<R: Runtime>(app: AppHandle<R>) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Mandar arquivos")
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Onde salvar um anexo que chegou (`None`: cancelou).
#[tauri::command]
pub async fn dm_pick_save<R: Runtime>(app: AppHandle<R>, name: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Salvar anexo")
            .set_file_name(name)
            .blocking_save_file()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;

    const BETO: &str = "222222222222222222";

    fn login(t: &crate::test_support::TestApp) {
        let dir = t.app.path().app_data_dir().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("discord.json"),
            r#"{"id":"111111111111111111","username":"ana","globalName":"Ana","avatar":null}"#,
        )
        .unwrap();
    }

    #[test]
    fn sem_login_as_conversas_pedem_para_entrar() {
        let t = test_app();
        let erro = dm_conversations(t.handle().clone()).unwrap_err();
        assert!(erro.contains("Discord"));
        assert_eq!(
            dm_send_text(t.handle().clone(), "x".into(), "oi".into()).unwrap_err(),
            "Amigo inválido."
        );
        // Sem motor, os avisos do registro só são reconhecidos (nada a fazer).
        assert!(on_registry_message(
            t.handle(),
            &serde_json::json!({"type": "dm-unreachable", "id": BETO})
        ));
        assert!(on_registry_message(
            t.handle(),
            &serde_json::json!({"type": "turn", "urls": []})
        ));
        assert!(!on_registry_message(t.handle(), &serde_json::json!({"type": "state"})));
        clear_history(t.handle(), None);
    }

    #[test]
    fn conversa_guardada_e_anexos_na_pasta_do_app() {
        let t = test_app();
        login(&t);
        let h = || t.handle().clone();
        let sent = dm_send_text(h(), BETO.into(), "oi".into()).unwrap();
        assert_eq!(dm_messages(h(), BETO.into()).unwrap(), vec![sent]);
        assert_eq!(dm_conversations(h()).unwrap()[0].friend, BETO);
        assert!(!dm_online(h(), BETO.into()).unwrap());
        // Ninguém online para responder o sinal: chega o aviso do registro.
        assert!(on_registry_message(
            t.handle(),
            &serde_json::json!({"type": "dm-unreachable", "id": BETO})
        ));
        assert!(on_registry_message(
            t.handle(),
            &serde_json::json!({"type": "dm-signal", "from": BETO, "payload": {"kind": "outra"}})
        ));
        assert!(on_registry_message(
            t.handle(),
            &serde_json::json!({"type": "turn", "urls": ["turn:127.0.0.1:3478"], "username": "u", "credential": "c"})
        ));

        // Arquivo com cópia: a cópia fica em anexos/enviados.
        let source = std::env::temp_dir().join(format!("squadview-dm-foto-{}.png", std::process::id()));
        std::fs::write(&source, b"png").unwrap();
        let file = dm_send_file(h(), BETO.into(), source.to_string_lossy().into(), true).unwrap();
        let copy = PathBuf::from(file.file.unwrap().path.unwrap());
        assert!(copy.starts_with(attachments_dir(t.handle()).unwrap().join("enviados")));
        let _ = std::fs::remove_file(&source);
        assert!(dm_send_file(h(), BETO.into(), "nao-existe.png".into(), false).is_err());

        // Baixar o que não é anexo nosso de receber avisa.
        assert!(dm_download(h(), BETO.into(), "nao-existe".into(), None).is_err());
        assert!(dm_download(h(), BETO.into(), sent_id(&t), None).is_err());

        // A cópia existe: dá para liberar para a prévia; o que não existe, não.
        assert_eq!(
            dm_preview(h(), BETO.into(), copy_id(&t)).unwrap(),
            copy.to_string_lossy()
        );
        assert!(dm_preview(h(), BETO.into(), "nao-existe".into()).is_err());
        assert!(dm_open(h(), BETO.into(), "nao-existe".into()).is_err());
        assert!(dm_reveal(h(), "x".into(), "nao-existe".into()).is_err());
        std::fs::remove_file(&copy).unwrap();
        assert_eq!(
            dm_preview(h(), BETO.into(), copy_id(&t)).unwrap_err(),
            "O arquivo não está mais onde estava."
        );

        dm_clear(h(), Some(BETO.into())).unwrap();
        assert!(dm_messages(h(), BETO.into()).unwrap().is_empty());
        assert!(dm_clear(h(), Some("../x".into())).is_err());
        stop(t.handle());
        // Sem motor, apagar tudo ainda funciona pela pasta.
        dm_clear(h(), None).unwrap();
    }

    /// Id do anexo com cópia (o último arquivo mandado ao Beto).
    fn copy_id(t: &crate::test_support::TestApp) -> String {
        dm_messages(t.handle().clone(), BETO.into())
            .unwrap()
            .into_iter()
            .rfind(|m| m.file.is_some())
            .unwrap()
            .id
    }

    /// Id de uma mensagem nossa (que não é anexo de receber).
    fn sent_id(t: &crate::test_support::TestApp) -> String {
        dm_send_text(t.handle().clone(), BETO.into(), "de novo".into())
            .unwrap()
            .id
    }

    #[test]
    fn baixar_sem_destino_vai_para_a_pasta_de_anexos_com_nome_seguro() {
        let t = test_app();
        login(&t);
        let h = || t.handle().clone();
        // Um anexo do Beto que chegou (gravado como o motor grava).
        let dir = conversations_dir(t.handle()).unwrap();
        sv_session::dm::History::new(&dir).push(
            BETO,
            DmMessage {
                id: "b-1".into(),
                mine: false,
                at: 1,
                text: None,
                file: Some(sv_session::dm::Attachment {
                    name: "../../perigo.png".into(),
                    size: 3,
                    mime: "image/png".into(),
                    path: None,
                    done: 0,
                }),
                status: sv_session::dm::Status::Received,
            },
        );
        let dest = PathBuf::from(dm_download(h(), BETO.into(), "b-1".into(), None).unwrap());
        assert_eq!(
            dest,
            attachments_dir(t.handle())
                .unwrap()
                .join("recebidos")
                .join("b-1-perigo.png")
        );
        let escolhido = std::env::temp_dir().join("escolhido.png");
        assert_eq!(
            dm_download(h(), BETO.into(), "b-1".into(), Some(escolhido.to_string_lossy().into())).unwrap(),
            escolhido.to_string_lossy()
        );
    }
}
