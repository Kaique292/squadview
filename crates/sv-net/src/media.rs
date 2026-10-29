//! Mídia sobre uma conexão QUIC já aberta (ver ADR 0008).
//!
//! - [`Broadcaster`]: quem transmite manda cada frame para todos os espectadores conectados.
//! - [`Receiver`]: quem assiste recebe frames, áudio e mantém a estimativa do relógio.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::{Connection, RecvStream};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use crate::clock::ClockEstimator;
use crate::protocol::{
    Control, FrameHeader, MAX_FRAME_BYTES, MAX_HEADER_BYTES, decode_audio, encode_audio, frame_message,
};
use crate::simulcast::{BacklogMeter, Layer, LayerChooser};
use crate::stripes::{Assembled, DATA_MASK, PARITY, Reassembler, STRIPES, Splitter, StripeHeader};

/// Um frame de vídeo codificado, compartilhado entre os espectadores sem copiar.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub header: FrameHeader,
    pub data: Arc<[u8]>,
    /// De qual camada do simulcast ele é (não vai para a rede: cada espectador recebe uma só).
    pub layer: Layer,
}

/// Pede um keyframe a uma camada de quem transmite.
pub type OnKeyframe = Arc<dyn Fn(Layer) + Send + Sync>;

/// Um frame cortado em faixas (ADR 0028), compartilhado por todos os filhos diretos da malha.
#[derive(Debug)]
pub struct StripedFrame {
    pub frame: FrameHeader,
    pub frame_len: u32,
    pub layer: Layer,
    pub stripes: [Arc<[u8]>; STRIPES],
}

impl StripedFrame {
    fn header(&self, index: u8) -> StripeHeader {
        StripeHeader {
            frame: self.frame,
            layer: self.layer as u8,
            index,
            frame_len: self.frame_len,
        }
    }
}

/// O que vai para a fila de um espectador: o frame inteiro ou, na malha, as faixas.
#[derive(Debug, Clone)]
enum Outgoing {
    Frame(VideoFrame),
    Stripes(Arc<StripedFrame>),
}

impl Outgoing {
    fn seq(&self) -> u32 {
        match self {
            Self::Frame(f) => f.header.seq,
            Self::Stripes(s) => s.frame.seq,
        }
    }

    fn keyframe(&self) -> bool {
        match self {
            Self::Frame(f) => f.header.keyframe,
            Self::Stripes(s) => s.frame.keyframe,
        }
    }

    fn layer(&self) -> Layer {
        match self {
            Self::Frame(f) => f.layer,
            Self::Stripes(s) => s.layer,
        }
    }

    fn timestamp_us(&self) -> u64 {
        match self {
            Self::Frame(f) => f.header.timestamp_us,
            Self::Stripes(s) => s.frame.timestamp_us,
        }
    }
}

/// Quais faixas quem transmite manda direto a um membro da malha (um bit por faixa). O
/// coordenador muda isso a cada plano, sem tirar o espectador da lista.
pub type StripeMask = Arc<AtomicU8>;

/// Frames na fila de cada espectador. Se ele não der conta (internet fraca), os mais novos são
/// descartados e pedimos um keyframe quando a fila esvaziar.
const VIEWER_QUEUE: usize = 6;

struct ViewerSlot {
    conn: Connection,
    video: mpsc::Sender<Outgoing>,
    /// Na malha (ADR 0030): as faixas que vão direto (o resto vem de outros membros).
    mesh: Option<StripeMask>,
    /// Qual camada este espectador recebe (ADR 0026).
    chooser: LayerChooser,
    /// A camada ativa, para o stream de controle pedir o keyframe certo.
    active: Arc<AtomicU8>,
    /// A conexão está com vídeo preso (a rede não leva o que escrevemos, ver [`BacklogMeter`]).
    congested: Arc<AtomicBool>,
    on_keyframe: OnKeyframe,
}

