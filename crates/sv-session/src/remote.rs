//! Assistir o stream de um amigo: conexão QUIC → decoder por GPU → vídeo dentro do app, e o som
//! do jogo dele pelo buffer de jitter.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use quinn::Connection;
use sv_encode::EncodedFrame;
use sv_gpu::{Size, clock};
use sv_media::{Bounds, Hole, WindowEvent, WindowTarget};
use sv_net::protocol::{FrameHeader, StreamDescription};
use sv_net::{Broadcaster, KeyframeRequester, Receiver, ReceiverSink, VideoFrame};

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
    /// Repasse pela sala (ADR 0014): quem não conecta em quem transmite recebe daqui, os mesmos
    /// frames já codificados (sem decodificar de novo).
    relay: Broadcaster,
    on_stats: Mutex<OnStats>,
    on_closed: Mutex<Option<OnClosed>>,
}

impl Shared {
    fn new(desc: &StreamDescription, streamer_name: &str, on_stats: OnStats, on_closed: OnClosed) -> Arc<Self> {
        Arc::new(Self {
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
            relay: Broadcaster::new(),
            on_stats: Mutex::new(on_stats),
            on_closed: Mutex::new(Some(on_closed)),
        })
    }

    fn on_audio_event(&self, event: ReceiverEvent) {
        match event {
            ReceiverEvent::Lost => lock(&self.meter).record_audio_lost(),
            ReceiverEvent::PlaybackFailed(e) => {
                lock(&self.base).audio_error = Some(format!("não deu para tocar o som: {e}"));
            }
        }
    }

    fn on_viewer_event(&self, event: ViewerEvent) {
        match event {
            ViewerEvent::Presented { latency_us } => lock(&self.meter).record_presented(latency_us),
            ViewerEvent::NeedKeyframe => {
                if let Some(k) = self.keyframes.get() {
                    k.request();
                }
            }
            ViewerEvent::Closed | ViewerEvent::Failed(_) => {}
        }
    }
}

/// O stream de um amigo sendo assistido. Para ao ser descartado.
pub struct RemoteView {
    // A ordem importa: a conexão fecha antes do decoder e da janela.
    receiver: Receiver,
    state: Arc<Shared>,
    desc: StreamDescription,
}

/// O que a sala precisa para repassar um stream que estamos assistindo a quem não alcança quem
/// transmite (ver [`crate::Room::set_relay_source`]).
#[derive(Clone)]
pub struct RelaySource {
    /// Id de quem transmite.
    pub streamer: String,
    pub desc: StreamDescription,
    pub broadcaster: Broadcaster,
    pub keyframes: KeyframeRequester,
}

