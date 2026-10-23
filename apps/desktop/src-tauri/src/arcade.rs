//! Comandos do fliperama (ADR 0023 e 0024) para a interface: o driver de controle (instalar e
//! remover), abrir e fechar como host, entrar e sair como jogador, e o teclado e o mouse do jogador.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use sv_capture::CaptureSource;
use sv_input::{DriverStatus, KeyMap, MouseButton};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_opener::OpenerExt;

use crate::diagnostics::Diagnostics;
use crate::room::{RoomState, lock};

/// Jogadores do fliperama que abrimos (`Vec<ArcadeSeatInfo>`).
pub const PLAYERS_EVENT: &str = "arcade-players";
/// Ganhamos a vaga num fliperama: `{ hostId, slot }`.
pub const SEAT_EVENT: &str = "arcade-seat";
/// Saímos de um fliperama: `{ hostId, reason }` (`reason` vazio: saímos nós).
pub const LEFT_EVENT: &str = "arcade-left";
/// Quem hospeda ligou ou desligou nosso controle total: `{ hostId, granted }`.
pub const KBM_EVENT: &str = "arcade-kbm";

/// Situação do driver de controle, para Configurações > Fliperama.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfo {
    /// `installed`, `missing`, `incompatible` ou `unavailable`.
    status: &'static str,
    detail: Option<String>,
    /// Versão registrada no Windows (se achamos o desinstalador).
    version: Option<String>,
    /// Dá para remover pelo app (o desinstalador está registrado).
    removable: bool,
}

fn driver_info() -> DriverInfo {
    let uninstaller = sv_input::find_uninstaller();
    let (status, detail) = match sv_input::driver_status() {
        DriverStatus::Installed => ("installed", None),
        DriverStatus::Missing => ("missing", None),
        DriverStatus::Incompatible(d) => ("incompatible", Some(d)),
        DriverStatus::Unavailable(d) => ("unavailable", Some(d)),
    };
    DriverInfo {
        status,
        detail,
        version: uninstaller.as_ref().and_then(|u| u.version.clone()),
        removable: uninstaller.is_some(),
    }
}

#[tauri::command]
pub async fn arcade_driver() -> DriverInfo {
    tauri::async_runtime::spawn_blocking(driver_info)
        .await
        .unwrap_or(DriverInfo {
            status: "unavailable",
            detail: None,
            version: None,
            removable: false,
        })
}

/// Abre o instalador oficial do driver no navegador (o app não embute o driver, ADR 0023).
#[tauri::command]
pub fn arcade_install_driver<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    app.opener()
        .open_url(sv_input::driver::INSTALLER_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Remove o driver pelo desinstalador registrado no Windows (pede administrador) e devolve como
/// ficou. Com um fliperama aberto, pede para fechar antes.
#[tauri::command]
pub async fn arcade_uninstall_driver<R: Runtime>(app: AppHandle<R>) -> Result<DriverInfo, String> {
    if lock(&app.state::<RoomState>())
        .room
        .as_ref()
        .is_some_and(sv_session::Room::arcade_is_open)
    {
        return Err("Feche o fliperama antes de remover o driver.".into());
    }
    let uninstaller = sv_input::find_uninstaller().ok_or("O desinstalador do driver não foi encontrado.")?;
    app.state::<Diagnostics>().write(
        "arcade-driver",
        &json!({ "action": "uninstall", "version": uninstaller.version }),
    );
    tauri::async_runtime::spawn_blocking(move || run_uninstaller(&uninstaller.command))
        .await
        .map_err(|e| e.to_string())??;
    Ok(arcade_driver().await)
}

/// Roda o comando registrado (`MsiExec.exe /X{...}` no instalador oficial) e espera ele terminar.
fn run_uninstaller(command: &str) -> Result<(), String> {
    let status = match msi_product(command) {
        // O MSI pede o administrador sozinho e mostra o progresso.
        Some(product) => std::process::Command::new("msiexec").args(["/x", &product]).status(),
        None => std::process::Command::new("cmd").args(["/C", command]).status(),
    }
    .map_err(|e| format!("Não deu para abrir o desinstalador: {e}"))?;
    // 1602: a pessoa cancelou. Qualquer outro código não-zero é falha.
    match status.code() {
        Some(0 | 3010) => Ok(()),
        Some(1602) => Err("A remoção foi cancelada.".into()),
        _ => Err("O desinstalador não terminou a remoção.".into()),
    }
}

/// O produto MSI (`{GUID}`) de um comando `MsiExec.exe /X{GUID}` (ou `/I{GUID}`).
fn msi_product(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    if !lower.trim_start().starts_with("msiexec") {
        return None;
    }
    let start = command.find('{')?;
    let end = command[start..].find('}')? + start;
    Some(command[start..=end].to_owned())
}

/// Abre o fliperama sobre a nossa transmissão: até `slots` jogadores (P2 a P4).
#[tauri::command]
pub fn arcade_open<R: Runtime>(app: AppHandle<R>, slots: u8, game: Option<String>) -> Result<(), String> {
    let capture = crate::preview::current_capture(&app);
    let window = match capture.as_ref().map(|(source, _)| *source) {
        Some(CaptureSource::Window(hwnd)) => Some(hwnd),
        _ => None,
    };
    let game = game.or_else(|| capture.map(|(_, name)| name));
    let state = app.state::<RoomState>();
    let running = lock(&state);
    let room = running.room.as_ref().ok_or("Entre numa sala primeiro.")?;
    room.arcade_open(game, slots, window)?;
    app.state::<Diagnostics>()
        .write("arcade-open", &json!({ "slots": slots, "window": window.is_some() }));
    Ok(())
}

#[tauri::command]
pub fn arcade_close(state: State<'_, RoomState>) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_close();
    }
}

