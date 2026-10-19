//! Prévia local: transmite para si mesmo (captura → encode → decode → janela) para medir
//! o impacto no jogo e a qualidade antes de envolver a rede.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use sv_capture::CaptureSource;
use sv_core::StreamQuality;
use sv_gpu::{Gpu, clock};
use sv_media::{Bounds, WindowEvent, WindowTarget};

use sv_encode::EncodedFrame;

use crate::surface::{Surface, lock};
use crate::{
    AudioPacket, AudioReceiver, AudioSender, ReceiverEvent, SenderEvent, SessionError, StatsMeter, StreamInfo,
    StreamStats, VideoSender, VideoViewer, ViewerEvent,
};

/// Uma prévia em andamento. Para quando é descartada ou quando a janela é fechada.
pub struct LocalPreview {
    // A ordem importa: o áudio para antes do vídeo, e quem envia antes de quem recebe.
    _audio_sender: Option<AudioSender>,
    _sender: Arc<Mutex<Option<VideoSender>>>,
    state: Arc<Shared>,
}

impl std::fmt::Debug for LocalPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPreview")
            .field("closed", &self.is_closed())
            .finish()
    }
}

struct Shared {
    meter: Mutex<StatsMeter>,
    base: Mutex<StreamStats>,
    viewer: Mutex<Option<VideoViewer>>,
    audio: Mutex<Option<AudioReceiver>>,
    closed: AtomicBool,
    on_stats: Mutex<Box<dyn FnMut(StreamStats) + Send>>,
    /// Onde a superfície de vídeo abre (embutida no app ou solta).
    target: WindowTarget,
    surface: Mutex<Surface>,
    tap: Option<Arc<dyn StreamTap>>,
    /// Número de sequência dos frames entregues ao `tap`.
    seq: std::sync::atomic::AtomicU32,
}

/// Quem mais recebe o stream além da prévia (a sala: os amigos assistindo).
pub trait StreamTap: Send + Sync + 'static {
    /// O encoder começou: tamanho e fps do vídeo que vai sair.
    fn started(&self, info: &StreamInfo);
    /// Um frame codificado, com número de sequência.
    fn video(&self, seq: u32, frame: &EncodedFrame);
    fn audio(&self, packet: &AudioPacket);
}

/// Pede keyframes ao encoder de uma prévia, de qualquer thread (ex.: alguém começou a assistir).
#[derive(Clone)]
pub struct KeyframeHandle(Weak<Mutex<Option<VideoSender>>>);

impl std::fmt::Debug for KeyframeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyframeHandle")
    }
}

impl KeyframeHandle {
    /// Um handle que não pede nada a ninguém (testes, ou quando o encoder ainda não existe).
    pub fn detached() -> Self {
        Self(Weak::new())
    }

    pub fn request(&self) {
        if let Some(slot) = self.0.upgrade()
            && let Some(s) = lock(&slot).as_ref()
        {
            s.request_keyframe();
        }
    }
}

impl LocalPreview {
    /// Começa a prévia de `source`. Se `audio_pid` vier, captura também o áudio desse processo
    /// (e dos filhos dele). As estatísticas saem por `on_stats` a cada 500 ms.
    pub fn start(
        source: CaptureSource,
        quality: StreamQuality,
        audio_pid: Option<u32>,
        target: WindowTarget,
        tap: Option<Arc<dyn StreamTap>>,
        on_stats: impl FnMut(StreamStats) + Send + 'static,
    ) -> Result<Self, SessionError> {
        let gpu = Gpu::new().map_err(|e| SessionError::Gpu(e.to_string()))?;
        let state = Arc::new(Shared {
            meter: Mutex::new(StatsMeter::new()),
            base: Mutex::new(StreamStats::default()),
            viewer: Mutex::new(None),
            audio: Mutex::new(None),
            closed: AtomicBool::new(false),
            on_stats: Mutex::new(Box::new(on_stats)),
            target,
            surface: Mutex::new(Surface::default()),
            tap,
            seq: std::sync::atomic::AtomicU32::new(0),
        });
        let sender_slot: Arc<Mutex<Option<VideoSender>>> = Arc::new(Mutex::new(None));

        let events_state = state.clone();
        // Fraco: o callback vive dentro do próprio sender, então um Arc forte criaria um ciclo.
        let keyframe_sender: Weak<Mutex<Option<VideoSender>>> = Arc::downgrade(&sender_slot);
        let sender = VideoSender::start(&gpu, source, quality, move |event| {
            on_sender_event(&events_state, &keyframe_sender, event);
        })?;
        *lock(&sender_slot) = Some(sender);
        let audio_sender = audio_pid.and_then(|pid| start_audio(&state, pid));
        Ok(Self {
            _audio_sender: audio_sender,
            _sender: sender_slot,
            state,
        })
    }

    /// Onde desenhar o vídeo embutido (pixels físicos, relativos à janela do app).
    pub fn set_video_bounds(&self, bounds: Bounds) {
        let mut s = lock(&self.state.surface);
        s.bounds = bounds;
        s.apply();
    }

    /// Esconde o vídeo embutido (ex.: um modal da interface abriu por cima) ou mostra de novo.
    pub fn set_video_hidden(&self, hidden: bool) {
        let mut s = lock(&self.state.surface);
        s.hidden = hidden;
        s.apply();
    }