impl std::fmt::Debug for RelaySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelaySource")
            .field("streamer", &self.streamer)
            .field("viewers", &self.broadcaster.viewer_count())
            .finish()
    }
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
        let state = Shared::new(&desc, streamer_name, Box::new(on_stats), Box::new(on_closed));

        // O som do jogo do amigo toca direto (diferente da prévia, aqui ele não sai duplicado).
        let audio_state = Arc::downgrade(&state);
        let audio = AudioReceiver::start(true, move |event| {
            if let Some(state) = audio_state.upgrade() {
                state.on_audio_event(event);
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
                if let Some(state) = event_state.upgrade() {
                    state.on_viewer_event(event);
                }
            },
        );
        *lock(&state.viewer) = Some(viewer);

        let _ = state.rtt_us.set(receiver.rtt_handle());
        Self { receiver, state, desc }
    }

    /// Para repassar este stream pela sala: quem pedir recebe os mesmos frames, e os pedidos de
    /// keyframe deles sobem até quem transmite (`streamer` é o id dele).
    pub fn relay_source(&self, streamer: &str) -> RelaySource {
        RelaySource {
            streamer: streamer.to_owned(),
            desc: self.desc,
            broadcaster: self.state.relay.clone(),
            keyframes: self.receiver.keyframe_requester(),
        }
    }

    /// Quantas pessoas recebem este stream repassado por nós.
    pub fn relayed_viewers(&self) -> usize {
        self.state.relay.viewer_count()
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

    /// Onde a interface está por cima do vídeo (menus, dicas): ali o vídeo deixa ela aparecer.
    pub fn set_video_holes(&self, holes: Vec<Hole>) {
        let mut s = lock(&self.state.surface);
        s.holes = holes;
        s.apply();
    }

    /// Raio dos cantos do quadro (pixels físicos): o vídeo por baixo da interface acompanha a borda.
    pub fn set_video_radius(&self, radius: u32) {
        let mut s = lock(&self.state.surface);
        s.radius = radius;
        s.apply();
    }

    /// Por baixo da interface (ADR 0021): leva o vídeo para a janela do picture in picture `hwnd`,
    /// ocupando `area` dela, ou de volta para a do app (`None`), no último lugar pedido lá.
    pub fn set_video_host(&self, host: Option<(isize, Bounds)>) {
        let mut s = lock(&self.state.surface);
        s.host = host;
        s.apply();
    }

    /// Solta o vídeo numa janela pequena, sempre no topo (picture in picture), ou devolve ao app.
    pub fn set_video_pip(&self, pip: bool) {
        let mut s = lock(&self.state.surface);
        s.pip = pip;
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

    /// Volume do som do amigo, de 0 a 2 (200%).
    pub fn set_volume(&self, volume: f32) {
        if let Some(audio) = lock(&self.state.audio).as_ref() {
            audio.set_volume(volume);
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
        // Paramos de assistir: quem recebia repassado por nós cai (e procura outro caminho).
        self.state.relay.close();
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
        // Repasse: só copia quando alguém recebe por nós (sem ninguém, nada muda no caminho).
        if state.relay.viewer_count() > 0 {
            state.relay.send_video(&VideoFrame {
                header,
                data: Arc::from(&data[..]),
            });
        }
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
        if self.state.relay.viewer_count() > 0 {
            self.state.relay.send_audio(seq, timestamp_us, peak, opus);
        }
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    use super::*;

    type Seen = Arc<Mutex<Vec<StreamStats>>>;

    /// O estado de quem assiste, sem decoder, janela nem saída de som.
    fn state() -> (Arc<Shared>, Seen, Arc<Mutex<Vec<String>>>) {
        let stats: Seen = Arc::default();
        let closed: Arc<Mutex<Vec<String>>> = Arc::default();
        let (s, c) = (stats.clone(), closed.clone());
        let desc = StreamDescription {
            width: 1920,
            height: 1080,
            fps: 60,
            audio: true,
        };
        let state = Shared::new(
            &desc,
            "Ana",
            Box::new(move |st| lock(&s).push(st)),
            Box::new(move |reason| lock(&c).push(reason)),
        );
        (state, stats, closed)
    }

    fn header(seq: u32) -> FrameHeader {
        FrameHeader {
            seq,
            timestamp_us: clock::now_us(),
            keyframe: seq == 0,
            encode_us: 3_000,
        }
    }

    /// Passa da janela de 500 ms das estatísticas.
    fn next_window() {
        std::thread::sleep(Duration::from_micros(StatsMeter::WINDOW_US + 30_000));
    }

    #[test]
    fn estatisticas_de_quem_assiste_trazem_o_formato_e_quem_transmite() {
        let (state, stats, _) = state();
        let rtt = Arc::new(AtomicU64::new(25_000));
        let _ = state.rtt_us.set(rtt);
        let mut sink = Sink { state: state.clone() };
        sink.video(header(0), vec![0; 10_000]);
        assert!(lock(&stats).is_empty(), "a janela ainda não fechou");
        sink.audio(0, 0, 0.5, &[0; 200]);
        state.on_viewer_event(ViewerEvent::Presented { latency_us: 40_000 });
        next_window();
        sink.video(header(1), vec![0; 10_000]);
        let got = lock(&stats).clone();
        assert_eq!(got.len(), 1);
        let s = &got[0];
        assert_eq!((s.width, s.height), (1920, 1080));
        assert_eq!(s.encoder, "transmitido por Ana");
        assert!(s.audio_active);
        assert_eq!(s.rtt_ms, 25.0);
        assert_eq!(s.encode_ms, 3.0);
        assert_eq!(s.latency_ms, 40.0);
        assert_eq!(s.audio_level, 0.5);
        assert!(s.bitrate_kbps > 0 && s.audio_kbps > 0);
    }

    #[test]
    fn sem_medida_da_conexao_a_ida_e_volta_fica_em_zero() {
        let (state, stats, _) = state();
        let mut sink = Sink { state };
        sink.video(header(0), vec![1; 100]);
        next_window();
        sink.video(header(1), vec![1; 100]);
        assert_eq!(lock(&stats)[0].rtt_ms, 0.0);
    }

    #[test]
    fn som_perdido_e_falha_na_saida_aparecem_nas_estatisticas() {
        let (state, stats, _) = state();
        state.on_audio_event(ReceiverEvent::Lost);
        state.on_audio_event(ReceiverEvent::Lost);
        state.on_audio_event(ReceiverEvent::PlaybackFailed("sem fone".into()));
        // Pedir keyframe sem conexão, janela fechada ou falha: não derruba nada.
        state.on_viewer_event(ViewerEvent::NeedKeyframe);
        state.on_viewer_event(ViewerEvent::Closed);
        state.on_viewer_event(ViewerEvent::Failed("x".into()));
        let mut sink = Sink { state };
        sink.video(header(0), vec![1; 100]);
        next_window();
        sink.video(header(1), vec![1; 100]);
        let s = &lock(&stats)[0];
        assert_eq!(s.audio_lost, 2);
        assert_eq!(s.audio_error.as_deref(), Some("não deu para tocar o som: sem fone"));
    }

    #[test]
    fn som_recebido_vai_para_o_receptor_de_audio() {
        let (state, _, _) = state();
        *lock(&state.audio) = Some(AudioReceiver::start(false, |_| {}).unwrap());
        let mut sink = Sink { state: state.clone() };
        for seq in 0..5 {
            sink.audio(seq, u64::from(seq), 0.1, &[0xF8, 0xFF, 0xFE]);
        }
        assert!(lock(&state.audio).is_some());
    }

    #[test]
    fn fim_da_conexao_avisa_uma_vez_so() {
        let (state, _, closed) = state();
        let mut sink = Sink { state };
        sink.closed("quem transmite parou".into());
        sink.closed("de novo".into());
        assert_eq!(*lock(&closed), ["quem transmite parou"]);
    }
}