#[tauri::command]
pub fn arcade_kick(state: State<'_, RoomState>, member_id: String) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_kick(&member_id);
    }
}

/// Dá o controle total de teclado e mouse a `member_id` (ou corta, com `null`), ADR 0024.
#[tauri::command]
pub fn arcade_grant_kbm<R: Runtime>(app: AppHandle<R>, member_id: Option<String>) -> Result<(), String> {
    let state = app.state::<RoomState>();
    let running = lock(&state);
    let room = running.room.as_ref().ok_or("Entre numa sala primeiro.")?;
    room.arcade_grant_kbm(member_id.as_deref())?;
    app.state::<Diagnostics>()
        .write("arcade-kbm", &json!({ "granted": member_id.is_some() }));
    Ok(())
}

/// Pede uma vaga no fliperama de `host_id` (a interface também pede para assistir a live dele).
#[tauri::command]
pub fn arcade_join<R: Runtime>(app: AppHandle<R>, host_id: String) -> Result<(), String> {
    let keymap = load_keymap(&app);
    let state = app.state::<RoomState>();
    let running = lock(&state);
    let room = running.room.as_ref().ok_or("Entre numa sala primeiro.")?;
    room.arcade_join(&host_id, keymap);
    Ok(())
}

#[tauri::command]
pub fn arcade_leave(state: State<'_, RoomState>) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_leave();
    }
}

/// Tecla do jogador (`KeyboardEvent.code`) com a janela do fliperama em foco.
#[tauri::command]
pub fn arcade_key(state: State<'_, RoomState>, code: String, down: bool) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_key(&code, down);
    }
}

/// Movimento do mouse (ponteiro preso no vídeo), somado desde o último quadro da página.
#[tauri::command]
pub fn arcade_mouse(state: State<'_, RoomState>, dx: f32, dy: f32) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_mouse_move(dx, dy);
    }
}

/// Botão do mouse: `left`, `right` ou `middle`.
#[tauri::command]
pub fn arcade_mouse_button(state: State<'_, RoomState>, button: String, down: bool) {
    let button = match button.as_str() {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        _ => return,
    };
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_mouse_button(button, down);
    }
}

#[tauri::command]
pub fn arcade_wheel(state: State<'_, RoomState>, delta: i16) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_wheel(delta);
    }
}

/// A janela do fliperama perdeu o foco (ou soltou o ponteiro): solta tudo.
#[tauri::command]
pub fn arcade_release(state: State<'_, RoomState>) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.arcade_release();
    }
}

fn keymap_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("arcade.json"))
}

fn load_keymap<R: Runtime>(app: &AppHandle<R>) -> KeyMap {
    keymap_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// O mapeamento do teclado do jogador (salvo em `arcade.json`, ou o padrão).
#[tauri::command]
pub fn arcade_keymap<R: Runtime>(app: AppHandle<R>) -> KeyMap {
    load_keymap(&app)
}

/// Salva o mapeamento do teclado (vale também para o fliperama em que estamos jogando).
#[tauri::command]
pub fn arcade_set_keymap<R: Runtime>(app: AppHandle<R>, keymap: KeyMap) -> Result<(), String> {
    let path = keymap_path(&app).ok_or("Sem pasta de dados do app.")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&keymap).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    if let Some(room) = lock(&app.state::<RoomState>()).room.as_ref() {
        room.arcade_set_keymap(keymap);
    }
    Ok(())
}

/// Volta o mapeamento do teclado ao padrão.
#[tauri::command]
pub fn arcade_reset_keymap<R: Runtime>(app: AppHandle<R>) -> Result<KeyMap, String> {
    let keymap = KeyMap::default();
    arcade_set_keymap(app, keymap.clone())?;
    Ok(keymap)
}

/// Os avisos do fliperama vindos da sala viram eventos para a interface.
pub(crate) fn emit<R: Runtime>(app: &AppHandle<R>, update: sv_session::RoomUpdate) {
    use sv_session::RoomUpdate as U;
    match update {
        U::ArcadePlayers(seats) => {
            let _ = app.emit(PLAYERS_EVENT, seats);
        }
        U::ArcadeSeat { host, slot } => {
            app.state::<Diagnostics>()
                .write("arcade-seat", &json!({ "slot": slot }));
            let _ = app.emit(SEAT_EVENT, json!({ "hostId": host, "slot": slot }));
        }
        U::ArcadeLeft { host, reason } => {
            app.state::<Diagnostics>()
                .write("arcade-left", &json!({ "reason": reason }));
            let _ = app.emit(LEFT_EVENT, json!({ "hostId": host, "reason": reason }));
        }
        U::ArcadeKbm { host, granted } => {
            let _ = app.emit(KBM_EVENT, json!({ "hostId": host, "granted": granted }));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acha_o_produto_msi_do_desinstalador() {
        assert_eq!(
            msi_product("MsiExec.exe /X{966606F3-2745-49E9-BF15-5C3EAA4E9077}").as_deref(),
            Some("{966606F3-2745-49E9-BF15-5C3EAA4E9077}")
        );
        assert_eq!(msi_product("msiexec /I{ABC}").as_deref(), Some("{ABC}"));
        assert_eq!(msi_product(r#""C:\Program Files\x\uninstall.exe""#), None);
        assert_eq!(msi_product("MsiExec.exe /X"), None);
    }

    #[test]
    fn situacao_do_driver_vira_texto_para_a_interface() {
        let info = driver_info();
        assert!(["installed", "missing", "incompatible", "unavailable"].contains(&info.status));
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("removable").is_some());
    }
}
