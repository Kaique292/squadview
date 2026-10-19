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
    let mut waiting_keyframe = true;
    let mut lost = false;
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
        if waiting_keyframe && !frame.keyframe {
            if !lost {
                lost = true;
                on_event(ViewerEvent::NeedKeyframe);
            }
            continue;
        }
        waiting_keyframe = false;
        lost = false;

        let offset = clock_offset.load(Ordering::Relaxed);
        let mut present_error = None;
        let decoded = decoder.decode(&frame.data, frame.timestamp_us, |image| {
            if let Err(e) = renderer.present(image) {
                present_error = Some(e);
            }
            last = Some(image.clone());
            let captured_local = frame.timestamp_us as i64 + offset;
            let latency = (clock::now_us() as i64 - captured_local).clamp(0, u32::MAX as i64) as u32;
            on_event(ViewerEvent::Presented { latency_us: latency });
        });
        if decoded.is_err() {
            // Frame corrompido ou perdido: limpa o decoder e espera o próximo keyframe.
            decoder.flush();
            waiting_keyframe = true;
            lost = true;
            on_event(ViewerEvent::NeedKeyframe);
        }
        if let Some(e) = present_error {
            return Err(e);
        }
    }
}