    /// Recebe os pedidos de tela cheia feitos dentro do vídeo (dois cliques, F11, Esc).
    pub fn on_video_event(&self, cb: impl Fn(WindowEvent) + Send + Sync + 'static) {
        lock(&self.state.surface).set_on_event(Arc::new(cb));
    }

    /// Liga ou desliga o som da prévia no PC (o áudio continua sendo capturado e medido).
    /// Desligado por padrão: o som do jogo sairia duas vezes, o original e o da prévia.
    pub fn set_listening(&self, listening: bool) {
        if let Some(audio) = lock(&self.state.audio).as_ref() {
            audio.set_listening(listening);
        }
    }

    /// Para pedir keyframes de fora (a sala, quando alguém entra para assistir).
    pub fn keyframe_handle(&self) -> KeyframeHandle {
        KeyframeHandle(Arc::downgrade(&self._sender))
    }

    /// O usuário fechou a janela da prévia.
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Relaxed)
    }
}

impl Drop for LocalPreview {
    fn drop(&mut self) {
        // Fecha a janela e a saída de áudio antes de parar a captura.
        lock(&self.state.viewer).take();
        lock(&self.state.audio).take();
    }
}

/// Liga o áudio do processo na prévia. Se falhar, a prévia segue só com vídeo e o motivo vai
/// para as estatísticas.
fn start_audio(state: &Arc<Shared>, pid: u32) -> Option<AudioSender> {
    let receiver_state = Arc::downgrade(state);
    let receiver = AudioReceiver::start(false, move |event| {
        let Some(state) = receiver_state.upgrade() else { return };
        match event {
            ReceiverEvent::Lost => lock(&state.meter).record_audio_lost(),
            ReceiverEvent::PlaybackFailed(e) => {
                lock(&state.base).audio_error = Some(format!("não deu para tocar o som da prévia: {e}"));
            }
        }
    });
    let receiver = match receiver {
        Ok(r) => r,
        Err(e) => {
            lock(&state.base).audio_error = Some(e);
            return None;
        }
    };
    *lock(&state.audio) = Some(receiver);

    let sender_state = Arc::downgrade(state);
    match AudioSender::start(pid, move |packet| {
        let Some(state) = sender_state.upgrade() else { return };
        lock(&state.meter).record_audio(packet.data.len(), packet.peak);
        if let Some(tap) = &state.tap {
            tap.audio(&packet);
        }
        if let Some(receiver) = lock(&state.audio).as_ref() {
            receiver.submit(packet);
        }
    }) {
        Ok(sender) => {
            lock(&state.base).audio_active = true;
            Some(sender)
        }
        Err(e) => {
            lock(&state.base).audio_error = Some(e);
            lock(&state.audio).take();
            None
        }
    }
}

fn on_sender_event(state: &Arc<Shared>, sender: &Weak<Mutex<Option<VideoSender>>>, event: SenderEvent) {
    match event {
        SenderEvent::Started(info) => {
            if let Some(tap) = &state.tap {
                tap.started(&info);
            }
            let mut base = lock(&state.base);
            *base = StreamStats {
                width: info.size.width,
                height: info.size.height,
                encoder: info.encoder.clone(),
                // O áudio começa antes do primeiro quadro: preserva o que ele já registrou.
                audio_active: base.audio_active,
                audio_error: base.audio_error.take(),
                ..Default::default()
            };
            drop(base);
            let viewer_state = Arc::downgrade(state);
            let sender = sender.clone();
            let window_state = Arc::downgrade(state);
            let viewer = VideoViewer::start(
                state.target.clone(),
                info,
                Arc::new(AtomicI64::new(0)),
                move |control| {
                    let Some(state) = window_state.upgrade() else { return };
                    lock(&state.surface).attach(control);
                },
                move |event| {
                    let Some(state) = viewer_state.upgrade() else { return };
                    match event {
                        ViewerEvent::Presented { latency_us } => lock(&state.meter).record_presented(latency_us),
                        ViewerEvent::NeedKeyframe => {
                            if let Some(slot) = sender.upgrade()
                                && let Some(s) = lock(&slot).as_ref()
                            {
                                s.request_keyframe();
                            }
                        }
                        ViewerEvent::Closed | ViewerEvent::Failed(_) => state.closed.store(true, Ordering::Relaxed),
                    }
                },
            );
            *lock(&state.viewer) = Some(viewer);
        }
        SenderEvent::Frame(frame) => {
            if let Some(tap) = &state.tap {
                tap.video(state.seq.fetch_add(1, Ordering::Relaxed), &frame);
            }
            lock(&state.meter).record_encoded(frame.data.len(), frame.encode_us);
            if let Some(viewer) = lock(&state.viewer).as_ref() {
                viewer.submit(frame);
            }
            let base = lock(&state.base).clone();
            let ready = lock(&state.meter).poll(clock::now_us(), &base);
            if let Some(stats) = ready {
                (lock(&state.on_stats))(stats);
            }
        }
        SenderEvent::Captured => lock(&state.meter).record_source_frame(),
        SenderEvent::Dropped => lock(&state.meter).record_dropped(),
        SenderEvent::Failed(_) => state.closed.store(true, Ordering::Relaxed),
    }
}
