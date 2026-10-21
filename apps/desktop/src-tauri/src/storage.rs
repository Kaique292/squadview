//! Armazenamento do app em Configurações: quanto cada parte ocupa no PC e o botão de limpar.
//!
//! Só entra aqui o que pode ser apagado sem perder nada importante: o app baixa ou refaz tudo
//! sozinho. A sessão do Discord, as preferências e o login nunca são apagados por aqui.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::diagnostics::Diagnostics;

/// Partes que dá para limpar, na ordem em que aparecem na tela.
pub const PARTS: [&str; 2] = ["web", "logs"];

/// Pastas do cache da interface (WebView2) que contam como "imagens e arquivos baixados".
const WEB_CACHE_DIRS: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoragePart {
    /// `web` (fotos do Discord e o que a interface baixou) ou `logs` (registros de desempenho).
    id: &'static str,
    bytes: u64,
}

/// Quanto ocupa uma pasta, somando tudo dentro dela (o que não dá para ler conta como zero).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Apaga os registros da pasta, menos o da sessão atual (ainda sendo escrito). Devolve quantos.
pub fn clear_logs(dir: &Path, keep: Option<&Path>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && Some(p.as_path()) != keep)
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

/// Pasta de dados do WebView2 (o Tauri usa `EBWebView` dentro da pasta local do app).
fn web_profile_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    Some(app.path().app_local_data_dir().ok()?.join("EBWebView").join("Default"))
}

fn usage<R: Runtime>(app: &AppHandle<R>) -> Vec<StoragePart> {
    let web = web_profile_dir(app)
        .map(|d| WEB_CACHE_DIRS.iter().map(|c| dir_size(&d.join(c))).sum())
        .unwrap_or(0);
    let diagnostics = app.state::<Diagnostics>();
    // O registro da sessão atual não sai na limpeza, então também não conta.
    let logs = diagnostics
        .dir()
        .map(|d| logs_size(d, diagnostics.current().as_deref()))
        .unwrap_or(0);
    vec![
        StoragePart { id: "web", bytes: web },
        StoragePart {
            id: "logs",
            bytes: logs,
        },
    ]
}

/// Quanto ocupam os registros da pasta, menos o da sessão atual.
fn logs_size(dir: &Path, keep: Option<&Path>) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| Some(e.path().as_path()) != keep)
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Quanto cada parte ocupa agora.
#[tauri::command]
pub fn storage_usage<R: Runtime>(app: AppHandle<R>) -> Vec<StoragePart> {
    usage(&app)
}

/// Limpa uma parte (`web`, `logs`) ou todas (`all`) e devolve como ficou.
#[tauri::command]
pub async fn storage_clear(app: AppHandle, part: String) -> Result<Vec<StoragePart>, String> {
    clear(&app, &part, || clear_web_cache(&app)).await
}

/// A limpeza em si; o cache da interface vem de fora porque só existe com o WebView2 de verdade.
async fn clear<R: Runtime, F: std::future::Future<Output = Result<(), String>>>(
    app: &AppHandle<R>,
    part: &str,
    web: impl FnOnce() -> F,
) -> Result<Vec<StoragePart>, String> {
    let all = part == "all";
    if !all && !PARTS.contains(&part) {
        return Err(format!("Parte desconhecida: {part}"));
    }
    if all || part == "logs" {
        let diagnostics = app.state::<Diagnostics>();
        if let Some(dir) = diagnostics.dir() {
            clear_logs(dir, diagnostics.current().as_deref());
        }
    }
    if all || part == "web" {
        web().await?;
    }
    Ok(usage(app))
}

/// Pede ao WebView2 para limpar o cache em disco. Com o app aberto os arquivos estão em uso, então
/// apagar a pasta não funciona: quem limpa é o próprio WebView2 (o resto do perfil, como as
/// preferências, fica intacto).
#[cfg(windows)]
async fn clear_web_cache(app: &AppHandle) -> Result<(), String> {
    use webview2_com::ClearBrowsingDataCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE, COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, ICoreWebView2_13,
        ICoreWebView2Profile2,
    };
    use windows::core::Interface;

    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let tx = std::sync::Mutex::new(Some(tx));
    window
        .with_webview(move |webview| {
            let finish = move |result: Result<(), String>| {
                if let Some(tx) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = tx.send(result);
                }
            };
            // SAFETY: chamadas COM na thread da interface, onde `with_webview` roda.
            let started = unsafe {
                (|| -> windows::core::Result<()> {
                    let core = webview.controller().CoreWebView2()?;
                    let profile: ICoreWebView2Profile2 = core.cast::<ICoreWebView2_13>()?.Profile()?.cast()?;
                    let kinds =
                        COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE;
                    let finish = std::sync::Mutex::new(Some(finish));
                    let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
                        if let Some(finish) = finish.lock().unwrap_or_else(|e| e.into_inner()).take() {
                            finish(result.map_err(|e| e.to_string()));
                        }
                        Ok(())
                    }));
                    profile.ClearBrowsingData(kinds, &handler)
                })()
            };
            if let Err(e) = started {
                // O `finish` foi para dentro do handler; sem handler, ninguém responde e o `rx` cai.
                eprintln!("limpar o cache da interface: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    match rx.await {
        Ok(result) => result,
        Err(_) => Err("O WebView2 não conseguiu limpar o cache.".to_owned()),
    }
}

#[cfg(not(windows))]
async fn clear_web_cache(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("squadview-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn soma_o_tamanho_das_pastas_dentro_de_pastas() {
        let dir = temp_dir("tamanho");
        std::fs::write(dir.join("a.bin"), [0u8; 100]).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("b.bin"), [0u8; 50]).unwrap();
        assert_eq!(dir_size(&dir), 150);
        assert_eq!(dir_size(&dir.join("nao-existe")), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limpar_registros_mantem_o_da_sessao_atual() {
        let dir = temp_dir("registros");
        for name in ["sessao-1.jsonl", "sessao-2.jsonl", "sessao-3.jsonl"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        let current = dir.join("sessao-3.jsonl");
        assert_eq!(clear_logs(&dir, Some(&current)), 2);
        assert!(current.exists());
        assert!(!dir.join("sessao-1.jsonl").exists());
        assert_eq!(clear_logs(&dir.join("nao-existe"), None), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mede_e_limpa_cada_parte_ou_tudo() {
        let t = test_app();
        let parts = storage_usage(t.handle().clone());
        assert_eq!(parts.iter().map(|p| p.id).collect::<Vec<_>>(), PARTS);
        let web_limpo = std::cell::Cell::new(0);
        let web = || {
            web_limpo.set(web_limpo.get() + 1);
            async { Ok(()) }
        };
        fn block<F: std::future::Future>(f: F) -> F::Output {
            tauri::async_runtime::block_on(f)
        }
        assert_eq!(block(clear(t.handle(), "logs", web)).unwrap().len(), PARTS.len());
        assert_eq!(web_limpo.get(), 0);
        block(clear(t.handle(), "web", web)).unwrap();
        block(clear(t.handle(), "all", web)).unwrap();
        assert_eq!(web_limpo.get(), 2);
        // Nada fora da lista: a limpeza nunca apaga a sessão, o login ou as preferências.
        assert!(block(clear(t.handle(), "sessao", web)).is_err());
        // O cache da interface falhando chega como erro para a tela avisar.
        let falha = || async { Err::<(), _>("ocupado".to_owned()) };
        assert_eq!(block(clear(t.handle(), "web", falha)).unwrap_err(), "ocupado");
    }
}
