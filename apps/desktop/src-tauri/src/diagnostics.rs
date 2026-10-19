//! Registro local de desempenho (ver `sv_session::SessionLog`): nada sai do PC.
//!
//! O uso de GPU e CPU é gravado a cada segundo desde a abertura do app, então o registro
//! traz o "antes" (só o jogo) e o "durante" (jogo + transmissão) para comparar.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sv_session::{SessionLog, SystemMonitor};
use tauri::{App, AppHandle, Emitter, Manager};

/// Evento com o uso de GPU e CPU, a cada segundo.
pub const SYSTEM_EVENT: &str = "system-load";

#[derive(Default)]
pub struct Diagnostics(Mutex<Option<SessionLog>>);

impl Diagnostics {
    pub fn write(&self, kind: &str, data: &impl Serialize) {
        if let Some(log) = self.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            log.write(kind, data);
        }
    }

    fn path(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| l.path().display().to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStarted {
    version: &'static str,
    gpu: String,
    os: String,
}

/// Abre o registro e começa a medir o sistema. Falhas aqui nunca impedem o app de abrir.
pub fn setup(app: &App) {
    let log = app
        .path()
        .app_local_data_dir()
        .ok()
        .and_then(|dir| SessionLog::create(&dir.join("logs")).ok());
    let diagnostics = Diagnostics(Mutex::new(log));
    diagnostics.write(
        "app",
        &AppStarted {
            version: env!("CARGO_PKG_VERSION"),
            gpu: sv_gpu::Gpu::new()
                .map(|g| g.adapter_name().to_owned())
                .unwrap_or_else(|e| format!("erro: {e}")),
            os: os_version(),
        },
    );
    app.manage(diagnostics);
    sample_system(app.handle().clone());
}

/// Caminho do registro desta sessão, para a interface mostrar.
#[tauri::command]
pub fn session_log_path(app: AppHandle) -> Option<String> {
    app.state::<Diagnostics>().path()
}

fn sample_system(app: AppHandle) {
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
}