/// Distribui o stream para os espectadores conectados.
#[derive(Clone, Default)]
pub struct Broadcaster {
    viewers: Arc<Mutex<Vec<ViewerSlot>>>,
    audio_buf: Arc<Mutex<Vec<u8>>>,
    /// Algum espectador precisa da camada baixa do simulcast.
    wants_low: Arc<AtomicBool>,
    /// Corta os frames para os membros da malha (só a thread do encoder principal usa).
    splitter: Arc<Mutex<Splitter>>,
    /// O mesmo, para a camada baixa (a thread do outro encoder).
    splitter_low: Arc<Mutex<Splitter>>,
}

impl std::fmt::Debug for Broadcaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broadcaster")
            .field("viewers", &self.viewer_count())
            .finish()
    }
}

impl Broadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn viewer_count(&self) -> usize {
        self.viewers.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Algum espectador recebe a camada baixa ou está indo para ela: quem transmite liga o
    /// segundo encoder só enquanto isto for verdade.
    pub fn wants_low(&self) -> bool {
        self.wants_low.load(Ordering::Relaxed)
    }

    /// Começa a mandar o stream para `conn`. `on_keyframe` é chamado quando alguém precisa de um
    /// keyframe de uma camada (ao entrar, ao perder frames, ao trocar de camada). Com `simulcast`,
    /// o espectador aceita trocar de camada (ADR 0026); sem, fica sempre na principal. Precisa
    /// rodar dentro de um runtime tokio.
    pub fn add(&self, conn: Connection, simulcast: bool, on_keyframe: OnKeyframe) {
        self.add_slot(conn, simulcast, None, on_keyframe);
    }

    /// Como [`Broadcaster::add`], para um membro da malha (ADR 0030): recebe só as faixas marcadas
    /// em `mask` (as outras chegam por outros membros). Recebendo tudo direto, troca de camada
    /// como qualquer espectador; com pais, fica na principal, que é a que eles repassam.
    pub fn add_mesh(&self, conn: Connection, mask: StripeMask, on_keyframe: OnKeyframe) {
        self.add_slot(conn, true, Some(mask), on_keyframe);
    }

    fn add_slot(&self, conn: Connection, simulcast: bool, mesh: Option<StripeMask>, on_keyframe: OnKeyframe) {
        let (tx, rx) = mpsc::channel(VIEWER_QUEUE);
        let active = Arc::new(AtomicU8::new(Layer::Main as u8));
        let congested = Arc::new(AtomicBool::new(false));
        let mesh_mask = mesh.clone();
        self.viewers.lock().unwrap_or_else(|e| e.into_inner()).push(ViewerSlot {
            conn: conn.clone(),
            video: tx,
            mesh,
            chooser: LayerChooser::new(simulcast),
            active: active.clone(),
            congested: congested.clone(),
            on_keyframe: on_keyframe.clone(),
        });
        on_keyframe(Layer::Main);

        let viewers = self.viewers.clone();
        let wants_low = self.wants_low.clone();
        let video_conn = conn.clone();
        let kf = on_keyframe.clone();
        tokio::spawn(async move {
            // Também termina quando a conexão fecha sem nenhum frame na fila (antes o espectador
            // continuava contado até o próximo frame).
            tokio::select! {
                () = send_video(video_conn.clone(), rx, kf, congested, mesh_mask) => {}
                _ = video_conn.closed() => {}
            }
            // Saiu (fechou a conexão ou caiu): tira da lista.
            let mut list = viewers.lock().unwrap_or_else(|e| e.into_inner());
            list.retain(|v| v.conn.stable_id() != video_conn.stable_id());
            wants_low.store(list.iter().any(|v| v.chooser.needs_low()), Ordering::Relaxed);
        });
        tokio::spawn(serve_control(conn, active, on_keyframe));
    }

