//! Lado de quem transmite: captura → conversão para NV12 → encoder de hardware.

use std::sync::{Arc, Mutex};

use sv_capture::{CaptureSource, FrameThrottle, VideoCapture};
use sv_core::StreamQuality;
use sv_encode::{EncodedFrame, Encoder, EncoderConfig, MfEncoder};
use sv_gpu::{Converter, Gpu, GpuFrame, Size, fit_inside};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

use crate::SessionError;

/// Texturas NV12 em rodízio: a captura escreve numa enquanto o encoder lê outra.
const RING: usize = 4;

/// Informações do stream, conhecidas depois do primeiro frame capturado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub size: Size,
    pub fps: u32,
    pub encoder: String,
}

/// Eventos que o [`VideoSender`] produz, na thread do encoder.
#[derive(Debug)]
pub enum SenderEvent {
    /// A fonte entregou um quadro novo (antes do limite de fps): mede o fps do jogo.
    Captured,
    /// O encoder foi criado (só uma vez, no primeiro frame).
    Started(StreamInfo),
    Frame(EncodedFrame),
    /// Um frame capturado foi descartado porque o encoder estava ocupado.
    Dropped,
    /// Não foi possível criar o encoder.
    Failed(String),
}

/// Uma transmissão de vídeo em andamento. Para quando é descartada.
pub struct VideoSender {
    // A ordem importa: a captura para antes do encoder ser destruído.
    _capture: VideoCapture,
    encoder: Arc<Mutex<Option<MfEncoder>>>,
}

impl std::fmt::Debug for VideoSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoSender").finish_non_exhaustive()
    }
}

impl VideoSender {
    /// Começa a capturar `source` na qualidade `quality`. Cada evento sai por `on_event`.
    pub fn start(
        gpu: &Gpu,
        source: CaptureSource,
        quality: StreamQuality,
        on_event: impl FnMut(SenderEvent) + Send + 'static,
    ) -> Result<Self, SessionError> {
        let on_event = Arc::new(Mutex::new(on_event));
        let encoder: Arc<Mutex<Option<MfEncoder>>> = Arc::new(Mutex::new(None));
        let mut pipeline = CapturePipeline {
            gpu: SendGpu(gpu.clone()),
            quality,
            throttle: FrameThrottle::new(quality.fps()),
            state: None,
            failed: false,
            encoder: encoder.clone(),
            on_event: on_event.clone(),
        };
        let capture = VideoCapture::start(gpu, source, move |frame| pipeline.on_frame(frame))
            .map_err(|e| SessionError::Capture(e.to_string()))?;
        Ok(Self {
            _capture: capture,
            encoder,
        })
    }

    /// Pede um keyframe (alguém começou a assistir ou perdeu um frame).
    pub fn request_keyframe(&self) {
        if let Some(encoder) = self.encoder.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            encoder.request_keyframe();
        }
    }
}

/// O que roda dentro do callback da captura.
struct CapturePipeline<F> {
    gpu: SendGpu,
    quality: StreamQuality,
    throttle: FrameThrottle,
    state: Option<Prepared>,
    failed: bool,
    encoder: Arc<Mutex<Option<MfEncoder>>>,
    on_event: Arc<Mutex<F>>,
}

struct Prepared {
    config: EncoderConfig,
    converter: Converter,
    ring: Vec<ID3D11Texture2D>,
    next: usize,
}

