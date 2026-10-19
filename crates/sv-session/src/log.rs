//! Registro local da sessão: um arquivo JSON Lines por abertura do app, só no PC do usuário.
//!
//! Nada daqui é enviado para lugar nenhum (o SquadView não tem telemetria). Serve para o
//! próprio usuário, ou para quem ele escolher mandar o arquivo, entender o desempenho.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Quantos registros antigos guardar.
const KEEP: usize = 20;

/// Um arquivo de registro aberto.
#[derive(Debug)]
pub struct SessionLog {
    path: PathBuf,
    file: BufWriter<File>,
    started: Instant,
}

#[derive(Serialize)]
struct Line<'a, T: Serialize> {
    /// Milissegundos desde que o app abriu.
    t: u64,
    kind: &'a str,
    #[serde(flatten)]
    data: &'a T,
}

impl SessionLog {
    /// Cria `dir/sessao-AAAAMMDD-HHMMSSZ.jsonl` e apaga os registros mais antigos além dos últimos 20.
    pub fn create(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = dir.join(format!("sessao-{}.jsonl", utc_stamp(secs)));
        let file = BufWriter::new(File::create(&path)?);
        prune(dir, KEEP);
        Ok(Self {
            path,
            file,
            started: Instant::now(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Acrescenta uma linha `{"t":…, "kind":…, …data}` e grava no disco na hora
    /// (se o app fechar de repente, o registro até ali fica salvo).
    pub fn write(&mut self, kind: &str, data: &impl Serialize) {
        let line = Line {
            t: self.started.elapsed().as_millis() as u64,
            kind,
            data,
        };
        if let Ok(json) = serde_json::to_string(&line) {
            let _ = writeln!(self.file, "{json}");
            let _ = self.file.flush();
        }
    }
}

/// `AAAAMMDD-HHMMSSZ` em UTC, sem depender de biblioteca de datas.
pub(crate) fn utc_stamp(unix_secs: u64) -> String {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Dias desde 1970-01-01 → (ano, mês, dia). Algoritmo de Howard Hinnant.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Mantém só os `keep` registros mais novos (o nome ordena por data).
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sessao-") && n.ends_with(".jsonl"))
        })
        .collect();
    logs.sort();
    let excess = logs.len().saturating_sub(keep);
    for old in &logs[..excess] {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sv-log-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn carimbo_de_data_em_utc() {
        assert_eq!(utc_stamp(0), "19700101-000000Z");
        // 2026-10-19 16:30:05 UTC
        assert_eq!(utc_stamp(1_792_427_405), "20261019-163005Z");
        // 29 de fevereiro de 2024
        assert_eq!(utc_stamp(1_709_164_800), "20240229-000000Z");
    }

    #[test]
    fn grava_uma_linha_json_por_registro() {
        #[derive(Serialize)]
        struct Exemplo {
            fps: f32,
        }
        let dir = temp_dir("linhas");
        let mut log = SessionLog::create(&dir).unwrap();
        log.write("stream", &Exemplo { fps: 60.0 });
        log.write("evento", &serde_json::json!({ "msg": "começou" }));
        let text = fs::read_to_string(log.path()).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["kind"], "stream");
        assert_eq!(lines[0]["fps"], 60.0);
        assert!(lines[0]["t"].is_u64());
        assert_eq!(lines[1]["msg"], "começou");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn guarda_so_os_registros_mais_novos() {
        let dir = temp_dir("limpeza");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..25 {
            File::create(dir.join(format!("sessao-2026010{}-0000{i:02}Z.jsonl", i / 10))).unwrap();
        }
        File::create(dir.join("outro-arquivo.txt")).unwrap();
        prune(&dir, 20);
        let left: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(left.len(), 21, "20 registros + o arquivo que não é registro");
        assert!(!dir.join("sessao-20260100-000000Z.jsonl").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
