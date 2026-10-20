//! Lado de quem assiste: frames codificados → decoder por GPU → janela de vídeo.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use sv_capture::Thumbnail;
use sv_encode::EncodedFrame;
use sv_gpu::{Gpu, GpuFrame, Size, clock};
use sv_media::{Decoder, VideoWindow, WindowControl, WindowTarget};

use crate::StreamInfo;

/// Frames esperando o decoder. Se encher, a cadeia de referências quebra e pedimos keyframe.
const QUEUE: usize = 16;
/// Maior lado da foto do último quadro (vai para a animação de saída na interface).
pub const SNAPSHOT_MAX: Size = Size::new(1280, 720);

/// Pedido de foto do último quadro: tamanho máximo e por onde responder.
type SnapshotRequest = (Size, SyncSender<Option<Thumbnail>>);

/// Eventos do [`VideoViewer`], na thread dele.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerEvent {
    /// Precisamos de um keyframe para (re)começar a decodificar.
    NeedKeyframe,
    /// Um frame apareceu na tela, `latency_us` depois de capturado.
    Presented {
        latency_us: u32,
    },
    /// O usuário fechou a janela.
    Closed,
    Failed(String),
}

/// Diferença entre o relógio de quem transmite e o nosso, em µs (somada aos timestamps recebidos).
/// No preview local é 0; na rede, `sv-net` estima e atualiza.
pub type ClockOffset = Arc<AtomicI64>;

/// Uma janela assistindo um stream. Fecha quando é descartada.
pub struct VideoViewer {
    frames: Option<SyncSender<EncodedFrame>>,
    snapshots: SyncSender<SnapshotRequest>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for VideoViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoViewer").finish_non_exhaustive()
    }
}

impl VideoViewer {
    /// Abre a superfície de vídeo em `target`. `on_window` recebe o controle dela (posição,
    /// visibilidade) assim que ela existir.
    pub fn start(
        target: WindowTarget,
        info: StreamInfo,
        clock_offset: ClockOffset,
        on_window: impl FnOnce(WindowControl) + Send + 'static,
        mut on_event: impl FnMut(ViewerEvent) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<EncodedFrame>(QUEUE);
        let (snapshots, snapshot_rx) = mpsc::sync_channel::<SnapshotRequest>(1);
        let thread = std::thread::Builder::new()
            .name("sv-viewer".into())
            .spawn(move || {
                if let Err(e) = run(target, &info, &clock_offset, on_window, rx, &snapshot_rx, &mut on_event) {
                    on_event(ViewerEvent::Failed(e.to_string()));
                }
            })
            .expect("não foi possível criar a thread de quem assiste");
        Self {
            frames: Some(tx),
            snapshots,
            thread: Some(thread),
        }
    }

    /// Foto do último quadro mostrado, reduzida para caber em `max`. Uma cópia da GPU para a
    /// CPU, só quando pedida (ao parar o vídeo). `None` se nada foi mostrado ainda ou se a
    /// thread não responder a tempo.
    pub fn snapshot(&self, max: Size, timeout: Duration) -> Option<Thumbnail> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.snapshots.try_send((max, reply)).ok()?;
        answer.recv_timeout(timeout).ok().flatten()
    }

    /// Entrega um frame recebido. Devolve `false` se a fila estava cheia (o frame foi perdido).
    pub fn submit(&self, frame: EncodedFrame) -> bool {
        match self.frames.as_ref().map(|tx| tx.try_send(frame)) {
            Some(Ok(())) => true,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => false,
        }
    }
}

impl Drop for VideoViewer {
    fn drop(&mut self) {
        self.frames = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    target: WindowTarget,
    info: &StreamInfo,
    clock_offset: &ClockOffset,
    on_window: impl FnOnce(WindowControl),
    frames: mpsc::Receiver<EncodedFrame>,
    snapshots: &Receiver<SnapshotRequest>,
    on_event: &mut impl FnMut(ViewerEvent),
) -> windows::core::Result<()> {
    let gpu = Gpu::new()?;
    let window = VideoWindow::open(target, info.size)?;
    on_window(window.control());
    let mut renderer = window.renderer(&gpu)?;
    let mut decoder = Decoder::new(&gpu, info.size, info.fps)?;
    let mut gate = KeyframeGate::new();
    // O último quadro mostrado: só uma referência à textura (AddRef), sem cópia por frame.
    let mut last: Option<GpuFrame> = None;

    loop {
        while let Ok((max, reply)) = snapshots.try_recv() {
            let photo = last.as_ref().and_then(|f| sv_capture::read_frame(&gpu, f, max).ok());
            let _ = reply.try_send(photo);
        }
        if window.is_closed() {
            on_event(ViewerEvent::Closed);
            return Ok(());
        }
        let frame = match frames.recv_timeout(Duration::from_millis(100)) {
            Ok(f) => f,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        match gate.admit(frame.keyframe) {
            Admit::Decode => {}
            Admit::Skip => continue,
            Admit::SkipAndAsk => {
                on_event(ViewerEvent::NeedKeyframe);
                continue;
            }
        }

        let offset = clock_offset.load(Ordering::Relaxed);
        let mut present_error = None;
        let decoded = decoder.decode(&frame.data, frame.timestamp_us, |image| {
            if let Err(e) = renderer.present(image) {
                present_error = Some(e);
            }
            last = Some(image.clone());
            on_event(ViewerEvent::Presented {
                latency_us: latency_us(frame.timestamp_us, offset, clock::now_us()),
            });
        });
        if decoded.is_err() {
            // Frame corrompido ou perdido: limpa o decoder e espera o próximo keyframe.
            decoder.flush();
            gate.decode_failed();
            on_event(ViewerEvent::NeedKeyframe);
        }
        if let Some(e) = present_error {
            return Err(e);
        }
    }
}

/// O que fazer com um frame que chegou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admit {
    Decode,
    /// Sem keyframe ainda: descarta (o keyframe já foi pedido).
    Skip,
    /// Sem keyframe ainda: descarta e pede um (só uma vez por espera).
    SkipAndAsk,
}

/// Sem um keyframe o decoder não tem referência: no começo e depois de um erro, descarta tudo
/// até o próximo keyframe, pedindo um só uma vez.
#[derive(Debug)]
struct KeyframeGate {
    waiting: bool,
    asked: bool,
}

impl KeyframeGate {
    fn new() -> Self {
        Self {
            waiting: true,
            asked: false,
        }
    }

