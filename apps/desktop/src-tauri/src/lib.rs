//! Ponto de entrada do app desktop: liga a interface (React) ao núcleo em Rust.

mod crash;
mod diagnostics;
mod discord;
mod dm;
mod friends;
mod invite;
mod presence;
mod preview;
mod room;
mod storage;
#[cfg(test)]
mod test_support;
mod tray;
mod voice;

use std::sync::atomic::{AtomicBool, Ordering};

/// Versão do app, exibida na interface e usada pelo auto-update.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A interface terminou de abrir: fecha a splash e mostra a janela principal.
#[tauri::command]
fn app_ready(app: tauri::AppHandle) {
    show_main(&app, "pronta");
}

/// Mostra a janela principal (mesmo escondida ou minimizada) e fecha a splash. `reason` vai para
/// o registro: ajuda a entender quando a interface não avisou que estava pronta.
fn show_main<R: tauri::Runtime>(app: &tauri::AppHandle<R>, reason: &str) {
    use tauri::Manager;
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    // A rede de segurança só vale para a abertura: se a janela já apareceu e a pessoa mandou para
    // a bandeja, ela fica lá.
    if reason == "rede-de-seguranca" && (main.is_visible().unwrap_or(false) || SHOWN.load(Ordering::Relaxed)) {
        return;
    }
    SHOWN.store(true, Ordering::Relaxed);
    let _ = main.show();
    let _ = main.unminimize();
    let _ = main.set_focus();
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
    // `try_state`: o aviso de "abriram de novo" pode chegar antes do `setup` registrar o
    // Diagnostics (na atualização o app abre duas vezes quase juntas). Sem registro, só não anota.
    if let Some(diagnostics) = app.try_state::<diagnostics::Diagnostics>() {
        diagnostics.write("window-shown", &serde_json::json!({ "reason": reason }));
    }
}

/// A janela principal já apareceu alguma vez.
static SHOWN: AtomicBool = AtomicBool::new(false);

/// Se a interface travar antes de avisar, a janela principal aparece mesmo assim.
const SPLASH_FAILSAFE: std::time::Duration = std::time::Duration::from_secs(8);

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

/// Todo estado que os comandos e os eventos usam, registrado no `Builder`: existe antes de
/// qualquer janela, plugin ou `setup`. Um estado registrado só no `setup` derruba o app quando
/// algo chega cedo (foi o que aconteceu na atualização para a 0.3.0). Os testes conferem que todo
/// `state::<T>()` do código tem o seu `T` aqui.
fn manage_states<R: tauri::Runtime>(builder: tauri::Builder<R>, presence: presence::Presence) -> tauri::Builder<R> {
    builder
        .manage(diagnostics::Diagnostics::default())
        .manage(preview::PreviewState::default())
        .manage(std::sync::Arc::new(preview::TapSlot::default()))
        .manage(room::RoomState::default())
        .manage(invite::PendingInvite::default())
        .manage(friends::Friends::default())
        .manage(dm::Dms::default())
        .manage(tray::Tray::default())
        .manage(presence)
}

