//! Registro local de desempenho (ver `sv_session::SessionLog`): nada sai do PC.
//!
//! O uso de GPU e CPU é gravado a cada segundo desde a abertura do app, então o registro
//! traz o "antes" (só o jogo) e o "durante" (jogo + transmissão) para comparar.
//!
//! A pessoa liga ou desliga em Configurações (fica salvo em `diagnostics.json`) e pode copiar os
//! arquivos para colar numa conversa do Discord e mandar para quem vai analisar.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sv_session::{SessionLog, SystemMonitor};
use tauri::{App, AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;

/// Evento com o uso de GPU e CPU, a cada segundo.
pub const SYSTEM_EVENT: &str = "system-load";
/// Quantos registros vão juntos ao copiar (a sessão atual e as anteriores).
const COPY_COUNT: usize = 3;

/// Registrado no `Builder` (antes de qualquer janela ou plugin), vazio, e configurado no `setup`.
/// Assim nenhum caminho que chegue cedo (como o aviso de "abriram o app de novo" durante uma
/// atualização) encontra o estado faltando: no pior caso, só não anota.
#[derive(Default)]
pub struct Diagnostics {
    log: Mutex<Option<SessionLog>>,
    /// Pasta dos registros e arquivo da preferência (definidos no `setup`).
    paths: OnceLock<Paths>,
}

#[derive(Default)]
struct Paths {
    /// Pasta dos registros (`None` se não deu para descobrir).
    dir: Option<PathBuf>,
    /// Arquivo com a preferência ligado/desligado.
    settings: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Settings {
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsStatus {
    enabled: bool,
    /// Quantos registros existem na pasta.
    files: usize,
}

impl Diagnostics {
    /// Configura as pastas e abre o registro, se estiver ligado. Só a primeira chamada vale.
    fn init(&self, dir: Option<PathBuf>, settings: Option<PathBuf>) {
        let enabled = load_enabled(settings.as_deref());
        if self.paths.set(Paths { dir, settings }).is_ok() && enabled {
            *self.lock() = open_log(self.dir());
        }
    }

    fn dir(&self) -> Option<&Path> {
        self.paths.get().and_then(|p| p.dir.as_deref())
    }

    fn settings(&self) -> Option<&Path> {
        self.paths.get().and_then(|p| p.settings.as_deref())
    }

    pub fn write(&self, kind: &str, data: &impl Serialize) {
        if let Some(log) = self.lock().as_mut() {
            log.write(kind, data);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<SessionLog>> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn path(&self) -> Option<String> {
        self.lock().as_ref().map(|l| l.path().display().to_string())
    }

    fn enabled(&self) -> bool {
        self.lock().is_some()
    }

    /// Abre um registro novo (com a linha "app") ou fecha o atual.
    fn set_enabled(&self, enabled: bool) {
        let mut log = self.lock();
        if enabled == log.is_some() {
            return;
        }
        *log = if enabled { open_log(self.dir()) } else { None };
    }

    fn recent(&self) -> Vec<PathBuf> {
        self.dir()
            .map(|d| sv_session::recent_logs(d, COPY_COUNT))
            .unwrap_or_default()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStarted {
    version: &'static str,
    gpu: String,
    os: String,
}

fn open_log(dir: Option<&Path>) -> Option<SessionLog> {
    let mut log = SessionLog::create(dir?).ok()?;
    log.write(
        "app",
        &AppStarted {
            version: env!("CARGO_PKG_VERSION"),
            gpu: sv_gpu::Gpu::new()
                .map(|g| g.adapter_name().to_owned())
                .unwrap_or_else(|e| format!("erro: {e}")),
            os: os_version(),
        },
    );
    Some(log)
}

/// Lê a preferência salva; sem arquivo, o registro vem ligado (ele nunca sai do PC).
fn load_enabled(settings: Option<&Path>) -> bool {
    settings
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|data| serde_json::from_slice::<Settings>(&data).ok())
        .is_none_or(|s| s.enabled)
}

/// Abre o registro (se ligado) e começa a medir o sistema. Falhas aqui nunca impedem o app de abrir.
pub fn setup<R: Runtime>(app: &App<R>) {
    let base = app.path().app_local_data_dir().ok();
    let dir = base.as_ref().map(|d| d.join("logs"));
    let settings = base.as_ref().map(|d| d.join("diagnostics.json"));
    // O estado já existe (registrado no `Builder`); se faltar por engano, registra aqui.
    if app.try_state::<Diagnostics>().is_none() {
        app.manage(Diagnostics::default());
    }
    app.state::<Diagnostics>().init(dir, settings);
    sample_system(app.handle().clone());
}

/// Caminho do registro desta sessão, para a interface mostrar.
#[tauri::command]
pub fn session_log_path<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    app.state::<Diagnostics>().path()
}

/// Se o registro está ligado e quantos arquivos existem.
#[tauri::command]
pub fn diagnostics_status<R: Runtime>(app: AppHandle<R>) -> DiagnosticsStatus {
    let diagnostics = app.state::<Diagnostics>();
    DiagnosticsStatus {
        enabled: diagnostics.enabled(),
        files: diagnostics.recent().len(),
    }
}

/// Liga ou desliga o registro e guarda a escolha.
#[tauri::command]
pub fn set_diagnostics_enabled<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<DiagnosticsStatus, String> {
    let diagnostics = app.state::<Diagnostics>();
    if let Some(path) = diagnostics.settings() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec(&Settings { enabled }).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
    }
    diagnostics.set_enabled(enabled);
    Ok(diagnostics_status(app.clone()))
}

/// Copia os registros mais recentes para a área de transferência, como arquivos: é só colar
/// (Ctrl+V) numa conversa do Discord. Devolve quantos foram copiados.
#[tauri::command]
pub fn copy_diagnostics<R: Runtime>(app: AppHandle<R>) -> Result<usize, String> {
    let files = app.state::<Diagnostics>().recent();
    if files.is_empty() {
        return Err("Ainda não há registros. Ligue o registro e use o app um pouco.".into());
    }
    clipboard::copy_files(&files)?;
    Ok(files.len())
}

/// Abre a pasta dos registros no Explorer, com o mais recente selecionado.
#[tauri::command]
pub fn reveal_diagnostics<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let latest = app.state::<Diagnostics>().recent().into_iter().next();
    let Some(latest) = latest else {
        return Err("Ainda não há registros.".into());
    };
    app.opener().reveal_item_in_dir(&latest).map_err(|e| e.to_string())
}

/// Área de transferência do Windows com arquivos (o mesmo formato do "Copiar" do Explorer).
mod clipboard {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DROPFILES;

    /// Lista de arquivos no formato do `CF_HDROP`: caminhos UTF-16 separados por `\0` e
    /// terminados por `\0\0`.
    pub(super) fn file_list(files: &[PathBuf]) -> Vec<u16> {
        let mut list = Vec::new();
        for file in files {
            list.extend(file.as_os_str().encode_wide());
            list.push(0);
        }
        list.push(0);
        list
    }

    pub fn copy_files(files: &[PathBuf]) -> Result<(), String> {
        let list = file_list(files);
        let header = size_of::<DROPFILES>();
        let size = header + list.len() * 2;
        // SAFETY: o bloco tem `size` bytes: o cabeçalho DROPFILES e, logo depois, a lista. Ele só
        // é escrito entre GlobalLock e GlobalUnlock. Se o Windows aceitar o SetClipboardData, a
        // memória passa a ser dele; se não, liberamos aqui.
        unsafe {
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, size).map_err(|e| e.to_string())?;
            let ptr = GlobalLock(mem).cast::<u8>();
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err("Sem memória para copiar.".into());
            }
            ptr.cast::<DROPFILES>().write_unaligned(DROPFILES {
                pFiles: header as u32,
                fWide: true.into(),
                ..Default::default()
            });
            std::ptr::copy_nonoverlapping(list.as_ptr().cast::<u8>(), ptr.add(header), list.len() * 2);
            let _ = GlobalUnlock(mem);

            if let Err(e) = OpenClipboard(None) {
                let _ = GlobalFree(Some(mem));
                return Err(format!("Não deu para abrir a área de transferência: {e}"));
            }
            let result = EmptyClipboard().and_then(|()| SetClipboardData(u32::from(CF_HDROP.0), Some(HANDLE(mem.0))));
            let _ = CloseClipboard();
            if let Err(e) = result {
                let _ = GlobalFree(Some(mem));
                return Err(format!("Não deu para copiar: {e}"));
            }
        }
        Ok(())
    }
}

fn sample_system<R: Runtime>(app: AppHandle<R>) {
    std::thread::Builder::new()
        .name("sv-system-monitor".into())
        .spawn(move || {
            let Some(mut monitor) = SystemMonitor::new() else {
                return;
            };
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let load = monitor.sample();
                app.state::<Diagnostics>().write("system", &load);
                let _ = app.emit(SYSTEM_EVENT, load);
            }
        })
        .expect("não foi possível criar a thread do monitor do sistema");
}

