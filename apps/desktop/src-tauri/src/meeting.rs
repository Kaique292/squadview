//! Modo reunião (ADR 0032), parte 1: a transcrição da nossa voz, no nosso PC, para a aba Reunião.
//!
//! - O modelo do Whisper não vem no instalador: é baixado na primeira vez, com progresso, para a
//!   pasta de dados do app, e conferido pelo SHA-256 antes de ser usado.
//! - Ligar a transcrição carrega o modelo (fora da thread da interface) e o entrega ao motor de
//!   voz da sala, que corta as falas do microfone e as transcreve numa thread de prioridade baixa.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sv_media::transcribe::{WhisperTranscriber, cpu_supported};
use sv_session::TranscriptLine;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::room::{RoomState, lock};

/// Uma linha nova da nossa transcrição.
pub const LINE_EVENT: &str = "meeting-line";
/// Progresso do download do modelo.
pub const PROGRESS_EVENT: &str = "meeting-model-progress";

/// Pasta dos modelos, dentro da pasta de dados local do app.
const MODELS_DIR: &str = "modelos";
/// Progresso para a interface no máximo a cada 200 ms.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Os modelos que dá para escolher (ADR 0032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    /// `small`: o padrão, melhor em português.
    Small,
    /// `base`: menor e mais leve, para PC mais fraco.
    Base,
}

struct ModelSpec {
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
    size: u64,
}

impl ModelKind {
    const ALL: [Self; 2] = [Self::Small, Self::Base];

    fn spec(self) -> ModelSpec {
        match self {
            Self::Small => ModelSpec {
                file: "ggml-small-q5_1.bin",
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
                sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
                size: 190_085_487,
            },
            Self::Base => ModelSpec {
                file: "ggml-base-q5_1.bin",
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
                sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
                size: 59_707_625,
            },
        }
    }
}

/// Estado do modo reunião no app: se há um download em andamento e o pedido de cancelar.
#[derive(Default)]
pub struct Meeting {
    downloading: AtomicBool,
    cancel: AtomicBool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    kind: ModelKind,
    /// Tamanho do download, em bytes.
    size: u64,
    installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStatus {
    /// O processador tem o que o whisper.cpp do app precisa (AVX2, FMA e F16C).
    cpu_supported: bool,
    models: Vec<ModelStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    kind: ModelKind,
    received: u64,
    total: u64,
}

/// Uma linha da transcrição, como a interface recebe.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Line {
    Text { at_ms: u64, text: String },
    Skipped { at_ms: u64 },
    Failed { at_ms: u64, error: String },
}

impl From<TranscriptLine> for Line {
    fn from(line: TranscriptLine) -> Self {
        match line {
            TranscriptLine::Text { at_ms, text } => Self::Text { at_ms, text },
            TranscriptLine::Skipped { at_ms } => Self::Skipped { at_ms },
            TranscriptLine::Failed { at_ms, error } => Self::Failed { at_ms, error },
        }
    }
}

/// Repassa uma linha da transcrição para a interface.
pub fn emit_line<R: Runtime>(app: &AppHandle<R>, line: TranscriptLine) {
    let _ = app.emit(LINE_EVENT, Line::from(line));
}

fn models_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join(MODELS_DIR))
}

/// O modelo está baixado (e foi conferido: só fica com o nome final depois do SHA-256).
fn installed(dir: &Path, kind: ModelKind) -> bool {
    let spec = kind.spec();
    std::fs::metadata(dir.join(spec.file)).is_ok_and(|m| m.len() == spec.size)
}

/// O processador serve e quais modelos já estão baixados.
#[tauri::command]
pub fn meeting_status<R: Runtime>(app: AppHandle<R>) -> Result<MeetingStatus, String> {
    let dir = models_dir(&app)?;
    Ok(MeetingStatus {
        cpu_supported: cpu_supported(),
        models: ModelKind::ALL
            .into_iter()
            .map(|kind| ModelStatus {
                kind,
                size: kind.spec().size,
                installed: installed(&dir, kind),
            })
            .collect(),
    })
}

