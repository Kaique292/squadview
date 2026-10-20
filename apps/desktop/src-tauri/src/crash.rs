//! Falhas fatais: o app de release não tem console, então um erro na abertura fecharia o app em
//! silêncio (só a ampulheta no cursor). Aqui o erro vai para `logs\crash.log` e aparece numa
//! caixa de mensagem, para a pessoa saber o que houve e poder mandar o arquivo.

use std::io::Write;
use std::path::PathBuf;

/// Pasta dos registros (a mesma do registro de desempenho), sem depender do Tauri estar de pé.
fn logs_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("io.github.kaique292.squadview").join("logs"))
}

/// Acrescenta uma linha com a hora, a versão e a mensagem ao `crash.log`.
fn record(message: &str) {
    if let Some(dir) = logs_dir() {
        record_in(&dir, message);
    }
}

fn record_in(dir: &std::path::Path, message: &str) {
    let _ = std::fs::create_dir_all(dir);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("crash.log"))
    {
        let _ = writeln!(file, "[{secs}] SquadView {}: {message}", env!("CARGO_PKG_VERSION"));
    }
}

/// Caixa de mensagem do Windows (funciona mesmo sem a interface).
fn message_box(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;
    // SAFETY: strings válidas até o fim da chamada; sem janela dona.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from("SquadView"),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Grava qualquer pânico no `crash.log`. O da thread principal também avisa a pessoa.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!(" ({}:{})", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "erro desconhecido".into());
        let thread = std::thread::current().name().unwrap_or("?").to_owned();
        record(&format!("pânico na thread {thread}: {payload}{location}"));
        if thread == "main" {
            message_box(&format!(
                "O SquadView fechou por um erro:\n\n{payload}\n\nO detalhe ficou em %LOCALAPPDATA%\\io.github.kaique292.squadview\\logs\\crash.log."
            ));
        }
        previous(info);
    }));
}

/// Erro sem volta na abertura: registra, avisa e fecha.
pub fn fatal(message: &str) -> ! {
    record(message);
    message_box(&format!(
        "{message}\n\nSe continuar, reinstale o SquadView. O detalhe ficou em %LOCALAPPDATA%\\io.github.kaique292.squadview\\logs\\crash.log."
    ));
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    #[test]
    fn crash_log_acumula_linhas_com_versao() {
        let dir = std::env::temp_dir().join(format!("sv-crash-{}", std::process::id()));
        super::record_in(&dir, "primeiro");
        super::record_in(&dir, "segundo");
        let log = std::fs::read_to_string(dir.join("crash.log")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(&format!("SquadView {}: primeiro", env!("CARGO_PKG_VERSION"))));
        assert!(lines[1].ends_with("segundo"));
    }

    #[test]
    fn pasta_dos_registros_e_a_do_app() {
        let dir = super::logs_dir().expect("LOCALAPPDATA no Windows");
        assert!(
            dir.ends_with("io.github.kaique292.squadview\\logs") || dir.ends_with("io.github.kaique292.squadview/logs")
        );
    }
}