    /// Manda um frame para todos os que recebem a camada dele. Não bloqueia: quem estiver com a
    /// fila cheia perde este frame (e, perdendo muitos, desce para a camada baixa).
    pub fn send_video(&self, frame: &VideoFrame) {
        let now = frame.header.timestamp_us;
        // Trocas de camada são raras: o Vec só aloca quando alguma acontece.
        let mut switches: Vec<(OnKeyframe, Layer)> = Vec::new();
        let mut viewers = self.viewers.lock().unwrap_or_else(|e| e.into_inner());
        let mut wants_low = false;
        // Na malha, o frame é cortado uma vez só e as faixas vão para todos os filhos diretos.
        let striped = viewers
            .iter()
            .any(|v| v.mesh.is_some())
            .then(|| Arc::new(self.split(frame)));
        for v in viewers.iter_mut() {
            // Com pais na malha, só a principal: é a que eles repassam.
            let main_only = v.mesh.as_ref().is_some_and(|m| m.load(Ordering::Relaxed) != DATA_MASK);
            if main_only && let Some(layer) = v.chooser.force_main(now) {
                switches.push((v.on_keyframe.clone(), layer));
            }
            if v.chooser.accepts(frame.layer, frame.header.keyframe, now) {
                v.active.store(v.chooser.active() as u8, Ordering::Relaxed);
                let item = match (&v.mesh, &striped) {
                    (Some(_), Some(s)) => Outgoing::Stripes(s.clone()),
                    (Some(_), None) => continue,
                    (None, _) => Outgoing::Frame(frame.clone()),
                };
                // Rede atrasada conta como frame perdido: sem isso, o QUIC guardaria segundos de
                // vídeo antes de a fila do espectador encher.
                let delivered = v.video.try_send(item).is_ok() && !v.congested.load(Ordering::Relaxed);
                if let Some(layer) = v.chooser.record(delivered, now).filter(|_| !main_only) {
                    switches.push((v.on_keyframe.clone(), layer));
                }
            }
            wants_low |= v.chooser.needs_low();
        }
        drop(viewers);
        self.wants_low.store(wants_low, Ordering::Relaxed);
        // Fora da trava: pedir o keyframe chega no encoder.
        for (on_keyframe, layer) in switches {
            on_keyframe(layer);
        }
    }

    fn split(&self, frame: &VideoFrame) -> StripedFrame {
        let splitter = match frame.layer {
            Layer::Main => &self.splitter,
            Layer::Low => &self.splitter_low,
        };
        let mut splitter = splitter.lock().unwrap_or_else(|e| e.into_inner());
        let parts = splitter.split(&frame.data);
        StripedFrame {
            frame: frame.header,
            frame_len: frame.data.len() as u32,
            layer: frame.layer,
            stripes: std::array::from_fn(|i| Arc::from(&parts[i][..])),
        }
    }

    /// Manda um pacote de áudio (datagrama: sem retransmissão; perda vira PLC do outro lado).
    pub fn send_audio(&self, seq: u32, timestamp_us: u64, peak: f32, opus: &[u8]) {
        let mut buf = self.audio_buf.lock().unwrap_or_else(|e| e.into_inner());
        encode_audio(seq, timestamp_us, peak, opus, &mut buf);
        let bytes = bytes::Bytes::copy_from_slice(&buf);
        for v in self.viewers.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            let _ = v.conn.send_datagram(bytes.clone());
        }
    }

    /// Desconecta todo mundo (parou de transmitir).
    pub fn close(&self) {
        for v in self.viewers.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            v.conn.close(quinn::VarInt::from_u32(0), b"fim da transmissao");
        }
    }
}

async fn send_video(
    conn: Connection,
    mut rx: mpsc::Receiver<Outgoing>,
    on_keyframe: OnKeyframe,
    congested: Arc<AtomicBool>,
    mesh: Option<StripeMask>,
) {
    let mut backlog = BacklogMeter::new();
    let mut last_seq: Option<u32> = None;
    let mut waiting_keyframe = false;
    while let Some(frame) = rx.recv().await {
        // Pulou frame (fila cheia): os seguintes não decodificam até chegar um keyframe.
        if last_seq.is_some_and(|s| frame.seq() != s.wrapping_add(1)) && !frame.keyframe() {
            if !waiting_keyframe {
                waiting_keyframe = true;
                on_keyframe(frame.layer());
            }
            last_seq = Some(frame.seq());
            continue;
        }
        if frame.keyframe() {
            waiting_keyframe = false;
        }
        last_seq = Some(frame.seq());
        if waiting_keyframe {
            continue;
        }
        let written = match &frame {
            Outgoing::Frame(f) => {
                let Ok(mut stream) = conn.open_uni().await else { return };
                if stream.write_all(&frame_message(&f.header)).await.is_err()
                    || stream.write_all(&f.data).await.is_err()
                {
                    return;
                }
                let _ = stream.finish();
                f.data.len()
            }
            Outgoing::Stripes(s) => {
                let mask = mesh.as_ref().map_or(0, |m| m.load(Ordering::Relaxed));
                let mut written = 0;
                for i in 0..STRIPES as u8 {
                    if mask & (1 << i) == 0 {
                        continue;
                    }
                    if send_stripe(&conn, &s.header(i), &s.stripes[usize::from(i)])
                        .await
                        .is_err()
                    {
                        return;
                    }
                    written += s.stripes[usize::from(i)].len();
                }
                written
            }
        };
        let late = backlog.sample(written as u64, conn.stats().udp_tx.bytes, frame.timestamp_us());
        congested.store(late, Ordering::Relaxed);
    }
}