/// Baixa o modelo, com progresso em [`PROGRESS_EVENT`]. Um download por vez.
#[tauri::command]
pub async fn meeting_download_model<R: Runtime>(app: AppHandle<R>, kind: ModelKind) -> Result<(), String> {
    let meeting = app.state::<Meeting>();
    if meeting.downloading.swap(true, Ordering::AcqRel) {
        return Err("Já tem um modelo sendo baixado.".into());
    }
    meeting.cancel.store(false, Ordering::Release);
    let result = async {
        let dir = models_dir(&app)?;
        if installed(&dir, kind) {
            return Ok(());
        }
        let spec = kind.spec();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        download(
            &http,
            spec.url,
            spec.sha256,
            spec.size,
            &dir.join(spec.file),
            &meeting.cancel,
            |received, total| {
                let _ = app.emit(PROGRESS_EVENT, Progress { kind, received, total });
            },
        )
        .await
    }
    .await;
    meeting.downloading.store(false, Ordering::Release);
    result
}

/// Cancela o download em andamento.
#[tauri::command]
pub fn meeting_cancel_download(meeting: State<'_, Meeting>) {
    meeting.cancel.store(true, Ordering::Release);
}

/// Apaga um modelo baixado.
#[tauri::command]
pub fn meeting_delete_model<R: Runtime>(app: AppHandle<R>, kind: ModelKind) -> Result<(), String> {
    let path = models_dir(&app)?.join(kind.spec().file);
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Não deu para apagar o modelo: {e}")),
        _ => Ok(()),
    }
}

/// Liga (com o modelo e o idioma) ou desliga a transcrição da nossa voz. `language`: código
/// do idioma (`"pt"`), ou nulo para o Whisper detectar sozinho.
#[tauri::command]
pub async fn meeting_set_transcribing<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
    model: ModelKind,
    language: Option<String>,
) -> Result<(), String> {
    let transcriber = if enabled {
        let path = models_dir(&app)?.join(model.spec().file);
        if !installed(path.parent().unwrap_or(&path), model) {
            return Err("O modelo de transcrição ainda não foi baixado.".into());
        }
        // Carregar o modelo leva um tempo: fora da thread da interface.
        let transcriber =
            tauri::async_runtime::spawn_blocking(move || WhisperTranscriber::load(&path, language.as_deref()))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("Não deu para carregar o modelo: {e}"))?;
        Some(Box::new(transcriber) as Box<dyn sv_media::transcribe::Transcriber>)
    } else {
        None
    };
    let state = app.state::<RoomState>();
    let running = lock(&state);
    let voice = running.room.as_ref().and_then(|r| r.voice());
    match (voice, transcriber) {
        (Some(voice), transcriber) => voice.set_transcriber(transcriber),
        (None, Some(_)) => Err("Entre na voz da sala para transcrever.".into()),
        (None, None) => Ok(()),
    }
}