impl<F: FnMut(SenderEvent) + Send + 'static> CapturePipeline<F> {
    fn on_frame(&mut self, frame: &GpuFrame) {
        self.emit(SenderEvent::Captured);
        if self.failed || !self.throttle.accept(frame.timestamp_us) {
            return;
        }
        if self.state.is_none() {
            match self.prepare(frame.size) {
                Ok(state) => self.state = Some(state),
                Err(e) => {
                    self.failed = true;
                    self.emit(SenderEvent::Failed(e.to_string()));
                    return;
                }
            }
        }
        let Some(state) = self.state.as_mut() else { return };
        let target = state.ring[state.next].clone();
        state.next = (state.next + 1) % RING;

        // A fonte pode mudar de tamanho (janela redimensionada): o encoder continua no tamanho
        // inicial e a imagem é encaixada nele, com barras se a proporção mudar.
        let size = state.config.size;
        let converted = state.converter.ensure_sizes(frame.size, size).and_then(|()| {
            state
                .converter
                .convert(&frame.texture, frame.slice, &target, Some(fit_inside(frame.size, size)))
        });
        if converted.is_err() {
            return;
        }
        // SAFETY: envia os comandos de conversão para a GPU antes de o encoder ler a textura.
        unsafe { self.gpu.0.context.Flush() };

        let accepted = self
            .encoder
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|enc| {
                enc.submit(GpuFrame {
                    texture: target,
                    slice: 0,
                    size,
                    timestamp_us: frame.timestamp_us,
                })
            });
        if !accepted {
            self.emit(SenderEvent::Dropped);
        }
    }

    fn prepare(&mut self, source: sv_gpu::Size) -> Result<Prepared, SessionError> {
        let config = EncoderConfig::for_quality(self.quality, source);
        let converter =
            Converter::new(&self.gpu.0, source, config.size).map_err(|e| SessionError::Gpu(e.to_string()))?;
        let ring = (0..RING)
            .map(|_| nv12_texture(&self.gpu.0, config.size))
            .collect::<windows::core::Result<Vec<_>>>()
            .map_err(|e| SessionError::Gpu(e.to_string()))?;

        let on_event = self.on_event.clone();
        let encoder = MfEncoder::start(&self.gpu.0, config, move |frame| {
            (on_event.lock().unwrap_or_else(|e| e.into_inner()))(SenderEvent::Frame(frame));
        })
        .map_err(|e| SessionError::Encoder(e.to_string()))?;
        let info = StreamInfo {
            size: config.size,
            fps: config.fps,
            encoder: encoder.name().to_owned(),
        };
        *self.encoder.lock().unwrap_or_else(|e| e.into_inner()) = Some(encoder);
        self.emit(SenderEvent::Started(info));
        Ok(Prepared {
            config,
            converter,
            ring,
            next: 0,
        })
    }

    fn emit(&self, event: SenderEvent) {
        (self.on_event.lock().unwrap_or_else(|e| e.into_inner()))(event);
    }
}

/// O [`Gpu`] tem proteção multithread ligada, então pode ser usado a partir da thread da captura.
struct SendGpu(Gpu);

// SAFETY: o device D3D11 é thread-safe e o contexto imediato tem SetMultithreadProtected(true)
// (ver sv_gpu::Gpu::new). Os objetos derivados (Converter, texturas) só são usados nesta thread.
unsafe impl Send for SendGpu {}
// SAFETY: idem; necessário porque Prepared/Converter guardam clones do Gpu.
unsafe impl Send for Prepared {}

fn nv12_texture(gpu: &Gpu, size: Size) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.width,
        Height: size.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    };
    let mut texture = None;
    // SAFETY: descritor válido e ponteiro de saída vivo.
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    Ok(texture.expect("CreateTexture2D sem textura"))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// Para rodar: `cargo test -p sv-session --lib sender -- --ignored` numa máquina com placa de
    /// vídeo (NVENC/AMF/QSV) e uma sessão de desktop.
    #[test]
    #[ignore = "precisa de placa de vídeo com encoder e de uma sessão de desktop"]
    fn transmite_o_monitor_principal_e_comeca_por_um_keyframe() {
        let gpu = Gpu::new().unwrap();
        let monitor = sv_capture::list_sources()
            .into_iter()
            .find(|s| matches!(s.id, CaptureSource::Monitor(_)))
            .expect("um monitor");
        let (tx, rx) = mpsc::channel();
        let sender = VideoSender::start(&gpu, monitor.id, StreamQuality::DEFAULT, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        assert!(format!("{sender:?}").starts_with("VideoSender"));
        let mut started = None;
        let mut first_frame = None;
        while started.is_none() || first_frame.is_none() {
            match rx.recv_timeout(Duration::from_secs(5)).expect("nada em 5 s") {
                SenderEvent::Started(info) => started = Some(info),
                SenderEvent::Frame(f) if first_frame.is_none() => first_frame = Some(f),
                SenderEvent::Failed(e) => panic!("encoder falhou: {e}"),
                _ => {}
            }
        }
        let info = started.unwrap();
        assert!(info.size.width > 0 && !info.encoder.is_empty(), "{info:?}");
        assert!(first_frame.unwrap().keyframe, "o primeiro frame é keyframe");
        sender.request_keyframe();
    }
}
