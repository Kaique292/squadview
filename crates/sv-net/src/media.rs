//! Mídia sobre uma conexão QUIC já aberta (ver ADR 0008).
//!
//! - [`Broadcaster`]: quem transmite manda cada frame para todos os espectadores conectados.
//! - [`Receiver`]: quem assiste recebe frames, áudio e mantém a estimativa do relógio.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::{Connection, RecvStream};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use crate::clock::ClockEstimator;
use crate::protocol::{
    Control, FrameHeader, MAX_FRAME_BYTES, MAX_HEADER_BYTES, decode_audio, encode_audio, frame_message,
};

/// Um frame de vídeo codificado, compartilhado entre os espectadores sem copiar.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub header: FrameHeader,
    pub data: Arc<[u8]>,
}

/// Frames na fila de cada espectador. Se ele não der conta (internet fraca), os mais novos são
/// descartados e pedimos um keyframe quando a fila esvaziar.
const VIEWER_QUEUE: usize = 6;

struct ViewerSlot {
    conn: Connection,
    video: mpsc::Sender<VideoFrame>,
}

/// Distribui o stream para os espectadores conectados.
#[derive(Clone, Default)]
pub struct Broadcaster {
    viewers: Arc<Mutex<Vec<ViewerSlot>>>,
    audio_buf: Arc<Mutex<Vec<u8>>>,
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

    /// Começa a mandar o stream para `conn`. `on_keyframe` é chamado quando alguém precisa de um
    /// keyframe (ao entrar, ao perder frames). Precisa rodar dentro de um runtime tokio.
    pub fn add(&self, conn: Connection, on_keyframe: Arc<dyn Fn() + Send + Sync>) {
        let (tx, rx) = mpsc::channel(VIEWER_QUEUE);
        self.viewers.lock().unwrap_or_else(|e| e.into_inner()).push(ViewerSlot {
            conn: conn.clone(),
            video: tx,
        });
        on_keyframe();

        let viewers = self.viewers.clone();
        let video_conn = conn.clone();
        let kf = on_keyframe.clone();
        tokio::spawn(async move {
            // Também termina quando a conexão fecha sem nenhum frame na fila (antes o espectador
            // continuava contado até o próximo frame).
            tokio::select! {
                () = send_video(video_conn.clone(), rx, kf) => {}
                _ = video_conn.closed() => {}
            }
            // Saiu (fechou a conexão ou caiu): tira da lista.
            viewers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|v| v.conn.stable_id() != video_conn.stable_id());
        });
        tokio::spawn(serve_control(conn, on_keyframe));
    }

    /// Manda um frame para todos. Não bloqueia: quem estiver com a fila cheia perde este frame.
    pub fn send_video(&self, frame: &VideoFrame) {
        for v in self.viewers.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            let _ = v.video.try_send(frame.clone());
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

async fn send_video(conn: Connection, mut rx: mpsc::Receiver<VideoFrame>, on_keyframe: Arc<dyn Fn() + Send + Sync>) {
    let mut last_seq: Option<u32> = None;
    let mut waiting_keyframe = false;
    while let Some(frame) = rx.recv().await {
        // Pulou frame (fila cheia): os seguintes não decodificam até chegar um keyframe.
        if last_seq.is_some_and(|s| frame.header.seq != s.wrapping_add(1)) && !frame.header.keyframe {
            if !waiting_keyframe {
                waiting_keyframe = true;
                on_keyframe();
            }
            last_seq = Some(frame.header.seq);
            continue;
        }
        if frame.header.keyframe {
            waiting_keyframe = false;
        }
        last_seq = Some(frame.header.seq);
        if waiting_keyframe {
            continue;
        }
        let Ok(mut stream) = conn.open_uni().await else { return };
        if stream.write_all(&frame_message(&frame.header)).await.is_err()
            || stream.write_all(&frame.data).await.is_err()
        {
            return;
        }
        let _ = stream.finish();
    }
}

async fn serve_control(conn: Connection, on_keyframe: Arc<dyn Fn() + Send + Sync>) {
    let Ok((mut send, mut recv)) = conn.accept_bi().await else {
        return;
    };
    while let Some(msg) = read_message::<Control>(&mut recv).await {
        match msg {
            Control::KeyframeRequest => on_keyframe(),
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
        let sink = Arc::new(Mutex::new(sink));
        let clock_offset = Arc::new(AtomicI64::new(0));
        let rtt_us = Arc::new(AtomicU64::new(0));
        let (control_tx, control_rx) = mpsc::unbounded_channel();

        tokio::spawn(receive_video(conn.clone(), sink.clone()));
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
