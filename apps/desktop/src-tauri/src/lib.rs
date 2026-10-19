//! Ponto de entrada do app desktop: liga a interface (React) ao núcleo em Rust.

mod diagnostics;
mod discord;
mod invite;
mod presence;
mod preview;
mod room;

/// Versão do app, exibida na interface e usada pelo auto-update.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A interface terminou de abrir: fecha a splash e mostra a janela principal.
#[tauri::command]
fn app_ready(app: tauri::AppHandle) {
    show_main(&app);
}

fn show_main(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
}

/// Se a interface travar antes de avisar, a janela principal aparece mesmo assim.
const SPLASH_FAILSAFE: std::time::Duration = std::time::Duration::from_secs(15);

/// Convites `squadview://`: o link que abriu o app e os que chegarem com ele aberto.
fn setup_deep_links(app: &tauri::App) {
    use tauri_plugin_deep_link::DeepLinkExt;
    // No `yarn dev` o instalador não registrou o esquema; registra para o executável de teste.
    #[cfg(debug_assertions)]
    let _ = app.deep_link().register_all();
    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        let urls: Vec<String> = event.urls().iter().map(|u| u.to_string()).collect();
        invite::handle_urls(&handle, urls.iter().map(String::as_str));
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        let urls: Vec<String> = urls.iter().map(|u| u.to_string()).collect();
        invite::handle_urls(app.handle(), urls.iter().map(String::as_str));
    }
}

/// Inicia o app Tauri.
pub fn run() {
    // Provedor de criptografia do rustls para o HTTPS (login do Discord). Ignora se já houver um.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = tauri::Builder::default();
    // No app instalado, abrir de novo (ou clicar num convite) traz a janela que já está aberta.
    // No `yarn dev` ficam liberadas duas instâncias, para testar transmitir e assistir no mesmo PC.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        use tauri::Manager;
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_deep_link::init())
        // Atualização automática assinada (ver docs/adr/0006-atualizacao-automatica.md).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .manage(preview::PreviewState::default())
        .manage(std::sync::Arc::new(preview::TapSlot::default()))
        .manage(room::RoomState::default())
        .manage(invite::PendingInvite::default())
        .manage(presence::Presence::start())
        .setup(|app| {
            diagnostics::setup(app);
            setup_deep_links(app);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(SPLASH_FAILSAFE);
                show_main(&handle);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            app_ready,
            diagnostics::session_log_path,
            diagnostics::diagnostics_status,
            diagnostics::set_diagnostics_enabled,
            diagnostics::copy_diagnostics,
            diagnostics::reveal_diagnostics,
            discord::discord_login,
            discord::discord_profile,
            discord::discord_logout,
            invite::take_pending_invite,
            presence::set_presence,
            preview::list_sources,
            preview::source_thumbnail,
            preview::start_preview,
            preview::set_preview_listening,
            preview::set_video_bounds,
            preview::set_video_hidden,
            preview::set_fullscreen,
            room::room_join,
            room::room_leave,
            room::room_watch,
            room::room_stop_watching,
            room::set_watch_listening,
            preview::stop_preview,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o SquadView");
}

#[cfg(test)]
mod tests {
    #[test]
    fn versao_segue_o_workspace() {
        assert_eq!(super::app_version(), env!("CARGO_PKG_VERSION"));
    }
}