/// Inicia o app Tauri.
pub fn run() {
    // Falha fatal: vai para o crash.log e aparece numa caixa de mensagem (sem console, o app
    // fecharia em silêncio, só com a ampulheta no cursor).
    crash::install();
    // Provedor de criptografia do rustls para o HTTPS (login do Discord). Ignora se já houver um.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = tauri::Builder::default();
    // No app instalado, abrir de novo (ou clicar num convite) traz a janela que já está aberta.
    // No `yarn dev` ficam liberadas duas instâncias, para testar transmitir e assistir no mesmo PC.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        // Mostra (e não só foca): se a janela ficou escondida por uma abertura travada, abrir
        // o app de novo precisa trazê-la de volta.
        show_main(app, "aberto-de-novo");
    }));

    manage_states(builder, presence::Presence::start())
        // A splash abre escondida e aparece assim que a página dela carrega (sem depender do
        // JavaScript dela, que pode falhar).
        .on_page_load(|webview, payload| {
            if webview.label() == "splash" && payload.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = webview.window().show();
            }
        })
        .plugin(tauri_plugin_deep_link::init())
        // Atualização automática assinada (ver docs/adr/0006-atualizacao-automatica.md).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Fechar a janela principal manda para a bandeja (se ligado em Configurações › Windows).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                tray::on_close_requested(window, api);
            }
        })
        .setup(|app| {
            diagnostics::setup(app);
            tray::setup(app);
            setup_deep_links(app);
            // Amigos e presença: conecta sozinho se já entrou com o Discord (ADR 0011).
            friends::start(app.handle());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(SPLASH_FAILSAFE);
                show_main(&handle, "rede-de-seguranca");
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
            dm::dm_conversations,
            dm::dm_messages,
            dm::dm_online,
            dm::dm_cancel_download,
            dm::dm_retry,
            dm::dm_send_text,
            dm::dm_send_file,
            dm::dm_download,
            dm::dm_clear,
            dm::dm_preview,
            dm::dm_open,
            dm::dm_reveal,
            dm::dm_pick_files,
            dm::dm_pick_save,
            friends::friends_action,
            friends::friends_status,
            invite::take_pending_invite,
            presence::set_presence,
            preview::list_sources,
            preview::source_thumbnail,
            preview::start_preview,
            preview::set_preview_listening,
            preview::set_video_bounds,
            preview::set_video_hidden,
            preview::set_video_holes,
            preview::set_fullscreen,
            room::room_join,
            room::room_leave,
            room::room_rename,
            room::room_set_name,
            room::room_set_profile,
            room::room_set_effect,
            room::room_answer_join,
            room::room_watch,
            room::room_stop_watching,
            room::room_set_pip,
            room::set_watch_listening,
            room::set_watch_volume,
            storage::storage_usage,
            storage::storage_clear,
            voice::voice_devices,
            voice::voice_set_muted,
            voice::voice_set_deafened,
            voice::voice_set_settings,
            voice::voice_set_peer_volume,
            voice::voice_set_peer_muted,
            preview::stop_preview,
            tray::window_settings,
            tray::set_close_to_tray,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| crash::fatal(&format!("O SquadView não conseguiu abrir: {e}")));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn versao_segue_o_workspace() {
        assert_eq!(super::app_version(), env!("CARGO_PKG_VERSION"));
    }

    /// App de mentira com uma janela "main", como o de verdade, sem rodar o `setup`.
    fn app_with_main(builder: tauri::Builder<tauri::test::MockRuntime>) -> tauri::App<tauri::test::MockRuntime> {
        let app = builder.build(mock_context(noop_assets())).expect("app de teste");
        tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("janela main");
        app
    }

    /// O crash da 0.3.0: durante a atualização o app abre duas vezes, e o aviso de "abriram de
    /// novo" chega à primeira instância antes do `setup` dela. Não pode derrubar o app.
    #[test]
    fn segunda_instancia_antes_do_setup_nao_derruba_o_app() {
        let app = app_with_main(mock_builder());
        // Sem nenhum estado registrado (o pior caso): só mostra a janela.
        super::show_main(app.handle(), "aberto-de-novo");
        // Com os estados do app de verdade, também.
        let app = app_with_main(super::manage_states(mock_builder(), super::presence::Presence::start()));
        super::show_main(app.handle(), "aberto-de-novo");
        super::show_main(app.handle(), "rede-de-seguranca");
    }

    #[test]
    fn estados_existem_antes_do_setup() {
        let app = app_with_main(super::manage_states(mock_builder(), super::presence::Presence::start()));
        assert!(app.try_state::<super::diagnostics::Diagnostics>().is_some());
        assert!(app.try_state::<super::preview::PreviewState>().is_some());
        assert!(app.try_state::<std::sync::Arc<super::preview::TapSlot>>().is_some());
        assert!(app.try_state::<super::room::RoomState>().is_some());
        assert!(app.try_state::<super::invite::PendingInvite>().is_some());
        assert!(app.try_state::<super::friends::Friends>().is_some());
        assert!(app.try_state::<super::presence::Presence>().is_some());
        // O registro de desempenho vazio (antes do `setup`) aceita escrita sem fazer nada.
        app.state::<super::diagnostics::Diagnostics>()
            .write("teste", &serde_json::json!({}));
    }

    /// Nomes dos tipos em `state::<T>()` / `try_state::<T>()` de um arquivo (o último segmento,
    /// sem `Arc<...>`), para conferir que todos estão em `manage_states`.
    fn state_types(source: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for (i, _) in source.match_indices("state::<") {
            let rest = &source[i + "state::<".len()..];
            let mut depth = 1;
            let end = rest
                .char_indices()
                .find(|&(_, c)| {
                    match c {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })
                .map(|(j, _)| j)
                .unwrap_or(rest.len());
            let ty = rest[..end].trim();
            let inner = ty
                .strip_prefix("std::sync::Arc<")
                .or_else(|| ty.strip_prefix("Arc<"))
                .unwrap_or(ty);
            let name = inner.trim_end_matches('>').rsplit("::").next().unwrap_or(inner).trim();
            found.insert(name.to_owned());
        }
        found
    }

    #[test]
    fn todo_estado_usado_no_codigo_e_registrado_no_builder() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let lib = std::fs::read_to_string(dir.join("lib.rs")).unwrap();
        let start = lib.find("fn manage_states").unwrap();
        let body = &lib[start..start + lib[start..].find("\n}").unwrap()];
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            // Os testes citam `state::<` de propósito, e os comentários não são código.
            let code: String = source
                .split("#[cfg(test)]")
                .next()
                .unwrap_or(&source)
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join(
                    "
",
                );
            for name in state_types(&code) {
                let registered = body.contains(&format!("{name}::default()"))
                    || body.contains(&format!("{name}::start()"))
                    || (name == "Presence" && body.contains(".manage(presence)"));
                if !registered {
                    missing.push(format!("{} usa {name}", path.file_name().unwrap().to_string_lossy()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "estados usados sem registro em manage_states: {missing:?}"
        );
    }

    #[test]
    fn acha_os_tipos_de_estado_no_codigo() {
        let found = state_types(
            "app.state::<Diagnostics>(); app.try_state::<crate::room::RoomState>(); \
             app.state::<std::sync::Arc<preview::TapSlot>>()",
        );
        assert_eq!(
            found,
            ["Diagnostics", "RoomState", "TapSlot"]
                .map(String::from)
                .into_iter()
                .collect()
        );
    }
}
