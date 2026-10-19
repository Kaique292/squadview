//! Ponto de entrada do app desktop: liga a interface (React) ao núcleo em Rust.

/// Versão do app, exibida na interface e usada pelo auto-update.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Inicia o app Tauri.
pub fn run() {
    tauri::Builder::default()
        // Atualização automática assinada (ver docs/adr/0006-atualizacao-automatica.md).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![app_version])
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
