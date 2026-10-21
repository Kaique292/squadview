//! Bandeja do Windows (perto do relógio), como no Discord: fechar a janela só esconde o app, que
//! continua na sala. Clique no ícone abre de novo; no botão direito há "Abrir" e "Sair".
//!
//! A pessoa pode desligar em Configurações › Windows (fica salvo em `janela.json`); desligado,
//! fechar a janela fecha o app, como antes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, State, Window};

const SETTINGS_FILE: &str = "janela.json";
const MENU_OPEN: &str = "abrir";
const MENU_QUIT: &str = "sair";

/// Fechar a janela manda para a bandeja (padrão: sim).
pub struct Tray {
    close_to_tray: AtomicBool,
}

impl Default for Tray {
    fn default() -> Self {
        Self {
            close_to_tray: AtomicBool::new(true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSettings {
    pub close_to_tray: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { close_to_tray: true }
    }
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    Some(app.path().app_local_data_dir().ok()?.join(SETTINGS_FILE))
}

/// Lê o que a pessoa escolheu (arquivo ausente ou estragado: o padrão).
fn load(path: Option<&Path>) -> WindowSettings {
    path.and_then(|p| std::fs::read(p).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Lê a escolha salva e põe o ícone na bandeja. Sem ícone (falhou criar), fechar a janela fecha o
/// app: esconder sem ter como abrir de novo deixaria a pessoa sem saída.
pub fn setup<R: Runtime>(app: &tauri::App<R>) {
    let settings = load(settings_path(app.handle()).as_deref());
    let created = create_icon(app).is_ok();
    app.state::<Tray>()
        .close_to_tray
        .store(settings.close_to_tray && created, Ordering::Relaxed);
}

fn create_icon<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Abrir o SquadView", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;
    let mut builder = TrayIconBuilder::with_id("principal")
        .tooltip("SquadView")
        .menu(&menu)
        // O menu é do botão direito; o esquerdo abre o app.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => crate::show_main(app, "bandeja"),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main(tray.app_handle(), "bandeja");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Pediram para fechar uma janela. A principal vai para a bandeja (se ligado); o resto fecha.
pub fn on_close_requested<R: Runtime>(window: &Window<R>, api: &tauri::CloseRequestApi) {
    if window.label() != "main" {
        return;
    }
    let to_tray = window
        .try_state::<Tray>()
        .is_some_and(|t| t.close_to_tray.load(Ordering::Relaxed));
    if to_tray {
        api.prevent_close();
        let _ = window.hide();
    }
}

/// O que está valendo (para a tela de Configurações).
#[tauri::command]
pub fn window_settings(tray: State<'_, Tray>) -> WindowSettings {
    WindowSettings {
        close_to_tray: tray.close_to_tray.load(Ordering::Relaxed),
    }
}

/// Liga ou desliga "minimizar para a bandeja ao fechar" e guarda a escolha.
#[tauri::command]
pub fn set_close_to_tray<R: Runtime>(app: AppHandle<R>, tray: State<'_, Tray>, enabled: bool) -> WindowSettings {
    tray.close_to_tray.store(enabled, Ordering::Relaxed);
    let settings = WindowSettings { close_to_tray: enabled };
    if let Some(path) = settings_path(&app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(&settings) {
            let _ = std::fs::write(path, json);
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn sem_arquivo_ou_estragado_fica_ligado() {
        assert!(load(None).close_to_tray);
        let dir = std::env::temp_dir().join(format!("sv-janela-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(SETTINGS_FILE);
        std::fs::write(&file, "não é json").unwrap();
        assert!(load(Some(&file)).close_to_tray);
        std::fs::write(&file, r#"{"closeToTray":false}"#).unwrap();
        assert!(!load(Some(&file)).close_to_tray);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn desligar_guarda_e_vale_na_hora() {
        let t = test_app();
        let state = || t.app.state::<Tray>();
        assert!(window_settings(state()).close_to_tray);
        assert!(!set_close_to_tray(t.handle().clone(), state(), false).close_to_tray);
        assert!(!window_settings(state()).close_to_tray);
        // Abrindo de novo, vale o que foi salvo.
        assert!(!load(settings_path(t.handle()).as_deref()).close_to_tray);
        set_close_to_tray(t.handle().clone(), state(), true);
        assert!(load(settings_path(t.handle()).as_deref()).close_to_tray);
    }
}
