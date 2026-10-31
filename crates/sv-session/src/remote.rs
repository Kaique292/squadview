//! Assistir o stream de um amigo: conexão QUIC → decoder por GPU → vídeo dentro do app, e o som
//! do jogo dele pelo buffer de jitter.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use quinn::Connection;
use sv_encode::EncodedFrame;
use sv_gpu::{Size, clock};
use sv_media::{Bounds, WindowTarget};
use sv_net::protocol::{FrameHeader, StreamDescription};
use sv_net::{
    Broadcaster, KeyframeRequester, Layer, Receiver, ReceiverSink, StripeReceiver, StripeRelay, StripeSink, VideoFrame,
};

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

    /// Pede um keyframe a quem transmite e conta o pedido nas estatísticas (e no registro).
    fn request_keyframe(&self) {
        lock(&self.meter).record_keyframe_request();
        if let Some(k) = self.keyframes.get() {
            k.request();
        }
    }

    fn on_viewer_event(&self, event: ViewerEvent) {
        match event {
            ViewerEvent::Presented { latency_us } => lock(&self.meter).record_presented(latency_us),
            ViewerEvent::NeedKeyframe => self.request_keyframe(),
            // Trocou de camada: as estatísticas (e o selo de qualidade) mostram o que chega de fato.
            ViewerEvent::Resized(size) => {
                let mut base = lock(&self.base);
                base.width = size.width;
                base.height = size.height;
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
    /// Na malha (ADR 0030): as faixas que chegam de quem transmite e dos pais, e os filhos.
    mesh: Option<MeshEndpoint>,
}

/// A nossa ponta na malha de uma live: onde chegam as faixas (de quem transmite e dos pais) e para
/// quem repassamos a faixa da casa.
#[derive(Clone, Debug)]
pub struct MeshEndpoint {
    pub stripes: StripeReceiver,
    pub relay: StripeRelay,
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
    /// A live veio pela malha: a sala liga os pais e os filhos aqui (ADR 0030).
    pub mesh: Option<MeshEndpoint>,
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
    /// Começa a assistir pela conexão `conn` (já aberta). Com `mesh`, quem transmite aceitou a
    /// gente na malha (ADR 0030): o vídeo chega em faixas, por esta conexão e pelos pais que a sala
    /// ligar. Precisa ser chamado dentro do runtime tokio da sala (`Room::runtime`).
    pub fn start(
        conn: Connection,
        desc: StreamDescription,
        streamer_name: &str,
        target: WindowTarget,
        mesh: bool,
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

        let (receiver, mesh) = if mesh {
            let stripes = StripeReceiver::new(MeshSink { state: state.clone() });
            stripes.attach(conn.clone());
            let receiver = Receiver::start_without_video(conn, Sink { state: state.clone() });
            (
                receiver,
                Some(MeshEndpoint {
                    stripes,
                    relay: StripeRelay::new(),
                }),
            )
        } else {
            (Receiver::start(conn, Sink { state: state.clone() }), None)
        };
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
        Self {
            receiver,
            state,
            desc,
            mesh,
        }
    }

    /// Para repassar este stream pela sala: quem pedir recebe os mesmos frames, e os pedidos de
    /// keyframe deles sobem até quem transmite (`streamer` é o id dele).
    pub fn relay_source(&self, streamer: &str) -> RelaySource {
        RelaySource {
            streamer: streamer.to_owned(),
            desc: self.desc,
            broadcaster: self.state.relay.clone(),
            keyframes: self.receiver.keyframe_requester(),
            mesh: self.mesh.clone(),
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
        if let Some(mesh) = &self.mesh {
            mesh.relay.close();
        }
        lock(&self.state.viewer).take();
        lock(&self.state.audio).take();
    }
}

struct Sink {
    state: Arc<Shared>,
}

/// Os frames remontados das faixas da malha seguem o mesmo caminho dos frames inteiros.
struct MeshSink {
    state: Arc<Shared>,
}

impl StripeSink for MeshSink {
    fn frame(&mut self, header: FrameHeader, data: Vec<u8>) {
        on_video(&self.state, header, data);
    }

    fn lost(&mut self, _first_seq: u32, _count: u32) {
        // Faltou mais de uma faixa: o decoder perdeu a cadeia de referências.
        self.state.request_keyframe();
    }
}

impl ReceiverSink for Sink {
    fn video(&mut self, header: FrameHeader, data: Vec<u8>) {
        on_video(&self.state, header, data);
    }

    fn audio(&mut self, seq: u32, timestamp_us: u64, peak: f32, opus: &[u8]) {
        on_audio(&self.state, seq, timestamp_us, peak, opus);
    }

    fn closed(&mut self, reason: String) {
        if let Some(cb) = lock(&self.state.on_closed).take() {
            cb(reason);
        }
    }
}

/// Um frame chegou (inteiro ou remontado das faixas): repasse, decoder e estatísticas.
fn on_video(state: &Arc<Shared>, header: FrameHeader, data: Vec<u8>) {
    lock(&state.meter).record_encoded(data.len(), header.encode_us);
    // Repasse: só copia quando alguém recebe por nós (sem ninguém, nada muda no caminho).
    if state.relay.viewer_count() > 0 {
        state.relay.send_video(&VideoFrame {
            header,
            data: Arc::from(&data[..]),
            // Para quem recebe por nós, o que chega é "a" camada (sem troca no repasse).
            layer: Layer::Main,
        });
    }
    let frame = EncodedFrame {
        data,
        keyframe: header.keyframe,
        timestamp_us: header.timestamp_us,
        encode_us: header.encode_us,
    };
    let accepted = lock(&state.viewer).as_ref().is_none_or(|v| v.submit(frame));
    if !accepted {
        // Fila do decoder cheia: o próximo frame não vai decodificar sem um keyframe.
        state.request_keyframe();
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

fn on_audio(state: &Arc<Shared>, seq: u32, timestamp_us: u64, peak: f32, opus: &[u8]) {
    lock(&state.meter).record_audio(opus.len(), peak);
    if state.relay.viewer_count() > 0 {
        state.relay.send_audio(seq, timestamp_us, peak, opus);
    }
    if let Some(audio) = lock(&state.audio).as_ref() {
        audio.submit(AudioPacket {
            seq,
            data: opus.to_vec(),
            peak,
            timestamp_us,
        });
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
    fn trocar_de_camada_muda_o_tamanho_nas_estatisticas() {
        let (state, stats, _) = state();
        state.on_viewer_event(ViewerEvent::Resized(Size::new(1280, 720)));
        let mut sink = Sink { state };
        sink.video(header(0), vec![1; 100]);
        next_window();
        sink.video(header(1), vec![1; 100]);
        let s = lock(&stats)[0].clone();
        assert_eq!((s.width, s.height), (1280, 720));
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
        assert_eq!(s.keyframe_requests, 1, "o pedido de keyframe vai para o registro");
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