    fn admit(&mut self, keyframe: bool) -> Admit {
        if self.waiting && !keyframe {
            if self.asked {
                return Admit::Skip;
            }
            self.asked = true;
            return Admit::SkipAndAsk;
        }
        self.waiting = false;
        self.asked = false;
        Admit::Decode
    }

    /// O frame não decodificou (corrompido ou faltou um antes): quem chama já pediu keyframe.
    fn decode_failed(&mut self) {
        self.waiting = true;
        self.asked = true;
    }
}

/// Da captura (no relógio de quem transmite, corrigido por `offset`) até agora, em µs.
fn latency_us(captured_us: u64, offset: i64, now_us: u64) -> u32 {
    let captured_local = captured_us as i64 + offset;
    (now_us as i64 - captured_local).clamp(0, u32::MAX as i64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comeca_esperando_keyframe_e_pede_um_so_uma_vez() {
        let mut gate = KeyframeGate::new();
        assert_eq!(gate.admit(false), Admit::SkipAndAsk);
        assert_eq!(gate.admit(false), Admit::Skip);
        assert_eq!(gate.admit(false), Admit::Skip);
        assert_eq!(gate.admit(true), Admit::Decode);
        assert_eq!(gate.admit(false), Admit::Decode, "depois do keyframe, tudo decodifica");
    }

    #[test]
    fn erro_de_decode_volta_a_esperar_keyframe_sem_pedir_de_novo() {
        let mut gate = KeyframeGate::new();
        assert_eq!(gate.admit(true), Admit::Decode);
        gate.decode_failed();
        assert_eq!(gate.admit(false), Admit::Skip, "o pedido já saiu junto com o erro");
        assert_eq!(gate.admit(true), Admit::Decode);
        // Outro erro depois de recuperar: o próximo keyframe recupera de novo.
        gate.decode_failed();
        assert_eq!(gate.admit(true), Admit::Decode);
        assert_eq!(gate.admit(false), Admit::Decode);
    }

    #[test]
    fn latencia_usa_o_relogio_corrigido_de_quem_transmite() {
        assert_eq!(latency_us(1_000, 0, 31_000), 30_000);
        // O relógio de quem transmite está 5 ms atrasado em relação ao nosso.
        assert_eq!(latency_us(1_000, 5_000, 31_000), 25_000);
        // Relógios mal estimados nunca dão latência negativa nem estouram.
        assert_eq!(latency_us(50_000, 0, 31_000), 0);
        assert_eq!(latency_us(0, -(u32::MAX as i64) * 2, 0), u32::MAX);
    }

    #[test]
    #[ignore = "precisa de GPU (abre uma janela de vídeo)"]
    fn sem_keyframe_pede_um_e_ainda_nao_tem_foto() {
        let (tx, rx) = mpsc::channel();
        let viewer = VideoViewer::start(
            WindowTarget::Standalone {
                title: "teste do SquadView".into(),
            },
            StreamInfo {
                size: Size::new(640, 360),
                fps: 30,
                encoder: String::new(),
            },
            ClockOffset::default(),
            |_| {},
            move |e| {
                let _ = tx.send(e);
            },
        );
        assert!(viewer.submit(EncodedFrame {
            data: vec![0; 100],
            keyframe: false,
            timestamp_us: 0,
            encode_us: 0,
        }));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(ViewerEvent::NeedKeyframe));
        assert!(viewer.snapshot(SNAPSHOT_MAX, Duration::from_millis(400)).is_none());
        assert!(format!("{viewer:?}").starts_with("VideoViewer"));
    }
}
