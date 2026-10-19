//! Assistir o stream de um amigo: conexão QUIC → decoder por GPU → vídeo dentro do app, e o som
//! do jogo dele pelo buffer de jitter.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use quinn::Connection;
use sv_encode::EncodedFrame;
use sv_gpu::{Size, clock};
use sv_media::{Bounds, WindowEvent, WindowTarget};
use sv_net::protocol::{FrameHeader, StreamDescription};
use sv_net::{KeyframeRequester, Receiver, ReceiverSink};

use crate::surface::{Surface, lock};
use crate::{AudioPacket, AudioReceiver, ReceiverEvent, StatsMeter, StreamInfo, StreamStats, VideoViewer, ViewerEvent};

type OnStats = Box<dyn FnMut(StreamStats) + Send>;
type OnClosed = Box<dyn FnOnce(String) + Send>;

struct Shared {
    meter: Mutex<StatsMeter>,
    base: Mutex<StreamStats>,
    viewer: Mutex<Option<VideoViewer>>,
    audio: Mutex<Option<AudioReceiver>>,
    surface: Mutex<Surface>,
    keyframes: OnceLock<KeyframeRequester>,
    rtt_us: OnceLock<Arc<std::sync::atomic::AtomicU64>>,
    on_stats: Mutex<OnStats>,
    on_closed: Mutex<Option<OnClosed>>,
}

/// O stream de um amigo sendo assistido. Para ao ser descartado.
pub struct RemoteView {
    // A ordem importa: a conexão fecha antes do decoder e da janela.
    receiver: Receiver,
    state: Arc<Shared>,
}

impl std::fmt::Debug for RemoteView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteView").finish_non_exhaustive()
    }
}

impl RemoteView {
    /// Começa a assistir pela conexão `conn` (já aberta). Precisa ser chamado dentro do runtime
    /// tokio da sala (`Room::runtime`).
    pub fn start(
        conn: Connection,
        desc: StreamDescription,
        streamer_name: &str,
        target: WindowTarget,
        on_stats: impl FnMut(StreamStats) + Send + 'static,
        on_closed: impl FnOnce(String) + Send + 'static,
    ) -> Self {
        let state = Arc::new(Shared {
            meter: Mutex::new(StatsMeter::new()),
            base: Mutex::new(StreamStats {
                width: desc.width,
                height: desc.height,
                encoder: format!("transmitido por {streamer_name}"),
                audio_active: desc.audio,
                ..Default::default()
            }),
            viewer: Mutex::new(None),
            audio: Mutex::new(None),
            surface: Mutex::new(Surface::default()),
            keyframes: OnceLock::new(),
            rtt_us: OnceLock::new(),
            on_stats: Mutex::new(Box::new(on_stats)),
            on_closed: Mutex::new(Some(Box::new(on_closed))),
        });

        // O som do jogo do amigo toca direto (diferente da prévia, aqui ele não sai duplicado).
        let audio_state = Arc::downgrade(&state);
        let audio = AudioReceiver::start(true, move |event| {
            let Some(state) = audio_state.upgrade() else { return };
            match event {
                ReceiverEvent::Lost => lock(&state.meter).record_audio_lost(),
                ReceiverEvent::PlaybackFailed(e) => {
                    lock(&state.base).audio_error = Some(format!("não deu para tocar o som: {e}"));
                }
            }
        });
        *lock(&state.audio) = audio.ok();

        let receiver = Receiver::start(conn, Sink { state: state.clone() });
        let _ = state.keyframes.set(receiver.keyframe_requester());

        // O vídeo abre com a primeira imagem; o relógio do amigo é corrigido pela estimativa da conexão.
        let info = StreamInfo {
            size: Size::new(desc.width, desc.height),
            fps: desc.fps,
            encoder: String::new(),
        };
        let window_state = Arc::downgrade(&state);
        let event_state = Arc::downgrade(&state);
        let viewer = VideoViewer::start(
            target,
            info,
            receiver.clock_offset(),
            move |control| {
                if let Some(state) = window_state.upgrade() {
                    lock(&state.surface).attach(control);
                }
            },
            move |event| {
                let Some(state) = event_state.upgrade() else { return };
                match event {
                    ViewerEvent::Presented { latency_us } => lock(&state.meter).record_presented(latency_us),
                    ViewerEvent::NeedKeyframe => {
                        if let Some(k) = state.keyframes.get() {
                            k.request();
                        }
                    }
                    ViewerEvent::Closed | ViewerEvent::Failed(_) => {}
                }
            },
        );
        *lock(&state.viewer) = Some(viewer);

        let _ = state.rtt_us.set(receiver.rtt_handle());
        Self { receiver, state }
    }

    pub fn set_video_bounds(&self, bounds: Bounds) {
        let mut s = lock(&self.state.surface);
        s.bounds = bounds;
        s.apply();
    }

    pub fn set_video_hidden(&self, hidden: bool) {
        let mut s = lock(&self.state.surface);
        s.hidden = hidden;
        s.apply();
    }

    pub fn on_video_event(&self, cb: impl Fn(WindowEvent) + Send + Sync + 'static) {
        lock(&self.state.surface).set_on_event(Arc::new(cb));
    }

    /// Liga ou desliga o som do amigo.
    pub fn set_listening(&self, listening: bool) {
        if let Some(audio) = lock(&self.state.audio).as_ref() {
            audio.set_listening(listening);
        }
    }

    pub fn request_keyframe(&self) {
        self.receiver.request_keyframe();
    }

    /// Foto do último quadro do vídeo, para a animação de saída. Ver [`VideoViewer::snapshot`].
    pub fn snapshot(&self) -> Option<sv_capture::Thumbnail> {
        lock(&self.state.viewer)
            .as_ref()?
            .snapshot(crate::viewer::SNAPSHOT_MAX, std::time::Duration::from_millis(400))
    }
}

impl Drop for RemoteView {
    fn drop(&mut self) {
        lock(&self.state.viewer).take();
        lock(&self.state.audio).take();
    }
}

struct Sink {
    state: Arc<Shared>,
}

impl ReceiverSink for Sink {
    fn video(&mut self, header: FrameHeader, data: Vec<u8>) {
        let state = &self.state;
        lock(&state.meter).record_encoded(data.len(), header.encode_us);
        let frame = EncodedFrame {
            data,
            keyframe: header.keyframe,
            timestamp_us: header.timestamp_us,
            encode_us: header.encode_us,
        };
        let accepted = lock(&state.viewer).as_ref().is_none_or(|v| v.submit(frame));
        if !accepted && let Some(k) = state.keyframes.get() {
            // Fila do decoder cheia: o próximo frame não vai decodificar sem um keyframe.
            k.request();
        }
        let mut base = lock(&state.base).clone();
        base.rtt_ms = state
            .rtt_us
            .get()
            .map_or(0.0, |r| r.load(Ordering::Relaxed) as f32 / 1000.0);
        let ready = lock(&state.meter).poll(clock::now_us(), &base);
        if let Some(stats) = ready {
            (lock(&state.on_stats))(stats);
        }
    }

    fn audio(&mut self, seq: u32, timestamp_us: u64, peak: f32, opus: &[u8]) {
        lock(&self.state.meter).record_audio(opus.len(), peak);
        if let Some(audio) = lock(&self.state.audio).as_ref() {
            audio.submit(AudioPacket {
                seq,
                data: opus.to_vec(),
                peak,
                timestamp_us,
            });
        }
    }

    fn closed(&mut self, reason: String) {
        if let Some(cb) = lock(&self.state.on_closed).take() {
            cb(reason);
        }
    }
}
