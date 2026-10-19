//! Comandos da prévia local (Fase 1): escolher a fonte, iniciar, parar e receber estatísticas.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use sv_capture::{CaptureSource, Source};
use sv_core::StreamQuality;
use sv_encode::EncodedFrame;
use sv_session::{AudioPacket, Bounds, LocalPreview, StreamInfo, StreamTap, WindowEvent, WindowTarget};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::diagnostics::Diagnostics;

/// Evento com as estatísticas, a cada 500 ms.
pub const STATS_EVENT: &str = "stream-stats";
/// Evento quando a prévia termina (janela fechada ou erro).
pub const ENDED_EVENT: &str = "stream-ended";
/// Evento com um aviso sobre a fonte (`null` quando o aviso deixa de valer).
pub const NOTICE_EVENT: &str = "stream-notice";
/// Pedido de tela cheia feito dentro do vídeo: `"toggle"` (dois cliques, F11) ou `"exit"` (Esc).
pub const FULLSCREEN_EVENT: &str = "video-fullscreen";

/// Sem estatística por esse tempo, com a janela viva, avisamos que não está chegando imagem.
const STALL_AFTER_MS: u64 = 4_000;
/// Por quanto tempo o aviso "Agora transmitindo…" fica na tela depois de trocar de janela.
const CHANGED_NOTICE_FOR: Duration = Duration::from_secs(6);

/// Agora, em ms do relógio do pipeline (o mesmo nas threads da prévia e do vigia).
fn now_ms() -> u64 {
    sv_gpu::clock::now_us() / 1000
}

/// Para onde vai o que a prévia produz: a sala atual (os amigos assistindo) ou ninguém.
/// Entrar ou sair de uma sala troca o destino sem reiniciar a captura.
#[derive(Default)]
pub struct TapSlot {
    tap: Mutex<Option<Arc<dyn StreamTap>>>,
    /// Formato do stream atual, para avisar uma sala em que entramos no meio da transmissão.
    info: Mutex<Option<StreamInfo>>,
}

struct DynamicTap(Arc<TapSlot>);

impl StreamTap for DynamicTap {
    fn started(&self, info: &StreamInfo) {
        *self.0.info.lock().unwrap_or_else(|e| e.into_inner()) = Some(info.clone());
        if let Some(t) = self.0.tap.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            t.started(info);
        }
    }
    fn video(&self, seq: u32, frame: &EncodedFrame) {
        if let Some(t) = self.0.tap.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            t.video(seq, frame);
        }
    }
    fn audio(&self, packet: &AudioPacket) {
        if let Some(t) = self.0.tap.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            t.audio(packet);
        }
    }
}

/// A prévia em andamento. `generation` muda a cada início, para o vigia antigo saber que acabou.
#[derive(Default)]
pub struct PreviewState(Mutex<Running>);

#[derive(Default)]
struct Running {
    preview: Option<LocalPreview>,
    generation: u64,
    /// "Ouvir a prévia" ligado (vale também para a prévia nova depois de uma troca de janela).
    listening: bool,
    layout: Layout,
}

/// Aviso mostrado na prévia.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub kind: &'static str,
    pub message: String,
}

/// Monitores e janelas que podem ser transmitidos.
#[tauri::command]
pub fn list_sources() -> Vec<Source> {
    sv_capture::list_sources()
}

/// Miniatura da fonte como `data:` URL JPEG, ou `None` se ela não mandou imagem a tempo
/// (ex.: janela minimizada). Roda fora da thread principal.
#[tauri::command]
pub async fn source_thumbnail(source: CaptureSource) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let gpu = sv_gpu::Gpu::new().ok()?;
        let thumb = sv_capture::thumbnail(&gpu, source, sv_gpu::Size::new(384, 216), Duration::from_millis(800))?;
        jpeg_data_url(&thumb)
    })
    .await
    .ok()
    .flatten()
}

/// Evento com a foto do último quadro (JPEG em data URL): a interface põe a foto no lugar do
/// vídeo e faz a animação de saída.
pub const VIDEO_EXIT_EVENT: &str = "video-exit";
/// Tempo para a interface desenhar a foto por baixo do vídeo antes de ele fechar. Assim a troca
/// do vídeo nativo pela foto não pisca.
const EXIT_HANDOFF: Duration = Duration::from_millis(160);