async fn serve_control(conn: Connection, active: Arc<AtomicU8>, on_keyframe: OnKeyframe) {
    let Ok((mut send, mut recv)) = conn.accept_bi().await else {
        return;
    };
    while let Some(msg) = read_message::<Control>(&mut recv).await {
        match msg {
            Control::KeyframeRequest => on_keyframe(Layer::from_u8(active.load(Ordering::Relaxed))),
            Control::Ping { sent_us } => {
                let pong = Control::Pong {
                    sent_us,
                    remote_us: now_us(),
                };
                if send.write_all(&frame_message(&pong)).await.is_err() {
                    return;
                }
            }
            Control::Pong { .. } => {}
        }
    }
}

/// O que chega para quem assiste.
pub trait ReceiverSink: Send + 'static {
    fn video(&mut self, frame: FrameHeader, data: Vec<u8>);
    fn audio(&mut self, seq: u32, timestamp_us: u64, peak: f32, opus: &[u8]);
    /// A conexão acabou (quem transmite parou ou a rede caiu).
    fn closed(&mut self, reason: String);
}

/// Pede keyframes a quem transmite, de qualquer thread.
#[derive(Debug, Clone)]
pub struct KeyframeRequester(mpsc::UnboundedSender<Control>);

impl KeyframeRequester {
    pub fn request(&self) {
        let _ = self.0.send(Control::KeyframeRequest);
    }
}

/// Recebe um stream. Para quando é descartado ou quando a conexão fecha.
#[derive(Debug)]
pub struct Receiver {
    conn: Connection,
    control: mpsc::UnboundedSender<Control>,
    clock_offset: Arc<AtomicI64>,
    rtt_us: Arc<AtomicU64>,
}

impl Receiver {
    /// Precisa rodar dentro de um runtime tokio.
    pub fn start(conn: Connection, sink: impl ReceiverSink) -> Self {
        Self::start_with(conn, sink, true)
    }

    /// Como [`Receiver::start`], sem ler o vídeo desta conexão: na malha (ADR 0030), os streams
    /// de vídeo dela trazem faixas, lidas por um [`StripeReceiver`]. O som, o relógio e os pedidos
    /// de keyframe continuam aqui.
    pub fn start_without_video(conn: Connection, sink: impl ReceiverSink) -> Self {
        Self::start_with(conn, sink, false)
    }