/// Baixa `url` para `dest`, conferindo o tamanho e o SHA-256. Baixa num `.part` ao lado e só
/// renomeia no fim, para nunca sobrar um modelo pela metade com o nome certo.
async fn download(
    http: &reqwest::Client,
    url: &str,
    sha256: &str,
    size: u64,
    dest: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Não deu para criar a pasta do modelo: {e}"))?;
    }
    let part = dest.with_extension("part");
    let result = async {
        let mut response = http
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Não deu para baixar o modelo: {e}"))?;
        let mut file = std::fs::File::create(&part).map_err(|e| format!("Não deu para salvar o modelo: {e}"))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut last = Instant::now();
        on_progress(0, size);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("O download do modelo caiu: {e}"))?
        {
            if cancel.load(Ordering::Acquire) {
                return Err("Download cancelado.".to_string());
            }
            received += chunk.len() as u64;
            if received > size {
                return Err("O modelo baixado veio maior que o esperado.".into());
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .map_err(|e| format!("Não deu para salvar o modelo: {e}"))?;
            if last.elapsed() >= PROGRESS_INTERVAL {
                last = Instant::now();
                on_progress(received, size);
            }
        }
        file.flush().map_err(|e| format!("Não deu para salvar o modelo: {e}"))?;
        drop(file);
        if received != size {
            return Err("O download do modelo veio incompleto.".into());
        }
        let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest != sha256 {
            return Err("O modelo baixado não confere (SHA-256 diferente).".into());
        }
        std::fs::rename(&part, dest).map_err(|e| format!("Não deu para salvar o modelo: {e}"))?;
        on_progress(size, size);
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_support::test_app;

    /// Um servidor HTTP de mentira que responde `body` a cada pedido. Devolve a URL.
    async fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move {
                    let mut request = [0u8; 1024];
                    let _ = socket.read(&mut request).await;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        format!("http://{addr}/modelo.bin")
    }

    /// Como no `run()` do app: o HTTPS usa o rustls com o provedor ring.
    fn http() -> reqwest::Client {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::new()
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sv-meeting-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn run<T>(f: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn baixa_o_modelo_confere_e_so_entao_salva_com_o_nome_final() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let dir = temp_dir("ok");
        let dest = dir.join("modelo.bin");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut last = (0, 0);
        let result = run(async {
            let url = serve(body.clone()).await;
            let cancel = AtomicBool::new(false);
            download(
                &http(),
                &url,
                &sha(&body),
                body.len() as u64,
                &dest,
                &cancel,
                |received, total| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    last = (received, total);
                },
            )
            .await
        });
        assert_eq!(result, Ok(()));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!dest.with_extension("part").exists());
        assert_eq!(last, (body.len() as u64, body.len() as u64), "termina em 100%");
        assert!(calls.load(Ordering::Relaxed) >= 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn modelo_adulterado_e_recusado_e_nao_fica_no_disco() {
        let body = vec![7u8; 1000];
        let dir = temp_dir("sha");
        let dest = dir.join("modelo.bin");
        let result = run(async {
            let url = serve(body.clone()).await;
            download(
                &http(),
                &url,
                &sha(b"outro arquivo"),
                1000,
                &dest,
                &AtomicBool::new(false),
                |_, _| {},
            )
            .await
        });
        assert!(result.unwrap_err().contains("SHA-256"));
        assert!(!dest.exists() && !dest.with_extension("part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_de_tamanho_errado_e_recusado() {
        let body = vec![1u8; 500];
        let dir = temp_dir("tamanho");
        let dest = dir.join("modelo.bin");
        let result = run(async {
            let url = serve(body.clone()).await;
            download(
                &http(),
                &url,
                &sha(&body),
                1000,
                &dest,
                &AtomicBool::new(false),
                |_, _| {},
            )
            .await
        });
        assert!(result.unwrap_err().contains("incompleto"));
        assert!(!dest.exists() && !dest.with_extension("part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelar_para_o_download_e_apaga_o_pedaco() {
        let body = vec![3u8; 100_000];
        let dir = temp_dir("cancelar");
        let dest = dir.join("modelo.bin");
        let result = run(async {
            let url = serve(body.clone()).await;
            download(
                &http(),
                &url,
                &sha(&body),
                body.len() as u64,
                &dest,
                &AtomicBool::new(true),
                |_, _| {},
            )
            .await
        });
        assert_eq!(result, Err("Download cancelado.".into()));
        assert!(!dest.exists() && !dest.with_extension("part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn status_lista_os_dois_modelos_ainda_nao_baixados() {
        let t = test_app();
        let status = meeting_status(t.handle().clone()).unwrap();
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["models"][0]["kind"], "small");
        assert_eq!(json["models"][1]["kind"], "base");
        assert_eq!(json["models"][0]["installed"], false);
        assert_eq!(json["models"][0]["size"], 190_085_487);
        assert!(json["cpuSupported"].is_boolean());
        // Apagar o que não existe não é erro.
        meeting_delete_model(t.handle().clone(), ModelKind::Base).unwrap();
    }

    #[test]
    fn ligar_sem_o_modelo_baixado_avisa_e_desligar_fora_da_sala_nao_falha() {
        let t = test_app();
        let on = run(meeting_set_transcribing(
            t.handle().clone(),
            true,
            ModelKind::Small,
            Some("pt".into()),
        ));
        assert!(on.unwrap_err().contains("ainda não foi baixado"));
        run(meeting_set_transcribing(
            t.handle().clone(),
            false,
            ModelKind::Small,
            None,
        ))
        .unwrap();
    }

    #[test]
    fn linhas_da_transcricao_chegam_a_interface() {
        let t = test_app();
        let lines = t.events(LINE_EVENT);
        emit_line(
            t.handle(),
            TranscriptLine::Text {
                at_ms: 42,
                text: "bora jogar".into(),
            },
        );
        lines.wait("texto", |v| {
            v["kind"] == "text" && v["atMs"] == 42 && v["text"] == "bora jogar"
        });
        // Pelo mesmo caminho dos outros eventos da voz.
        crate::voice::emit_event(
            t.handle(),
            sv_session::VoiceEvent::Transcript(TranscriptLine::Skipped { at_ms: 50 }),
        );
        lines.wait("trecho perdido", |v| v["kind"] == "skipped" && v["atMs"] == 50);
    }
}