/// Manda a foto do último quadro para a interface e dá tempo de ela aparecer. Chame fora da
/// thread principal, logo antes de fechar o vídeo.
pub(crate) fn hand_off_exit(app: &AppHandle, photo: Option<sv_capture::Thumbnail>) {
    if let Some(url) = photo.as_ref().and_then(jpeg_data_url) {
        let _ = app.emit(VIDEO_EXIT_EVENT, url);
        std::thread::sleep(EXIT_HANDOFF);
    }
}

pub(crate) fn jpeg_data_url(thumb: &sv_capture::Thumbnail) -> Option<String> {
    use base64::Engine;
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 80)
        .encode(
            &thumb.rgb,
            thumb.width as u16,
            thumb.height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

/// Roda `work` fora da thread principal e espera.
///
/// Abrir e fechar sessões de vídeo **nunca** pode acontecer na thread principal: a janela do vídeo
/// é filha da janela do app, e fechá-la faz o Windows pedir confirmação à thread principal. Se ela
/// estiver esperando o fechamento, as duas travam (era o "Não está respondendo" ao clicar em Parar).
pub async fn off_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

/// Começa a prévia local. Substitui a anterior, se houver.
#[tauri::command]
pub async fn start_preview(
    app: AppHandle,
    source: CaptureSource,
    quality: StreamQuality,
    audio_pid: Option<u32>,
) -> Result<(), String> {
    off_main(move || start_preview_blocking(app, source, quality, audio_pid)).await?
}

fn start_preview_blocking(
    app: AppHandle,
    source: CaptureSource,
    quality: StreamQuality,
    audio_pid: Option<u32>,
) -> Result<(), String> {
    let state = app.state::<PreviewState>();
    // A anterior (se houver) para antes da nova começar: uma captura e um encoder por vez.
    let old = state.0.lock().unwrap_or_else(|e| e.into_inner()).preview.take();
    drop(old);
    let info = sv_capture::list_sources().into_iter().find(|s| s.id == source);
    app.state::<Diagnostics>().write(
        "stream-start",
        &json!({ "source": info, "quality": quality, "audioPid": audio_pid }),
    );
    let last_stats = Arc::new(AtomicU64::new(0));
    let preview = launch(&app, source, quality, audio_pid, &last_stats).map_err(|e| {
        app.state::<Diagnostics>()
            .write("stream-error", &json!({ "message": e }));
        e
    })?;

    let generation = {
        let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
        running.generation += 1;
        running.listening = false;
        running.preview = Some(preview);
        running.generation
    };
    let _ = app.emit(NOTICE_EVENT, None::<Notice>);
    watch(app, generation, info, quality, audio_pid, last_stats);
    Ok(())
}

/// Liga ou desliga o som da prévia no PC.
#[tauri::command]
pub fn set_preview_listening(state: State<'_, PreviewState>, listening: bool) {
    let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    running.listening = listening;
    if let Some(preview) = running.preview.as_ref() {
        preview.set_listening(listening);
    }
}

/// Para a prévia (fora da thread principal: ver [`off_main`]).
#[tauri::command]
/// Para a transmissão. Antes de fechar o vídeo, manda a foto do último quadro para a animação
/// de saída (evento `video-exit`).
pub async fn stop_preview(app: AppHandle) -> Result<(), String> {
    off_main(move || {
        let state = app.state::<PreviewState>();
        let stopped = {
            let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
            running.generation += 1;
            running.preview.take()
        };
        let Some(preview) = stopped else { return };
        hand_off_exit(&app, preview.snapshot());
        // Fecha vídeo, áudio, encoder e captura, sem segurar nenhum lock.
        drop(preview);
        crate::room::stream_changed(&app, None);
        app.state::<Diagnostics>()
            .write("stream-end", &json!({ "reason": "parado pela interface" }));
    })
    .await
}

fn launch(
    app: &AppHandle,
    source: CaptureSource,
    quality: StreamQuality,
    audio_pid: Option<u32>,
    last_stats: &Arc<AtomicU64>,
) -> Result<LocalPreview, String> {
    let stats_app = app.clone();
    let last = last_stats.clone();
    // Conta a partir do início: sem nenhuma estatística em 4 s, também é "sem imagem".
    last.store(now_ms(), Ordering::Relaxed);
    let slot = app.state::<Arc<TapSlot>>().inner().clone();
    *slot.info.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let tap: Arc<dyn StreamTap> = Arc::new(DynamicTap(slot));
    let preview = LocalPreview::start(
        source,
        quality,
        audio_pid,
        video_target(app)?,
        Some(tap),
        move |stats| {
            last.store(now_ms(), Ordering::Relaxed);
            stats_app.state::<Diagnostics>().write("stream", &stats);
            let _ = stats_app.emit(STATS_EVENT, stats);
        },
    )
    .map_err(|e| e.to_string())?;

    // Dois cliques, F11 ou Esc dentro do vídeo: quem muda o layout é a interface.
    let event_app = app.clone();
    preview.on_video_event(move |event| {
        let kind = match event {
            WindowEvent::ToggleFullscreen => "toggle",
            WindowEvent::ExitFullscreen => "exit",
        };
        let _ = event_app.emit(FULLSCREEN_EVENT, kind);
    });
    layout(app).apply(&preview);
    // Na sala, os amigos passam a ver "AO VIVO" e podem pedir para assistir.
    crate::room::stream_changed(app, Some(preview.keyframe_handle()));
    Ok(preview)
}

/// Entramos ou saímos de uma sala: o stream em andamento passa a ir (ou deixa de ir) para ela.
pub fn restart_for_room(app: &AppHandle) {
    let slot = app.state::<Arc<TapSlot>>().inner().clone();
    let tap = crate::room::tap(app);
    *slot.tap.lock().unwrap_or_else(|e| e.into_inner()) = tap.clone();
    let running = app.state::<PreviewState>();
    let running = running.0.lock().unwrap_or_else(|e| e.into_inner());
    if let (Some(preview), Some(tap)) = (running.preview.as_ref(), tap) {
        if let Some(info) = slot.info.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            tap.started(info);
        }
        crate::room::stream_changed(app, Some(preview.keyframe_handle()));
    }
}