fn os_version() -> String {
    use std::os::windows::process::CommandExt;
    /// Não abre janela de console (o app de release não tem console).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // O `ver` do Windows traz o build exato ("Microsoft Windows [versão 10.0.26300.0]").
    std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .and_then(|o| windows_build(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Tira só o número da saída do `ver` (o texto em volta vem na página de código do console e
/// muda com o idioma do Windows).
fn windows_build(ver: &str) -> Option<String> {
    ver.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|part| part.matches('.').count() >= 2)
        .map(|v| format!("Windows {v}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn antes_do_setup_nao_ha_registro_e_nada_quebra() {
        let t = test_app();
        assert_eq!(session_log_path(t.handle().clone()), None);
        let status = diagnostics_status(t.handle().clone());
        assert!(!status.enabled);
        assert_eq!(status.files, 0);
        assert!(
            copy_diagnostics(t.handle().clone())
                .unwrap_err()
                .contains("Ainda não há registros")
        );
        assert!(reveal_diagnostics(t.handle().clone()).is_err());
        // Sem pastas configuradas, ligar só muda o estado em memória (sem arquivo).
        assert!(!set_diagnostics_enabled(t.handle().clone(), true).unwrap().enabled);
    }

    #[test]
    fn liga_desliga_e_guarda_a_escolha() {
        let t = test_app();
        setup(&t.app);
        // Sem preferência salva, vem ligado e já há um registro desta sessão.
        let status = diagnostics_status(t.handle().clone());
        assert!(status.enabled);
        assert_eq!(status.files, 1);
        let path = session_log_path(t.handle().clone()).expect("registro aberto");
        t.app
            .state::<Diagnostics>()
            .write("teste", &serde_json::json!({ "ok": true }));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"app\""), "{content}");
        assert!(content.contains("\"teste\""), "{content}");

        let off = set_diagnostics_enabled(t.handle().clone(), false).unwrap();
        assert!(!off.enabled);
        assert_eq!(session_log_path(t.handle().clone()), None);
        let settings = t.app.path().app_local_data_dir().unwrap().join("diagnostics.json");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), r#"{"enabled":false}"#);
        assert!(!load_enabled(Some(&settings)));

        // Ligar de novo abre outro registro; desligado não escreve nada.
        assert!(set_diagnostics_enabled(t.handle().clone(), true).unwrap().enabled);
        assert!(session_log_path(t.handle().clone()).is_some());
        // Ligar quando já está ligado não abre outro.
        let before = session_log_path(t.handle().clone());
        assert!(set_diagnostics_enabled(t.handle().clone(), true).unwrap().enabled);
        assert_eq!(session_log_path(t.handle().clone()), before);
    }

    #[test]
    fn setup_com_registro_desligado_nao_abre_arquivo() {
        let t = test_app();
        let base = t.app.path().app_local_data_dir().unwrap();
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("diagnostics.json"), r#"{"enabled":false}"#).unwrap();
        setup(&t.app);
        assert!(!diagnostics_status(t.handle().clone()).enabled);
        assert_eq!(session_log_path(t.handle().clone()), None);
    }

    #[test]
    fn versao_do_windows_sai_do_ver() {
        assert!(os_version().starts_with("Windows 10.0."), "{}", os_version());
    }

    #[test]
    fn le_o_build_do_windows_em_qualquer_idioma() {
        assert_eq!(
            super::windows_build("\r\nMicrosoft Windows [vers\u{fffd}o 10.0.26300.9711]\r\n").as_deref(),
            Some("Windows 10.0.26300.9711")
        );
        assert_eq!(
            super::windows_build("Microsoft Windows [Version 10.0.22631.4317]").as_deref(),
            Some("Windows 10.0.22631.4317")
        );
        assert_eq!(super::windows_build("sem versão"), None);
    }

    #[test]
    fn lista_de_arquivos_no_formato_do_windows() {
        let list = super::clipboard::file_list(&[r"C:\a.jsonl".into(), r"C:\b.jsonl".into()]);
        let text = String::from_utf16(&list).unwrap();
        assert_eq!(text, "C:\\a.jsonl\0C:\\b.jsonl\0\0");
    }

    #[test]
    fn registro_vem_ligado_sem_preferencia_salva() {
        let dir = std::env::temp_dir().join(format!("sv-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("diagnostics.json");
        assert!(super::load_enabled(None));
        assert!(super::load_enabled(Some(&file)));
        std::fs::write(&file, r#"{"enabled":false}"#).unwrap();
        assert!(!super::load_enabled(Some(&file)));
        std::fs::write(&file, "lixo").unwrap();
        assert!(super::load_enabled(Some(&file)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