    fn start_with(conn: Connection, sink: impl ReceiverSink, video: bool) -> Self {
        let sink = Arc::new(Mutex::new(sink));
        let clock_offset = Arc::new(AtomicI64::new(0));
        let rtt_us = Arc::new(AtomicU64::new(0));
        let (control_tx, control_rx) = mpsc::unbounded_channel();

        if video {
            tokio::spawn(receive_video(conn.clone(), sink.clone()));
        }
        tokio::spawn(receive_audio(conn.clone(), sink.clone()));
        tokio::spawn(run_control(
            conn.clone(),
            control_rx,
            clock_offset.clone(),
            rtt_us.clone(),
        ));

        let ping = control_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                if ping.send(Control::Ping { sent_us: now_us() }).is_err() {
                    return;
                }
            }
        });

        let closed_conn = conn.clone();
        tokio::spawn(async move {
            let reason = closed_conn.closed().await;
            sink.lock()
                .unwrap_or_else(|e| e.into_inner())
                .closed(reason.to_string());
        });

        Self {
            conn,
            control: control_tx,
            clock_offset,
            rtt_us,
        }
    }

    pub fn request_keyframe(&self) {
        let _ = self.control.send(Control::KeyframeRequest);
    }

    pub fn keyframe_requester(&self) -> KeyframeRequester {
        KeyframeRequester(self.control.clone())
    }

    /// Quanto somar a um timestamp de quem transmite para trazê-lo ao nosso relógio (µs).
    pub fn clock_offset(&self) -> Arc<AtomicI64> {
        self.clock_offset.clone()
    }

    /// Ida e volta até quem transmite (µs), 0 até a primeira medição.
    pub fn rtt_us(&self) -> u64 {
        self.rtt_us.load(Ordering::Relaxed)
    }

    /// A mesma medição, compartilhada (atualiza sozinha).
    pub fn rtt_handle(&self) -> Arc<AtomicU64> {
        self.rtt_us.clone()
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.conn.close(quinn::VarInt::from_u32(0), b"parou de assistir");
    }
}

async fn receive_video(conn: Connection, sink: Arc<Mutex<impl ReceiverSink>>) {
    // Um frame por vez, na ordem em que os streams abriram: o decoder precisa da ordem certa.
    while let Ok(mut stream) = conn.accept_uni().await {
        let Some(header) = read_message::<FrameHeader>(&mut stream).await else {
            continue;
        };
        let Ok(data) = stream.read_to_end(MAX_FRAME_BYTES).await else {
            continue;
        };
        sink.lock().unwrap_or_else(|e| e.into_inner()).video(header, data);
    }
}

async fn receive_audio(conn: Connection, sink: Arc<Mutex<impl ReceiverSink>>) {
    while let Ok(datagram) = conn.read_datagram().await {
        if let Some((seq, ts, peak, opus)) = decode_audio(&datagram) {
            sink.lock()
                .unwrap_or_else(|e| e.into_inner())
                .audio(seq, ts, peak, opus);
        }
    }
}

async fn run_control(
    conn: Connection,
    mut outgoing: mpsc::UnboundedReceiver<Control>,
    clock_offset: Arc<AtomicI64>,
    rtt_us: Arc<AtomicU64>,
) {
    let Ok((mut send, mut recv)) = conn.open_bi().await else {
        return;
    };
    let reader = tokio::spawn(async move {
        let mut estimator = ClockEstimator::new();
        while let Some(msg) = read_message::<Control>(&mut recv).await {
            if let Control::Pong { sent_us, remote_us } = msg {
                estimator.record(sent_us, remote_us, now_us());
                if let Some(offset) = estimator.to_local_offset() {
                    clock_offset.store(offset, Ordering::Relaxed);
                }
                if let Some(rtt) = estimator.rtt_us() {
                    rtt_us.store(rtt, Ordering::Relaxed);
                }
            }
        }
    });
    while let Some(msg) = outgoing.recv().await {
        if send.write_all(&frame_message(&msg)).await.is_err() {
            break;
        }
    }
    reader.abort();
}

/// Manda uma faixa (ADR 0028) num stream próprio: o QUIC retransmite o que se perder no caminho até
/// quem recebe (o "NACK" salto a salto), e a paridade cobre a faixa de quem saiu.
pub async fn send_stripe(conn: &Connection, header: &StripeHeader, payload: &[u8]) -> Result<(), quinn::WriteError> {
    let mut stream = conn.open_uni().await.map_err(quinn::WriteError::ConnectionLost)?;
    stream.write_all(&frame_message(header)).await?;
    stream.write_all(payload).await?;
    let _ = stream.finish();
    Ok(())
}

/// O que sai da remontagem das faixas.
pub trait StripeSink: Send + 'static {
    /// Um frame inteiro, na ordem.
    fn frame(&mut self, header: FrameHeader, data: Vec<u8>);
    /// Estes frames não chegaram a tempo: o decoder precisa de um keyframe.
    fn lost(&mut self, first_seq: u32, count: u32);
}