/// Posição atual pedida pela interface.
pub fn layout(app: &AppHandle) -> Layout {
    app.state::<PreviewState>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .layout
}

/// Começamos ou paramos de assistir um amigo: a área do vídeo passa a ser dele (a nossa prévia
/// sai da tela, mas a transmissão continua para quem estiver nos assistindo).
pub fn set_watching(app: &AppHandle, watching: bool) {
    let state = app.state::<PreviewState>();
    let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    running.layout.watching = watching;
    if let Some(p) = running.preview.as_ref() {
        running.layout.apply(p);
    }
}

/// O vídeo abre dentro da janela principal do app, como no Discord.
pub fn video_target(app: &AppHandle) -> Result<WindowTarget, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("janela principal não encontrada")?;
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    Ok(WindowTarget::Embedded {
        parent: hwnd.0 as isize,
    })
}

/// Onde a interface quer o vídeo (pixels físicos) e se ele está escondido por um modal.
#[derive(Debug, Default, Clone, Copy)]
pub struct Layout {
    pub bounds: Bounds,
    pub hidden: bool,
    /// Estamos assistindo um amigo: a área é dele, a prévia fica fora da tela.
    pub watching: bool,
}

impl Layout {
    fn apply(self, preview: &LocalPreview) {
        preview.set_video_bounds(if self.watching { Bounds::default() } else { self.bounds });
        preview.set_video_hidden(self.hidden);
    }
}

/// A área do stream na interface mudou de lugar ou de tamanho (pixels físicos, relativos à janela).
#[tauri::command]
pub fn set_video_bounds(app: AppHandle, state: State<'_, PreviewState>, x: i32, y: i32, width: u32, height: u32) {
    let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    running.layout.bounds = Bounds { x, y, width, height };
    let layout = running.layout;
    if let Some(p) = running.preview.as_ref() {
        layout.apply(p);
    }
    drop(running);
    crate::room::apply_layout(&app, layout.bounds, layout.hidden);
}

/// Um modal da interface abriu (o vídeo nativo ficaria por cima dele) ou fechou.
#[tauri::command]
pub fn set_video_hidden(app: AppHandle, state: State<'_, PreviewState>, hidden: bool) {
    let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    running.layout.hidden = hidden;
    let layout = running.layout;
    if let Some(p) = running.preview.as_ref() {
        layout.apply(p);
    }
    drop(running);
    crate::room::apply_layout(&app, layout.bounds, layout.hidden);
}

/// Liga ou desliga a tela cheia da janela do app (o stream ocupa tudo).
#[tauri::command]
pub fn set_fullscreen(app: AppHandle, fullscreen: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("janela principal não encontrada")?;
    window.set_fullscreen(fullscreen).map_err(|e| e.to_string())
}