/// Recebe as faixas de um stream por várias conexões (uma por pai, em cada árvore da sala) e
/// entrega os frames remontados. Uma conexão que cai só tira uma faixa: com as outras 4, os frames
/// continuam chegando.
#[derive(Clone)]
pub struct StripeReceiver {
    inner: Arc<Mutex<StripeState>>,
    /// Quando chegou a última faixa, de qualquer pai (µs, no nosso relógio).
    last_any_us: Arc<AtomicU64>,
}

struct StripeState {
    reassembler: Reassembler,
    /// A camada que estamos remontando (muda no keyframe de outra, com o simulcast).
    layer: u8,
    sink: Box<dyn StripeSink>,
    /// A faixa da casa: chegando, segue para os filhos (ADR 0030).
    home: Option<u8>,
    relay: StripeRelay,
}

/// Sem nenhuma faixa de um pai por este tempo, enquanto as outras chegam, ele é dado como perdido.
const PARENT_LOST_US: u64 = 300_000;
/// De quanto em quanto tempo o vigia confere os pais.
const PARENT_CHECK: Duration = Duration::from_millis(100);

/// Os filhos de um membro na malha: cada faixa da casa que chega é copiada para eles, cada um com
/// a sua fila (quem não dá conta perde faixas sem atrasar os outros).
#[derive(Clone, Default)]
pub struct StripeRelay {
    children: Arc<Mutex<Vec<Child>>>,
}

/// Um filho na malha: a conexão e a fila de faixas para ele.
type Child = (Connection, mpsc::Sender<(StripeHeader, Arc<[u8]>)>);

impl std::fmt::Debug for StripeRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeRelay").field("children", &self.count()).finish()
    }
}

impl StripeRelay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.children.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Passa a repassar para `conn` (um filho que conectou com o ALPN da malha). Precisa rodar
    /// dentro de um runtime tokio.
    pub fn add(&self, conn: Connection) {
        let (tx, mut rx) = mpsc::channel::<(StripeHeader, Arc<[u8]>)>(VIEWER_QUEUE);
        self.children
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((conn.clone(), tx));
        let children = self.children.clone();
        tokio::spawn(async move {
            let send = async {
                while let Some((header, data)) = rx.recv().await {
                    if send_stripe(&conn, &header, &data).await.is_err() {
                        return;
                    }
                }
            };
            tokio::select! {
                () = send => {}
                _ = conn.closed() => {}
            }
            children
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(c, _)| c.stable_id() != conn.stable_id());
        });
    }

    /// Copia uma faixa para todos os filhos (sem esperar ninguém).
    pub fn forward(&self, header: &StripeHeader, payload: &[u8]) {
        let children = self.children.lock().unwrap_or_else(|e| e.into_inner());
        if children.is_empty() {
            return;
        }
        let data: Arc<[u8]> = Arc::from(payload);
        for (_, tx) in children.iter() {
            let _ = tx.try_send((*header, data.clone()));
        }
    }

    /// Fecha as conexões com os filhos (paramos de assistir ou a casa mudou).
    pub fn close(&self) {
        for (conn, _) in self.children.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            conn.close(quinn::VarInt::from_u32(0), b"fim do repasse");
        }
    }
}

impl std::fmt::Debug for StripeReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeReceiver").finish_non_exhaustive()
    }
}

impl StripeReceiver {
    pub fn new(sink: impl StripeSink) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StripeState {
                reassembler: Reassembler::new(),
                layer: Layer::Main as u8,
                sink: Box::new(sink),
                home: None,
                relay: StripeRelay::new(),
            })),
            last_any_us: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Passa a ler faixas de `conn` (até ela fechar). Precisa rodar dentro de um runtime tokio.
    pub fn attach(&self, conn: Connection) {
        let last = Arc::new(AtomicU64::new(0));
        tokio::spawn(receive_stripes(
            conn,
            self.inner.clone(),
            self.last_any_us.clone(),
            last,
        ));
    }

    /// Como [`StripeReceiver::attach`], para um pai da malha: se a conexão fechar, ou passar
    /// [`PARENT_LOST_US`] sem faixa dele enquanto as outras chegam, `on_lost` é chamado (uma vez).
    pub fn attach_parent(&self, conn: Connection, on_lost: impl FnOnce() + Send + 'static) {
        let last = Arc::new(AtomicU64::new(now_us()));
        tokio::spawn(receive_stripes(
            conn.clone(),
            self.inner.clone(),
            self.last_any_us.clone(),
            last.clone(),
        ));
        let last_any = self.last_any_us.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = conn.closed() => break,
                    () = tokio::time::sleep(PARENT_CHECK) => {}
                }
                if parent_lost(last.load(Ordering::Relaxed), last_any.load(Ordering::Relaxed)) {
                    conn.close(quinn::VarInt::from_u32(0), b"ficou para tras");
                    break;
                }
            }
            on_lost();
        });
    }

    /// A faixa da casa e para quem repassá-la.
    pub fn set_home(&self, home: Option<u8>, relay: StripeRelay) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.home = home.filter(|h| *h <= PARITY);
        state.relay = relay;
    }
}

/// O pai ficou para trás: a última faixa dele é velha perto da última que chegou de qualquer pai.
/// Com a tela parada (nenhuma faixa de ninguém), ninguém é dado como perdido.
fn parent_lost(parent_last_us: u64, any_last_us: u64) -> bool {
    any_last_us.saturating_sub(parent_last_us) > PARENT_LOST_US
}

async fn receive_stripes(
    conn: Connection,
    inner: Arc<Mutex<StripeState>>,
    last_any_us: Arc<AtomicU64>,
    last_us: Arc<AtomicU64>,
) {
    while let Ok(mut stream) = conn.accept_uni().await {
        let Some(header) = read_message::<StripeHeader>(&mut stream).await else {
            continue;
        };
        let len = header.stripe_len();
        if header.frame_len > crate::stripes::MAX_FRAME_BYTES {
            continue;
        }
        let mut buf = inner.lock().unwrap_or_else(|e| e.into_inner()).reassembler.buffer();
        buf.resize(len, 0);
        if stream.read_exact(&mut buf).await.is_err() {
            continue;
        }
        let now = now_us();
        last_us.store(now, Ordering::Relaxed);
        last_any_us.store(now, Ordering::Relaxed);
        let mut guard = inner.lock().unwrap_or_else(|e| e.into_inner());
        let state = &mut *guard;
        // Outra camada (quem transmite trocou a nossa): recomeça no keyframe dela; antes disso,
        // as faixas dela não servem para nada.
        if header.layer != state.layer {
            if !header.frame.keyframe {
                continue;
            }
            state.reassembler = Reassembler::new();
            state.layer = header.layer;
        }
        if state.home == Some(header.index) {
            state.relay.forward(&header, &buf);
        }
        let sink = &mut state.sink;
        state
            .reassembler
            .push(header, buf, now, &mut |assembled| match assembled {
                Assembled::Frame(h, data) => sink.frame(h, data),
                Assembled::Lost { first_seq, count } => sink.lost(first_seq, count),
            });
    }
}

/// Lê uma mensagem com prefixo de tamanho; `None` se o stream acabou ou veio algo inválido.
async fn read_message<T: DeserializeOwned>(stream: &mut RecvStream) -> Option<T> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len).await.ok()?;
    let len = u16::from_be_bytes(len) as usize;
    if len > MAX_HEADER_BYTES {
        return None;
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.ok()?;
    postcard::from_bytes(&body).ok()
}

/// Relógio desta máquina em µs: o mesmo dos timestamps da captura, para a latência fechar a conta.
fn now_us() -> u64 {
    sv_gpu::clock::now_us()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pai_que_ficou_para_tras_e_dado_como_perdido_mas_tela_parada_nao() {
        // As outras faixas chegam e a deste pai parou há mais de 300 ms.
        assert!(parent_lost(1_000_000, 1_400_000));
        // Atrasou um pouco: ainda não.
        assert!(!parent_lost(1_000_000, 1_200_000));
        // Ninguém manda nada (tela parada): não é culpa do pai.
        assert!(!parent_lost(1_000_000, 1_000_000));
        assert!(!parent_lost(1_000_000, 0));
    }
}