/// Vigia a prévia: avisa quando ela é fechada, segue o jogo quando ele troca de janela
/// (ex.: LoL saindo do lobby para a partida) e avisa quando a imagem para de chegar.
fn watch(
    app: AppHandle,
    generation: u64,
    source: Option<Source>,
    quality: StreamQuality,
    audio_pid: Option<u32>,
    last_stats: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        let mut current = source;
        let mut audio_pid = audio_pid;
        let mut changed: Option<(Instant, Notice)> = None;
        let mut notice: Option<Notice> = None;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let state = app.state::<PreviewState>();
            let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
            if running.generation != generation {
                return; // Parada pela interface ou trocada por outra prévia.
            }
            if running.preview.as_ref().is_none_or(LocalPreview::is_closed) {
                let closed = running.preview.take();
                drop(running);
                drop(closed);
                crate::room::stream_changed(&app, None);
                app.state::<Diagnostics>()
                    .write("stream-end", &json!({ "reason": "janela da prévia fechada" }));
                let _ = app.emit(ENDED_EVENT, ());
                return;
            }

            let Some(src) = current.clone() else { continue };
            let next = if sv_capture::is_alive(src.id) {
                let quiet_ms = now_ms().saturating_sub(last_stats.load(Ordering::Relaxed));
                // Tela parada também não gera quadros; só janelas (jogos desenham sem parar) contam.
                let stalled = matches!(src.id, CaptureSource::Window(_)) && quiet_ms > STALL_AFTER_MS;
                stalled.then(|| Notice {
                    kind: "no-frames",
                    message: "A imagem parou de chegar. Se o jogo estiver em tela cheia exclusiva, troque para \
                              \"janela sem bordas\" nas opções de vídeo dele."
                        .into(),
                })
            } else if let Some(replacement) = sv_capture::find_replacement(&src) {
                drop(running);
                // O áudio era do jogo: passa a ser do processo da janela nova.
                let next_audio = if audio_pid.is_some() && audio_pid == src.process_id {
                    replacement.process_id
                } else {
                    audio_pid
                };
                match switch(&app, generation, &src, &replacement, quality, next_audio, &last_stats) {
                    Some(()) => {
                        audio_pid = next_audio;
                        let msg = Notice {
                            kind: "source-changed",
                            message: format!("O jogo trocou de janela. Agora transmitindo “{}”.", replacement.name),
                        };
                        changed = Some((Instant::now(), msg));
                        current = Some(replacement);
                        None
                    }
                    None => return,
                }
            } else {
                Some(Notice {
                    kind: "source-lost",
                    message: format!("“{}” sumiu. Esperando a janela do jogo voltar…", src.name),
                })
            };
            // Logo depois de trocar de janela, o aviso da troca vale mais que "sem imagem" (a nova
            // captura ainda está começando).
            let next = match &changed {
                Some((at, msg)) if at.elapsed() < CHANGED_NOTICE_FOR => Some(msg.clone()),
                _ => next,
            };
            if next != notice {
                let _ = app.emit(NOTICE_EVENT, next.clone());
                notice = next;
            }
        }
    });
}

/// Troca a fonte da prévia em andamento. `None` se a prévia foi parada no meio da troca.
#[allow(clippy::too_many_arguments)]
fn switch(
    app: &AppHandle,
    generation: u64,
    from: &Source,
    to: &Source,
    quality: StreamQuality,
    audio_pid: Option<u32>,
    last_stats: &Arc<AtomicU64>,
) -> Option<()> {
    let state = app.state::<PreviewState>();
    // Para a captura antiga antes de começar a nova (uma GPU, um encoder por vez).
    {
        let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if running.generation != generation {
            return None;
        }
        running.preview = None;
    }
    let preview = launch(app, to.id, quality, audio_pid, last_stats);
    let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if running.generation != generation {
        return None;
    }
    app.state::<Diagnostics>().write(
        "stream-source-changed",
        &json!({ "from": from.name, "to": to.name, "ok": preview.is_ok() }),
    );
    match preview {
        Ok(p) => {
            p.set_listening(running.listening);
            running.layout.apply(&p);
            running.preview = Some(p);
            Some(())
        }
        Err(e) => {
            running.generation += 1;
            drop(running);
            app.state::<Diagnostics>()
                .write("stream-error", &json!({ "message": e }));
            let _ = app.emit(ENDED_EVENT, ());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miniatura_vira_jpeg_em_data_url() {
        let thumb = sv_capture::Thumbnail {
            width: 2,
            height: 2,
            rgb: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        };
        let url = jpeg_data_url(&thumb).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,/9j/"), "{url}");
    }
}
