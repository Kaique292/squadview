//! Encoder H.264 de hardware via Media Foundation (MFT assíncrono com entrada D3D11).

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;

use sv_gpu::{Gpu, GpuFrame, clock, mf as mfu};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMF2DBuffer,
    IMFMediaEventGenerator, IMFSample, IMFTransform, MEError, METransformHaveOutput, METransformNeedInput,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_MPEG2_PROFILE,
    MF_TRANSFORM_ASYNC_UNLOCK, MFCreateDXGISurfaceBuffer, MFCreateMemoryBuffer, MFCreateSample,
    MFSampleExtension_CleanPoint, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoFormat_H264,
    MFVideoFormat_NV12, eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_High,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::Interface;

use crate::{Backend, EncodedFrame, Encoder, EncoderConfig, annexb};

/// Por que o encoder não pôde ser criado.
#[derive(Debug)]
pub enum EncoderError {
    /// A placa ou o driver não oferece encoder H.264 de hardware.
    NoHardwareEncoder,
    Windows(windows::core::Error),
}

impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHardwareEncoder => write!(
                f,
                "nenhum encoder H.264 de hardware encontrado (driver da placa atualizado?)"
            ),
            Self::Windows(e) => write!(f, "erro do Windows no encoder: {e}"),
        }
    }
}

impl std::error::Error for EncoderError {}

impl From<windows::core::Error> for EncoderError {
    fn from(e: windows::core::Error) -> Self {
        Self::Windows(e)
    }
}

type OnOutput = Box<dyn FnMut(EncodedFrame) + Send>;

/// Encoder H.264 de hardware. Para ao ser descartado.
pub struct MfEncoder {
    backend: Backend,
    name: String,
    frames: Option<SyncSender<GpuFrame>>,
    keyframe: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for MfEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MfEncoder")
            .field("backend", &self.backend)
            .field("name", &self.name)
            .finish()
    }
}

impl MfEncoder {
    /// Cria o encoder e a thread dele. `on_output` recebe cada frame codificado, na thread do encoder.
    pub fn start(
        gpu: &Gpu,
        config: EncoderConfig,
        on_output: impl FnMut(EncodedFrame) + Send + 'static,
    ) -> Result<Self, EncoderError> {
        // Capacidade 1: se o encoder ainda está com um frame, o próximo é descartado em vez de enfileirar.
        let (frames_tx, frames_rx) = mpsc::sync_channel::<GpuFrame>(1);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<String, EncoderError>>();
        let keyframe = Arc::new(AtomicBool::new(false));
        let gpu = gpu.clone();
        let keyframe_flag = keyframe.clone();
        let on_output: OnOutput = Box::new(on_output);

        let thread = std::thread::Builder::new()
            .name("sv-encoder".into())
            .spawn(
                move || match EncoderThread::new(&gpu, config, keyframe_flag, on_output) {
                    Ok((mut worker, name)) => {
                        let _ = ready_tx.send(Ok(name));
                        worker.run(frames_rx);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                },
            )
            .expect("não foi possível criar a thread do encoder");

        match ready_rx.recv() {
            Ok(Ok(name)) => Ok(Self {
                backend: Backend::from_mft_name(&name),
                name,
                frames: Some(frames_tx),
                keyframe,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(EncoderError::NoHardwareEncoder)
            }
        }
    }

    /// Nome do encoder, ex.: "NVIDIA H.264 Encoder MFT".
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Encoder for MfEncoder {
    fn backend(&self) -> Backend {
        self.backend
    }

    fn submit(&self, frame: GpuFrame) -> bool {
        match self.frames.as_ref().map(|tx| tx.try_send(frame)) {
            Some(Ok(())) => true,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => false,
        }
    }

    fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
    }
}

impl Drop for MfEncoder {
    fn drop(&mut self) {
        // Fechar o canal acorda a thread, que encerra o MFT e sai.
        self.frames = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Estado que vive só na thread do encoder (objetos COM não saem dela).
struct EncoderThread {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    codec: ICodecAPI,
    config: EncoderConfig,
    keyframe: Arc<AtomicBool>,
    on_output: OnOutput,
    provides_samples: bool,
    output_buffer_size: u32,
    sequence_header: Vec<u8>,
    /// Uma amostra MF por textura de entrada, reaproveitada a cada frame.
    samples: HashMap<usize, IMFSample>,
    /// Momento em que cada frame (pelo timestamp) foi entregue, para medir o tempo de encode.
    in_flight: VecDeque<(u64, u64)>,
}

impl EncoderThread {
    fn new(
        gpu: &Gpu,
        config: EncoderConfig,
        keyframe: Arc<AtomicBool>,
        on_output: OnOutput,
    ) -> Result<(Self, String), EncoderError> {
        mfu::init_thread()?;
        let found = mfu::find_transform(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_NV12,
            MFVideoFormat_H264,
        )?
        .ok_or(EncoderError::NoHardwareEncoder)?;
        let transform = found.transform;

        // SAFETY: todas as chamadas são COM na thread que criou os objetos, com argumentos válidos.
        unsafe {
            let attrs = transform.GetAttributes()?;
            attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);

            let manager = mfu::device_manager(gpu)?;
            transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;

            let codec: ICodecAPI = transform.cast()?;
            set(
                &codec,
                &CODECAPI_AVEncCommonRateControlMode,
                VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
            );
            set(
                &codec,
                &CODECAPI_AVEncCommonMeanBitRate,
                VARIANT::from(config.bitrate_kbps * 1000),
            );
            set(&codec, &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
            set(&codec, &CODECAPI_AVEncMPVDefaultBPictureCount, VARIANT::from(0u32));
            // GOP longo (2 min): keyframes saem sob demanda, quando alguém entra ou perde um frame.
            set(&codec, &CODECAPI_AVEncMPVGOPSize, VARIANT::from(config.fps * 120));

            let output = mfu::video_type(MFVideoFormat_H264, config.size, config.fps)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate_kbps * 1000)?;
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
            transform.SetOutputType(0, &output, 0)?;
            let input = mfu::video_type(MFVideoFormat_NV12, config.size, config.fps)?;
            transform.SetInputType(0, &input, 0)?;

            let info = transform.GetOutputStreamInfo(0)?;
            let provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            let events: IMFMediaEventGenerator = transform.cast()?;

            let mut worker = Self {
                transform,
                events,
                codec,
                config,
                keyframe,
                on_output,
                provides_samples,
                output_buffer_size: info.cbSize.max(1 << 20),
                sequence_header: Vec::new(),
                samples: HashMap::new(),
                in_flight: VecDeque::new(),
            };
            worker.refresh_sequence_header();
            Ok((worker, found.name))
        }
    }

    fn run(&mut self, frames: Receiver<GpuFrame>) {
        loop {
            // SAFETY: chamada bloqueante no gerador de eventos do MFT, na thread dona dele.
            let event = match unsafe { self.events.GetEvent(Default::default()) } {
                Ok(e) => e,
                Err(_) => break,
            };
            // SAFETY: leitura do tipo de um evento válido.
            let kind = unsafe { event.GetType() }.unwrap_or(MEError.0 as u32) as i32;
            let result = if kind == METransformNeedInput.0 {
                match frames.recv() {
                    Ok(frame) => self.process_input(&frame),
                    Err(_) => break, // MfEncoder foi descartado.
                }
            } else if kind == METransformHaveOutput.0 {
                self.process_output()
            } else if kind == MEError.0 {
                break;
            } else {
                Ok(())
            };
            if result.is_err() {
                break;
            }
        }
        // SAFETY: encerra o stream do MFT na thread dona dele.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
        }
    }

    fn process_input(&mut self, frame: &GpuFrame) -> windows::core::Result<()> {
        if self.keyframe.swap(false, Ordering::Relaxed) {
            set(&self.codec, &CODECAPI_AVEncVideoForceKeyFrame, VARIANT::from(1u32));
        }
        let sample = self.sample_for(&frame.texture)?;
        let duration = 10_000_000 / i64::from(self.config.fps.max(1));
        // SAFETY: amostra válida desta thread; os tempos do MF são em unidades de 100 ns.
        unsafe {
            sample.SetSampleTime(frame.timestamp_us as i64 * 10)?;
            sample.SetSampleDuration(duration)?;
            self.transform.ProcessInput(0, &sample, 0)?;
        }
        if self.in_flight.len() > 8 {
            self.in_flight.pop_front();
        }
        self.in_flight.push_back((frame.timestamp_us, clock::now_us()));
        Ok(())
    }

    fn sample_for(&mut self, texture: &ID3D11Texture2D) -> windows::core::Result<IMFSample> {
        let key = texture.as_raw() as usize;
        if let Some(sample) = self.samples.get(&key) {
            return Ok(sample.clone());
        }
        if self.samples.len() >= 16 {
            self.samples.clear();
        }
        // SAFETY: a textura é válida e pertence ao device entregue ao MFT.
        let sample = unsafe {
            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false)?;
            let length = buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()?;
            buffer.SetCurrentLength(length)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample
        };
        self.samples.insert(key, sample.clone());
        Ok(sample)
    }

    fn process_output(&mut self) -> windows::core::Result<()> {
        let provided = if self.provides_samples {
            None
        } else {
            // SAFETY: criação de amostra e buffer de memória com tamanho informado pelo MFT.
            unsafe {
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreateMemoryBuffer(self.output_buffer_size)?)?;
                Some(sample)
            }
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(provided),
            ..Default::default()
        };
        let mut status = 0u32;
        // SAFETY: `output` tem um único buffer válido; liberamos os campos ManuallyDrop logo depois.
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        drop(unsafe { ManuallyDrop::take(&mut output.pEvents) });

        match result {
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                // O encoder mudou o formato de saída: aceita o novo e segue.
                // SAFETY: renegociação padrão do MF na thread dona do MFT.
                unsafe {
                    let new_type = self.transform.GetOutputAvailableType(0, 0)?;
                    self.transform.SetOutputType(0, &new_type, 0)?;
                }
                self.refresh_sequence_header();
                return Ok(());
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        let Some(sample) = sample else { return Ok(()) };
        let (mut data, timestamp_us, clean_point) = read_sample(&sample)?;
        let keyframe = clean_point || annexb::is_keyframe(&data);
        // Quem entra no meio do stream precisa de SPS/PPS no keyframe.
        if keyframe && !annexb::has_parameter_sets(&data) && !self.sequence_header.is_empty() {
            data.splice(0..0, self.sequence_header.iter().copied());
        }
        let encode_us = self
            .in_flight
            .iter()
            .position(|&(ts, _)| ts == timestamp_us)
            .and_then(|i| self.in_flight.remove(i))
            .map(|(_, at)| clock::now_us().saturating_sub(at) as u32)
            .unwrap_or(0);
        (self.on_output)(EncodedFrame {
            data,
            keyframe,
            timestamp_us,
            encode_us,
        });
        Ok(())
    }

    fn refresh_sequence_header(&mut self) {
        // SAFETY: leitura de atributo do tipo de saída atual; o buffer tem o tamanho informado.
        let header = unsafe {
            self.transform.GetOutputCurrentType(0).ok().and_then(|t| {
                let size = t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).ok()?;
                let mut buf = vec![0u8; size as usize];
                t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None).ok()?;
                Some(buf)
            })
        };
        if let Some(header) = header {
            self.sequence_header = header;
        }
    }
}

/// Copia os bytes codificados de uma amostra (são pequenos: dezenas de KB por frame).
fn read_sample(sample: &IMFSample) -> windows::core::Result<(Vec<u8>, u64, bool)> {
    // SAFETY: o buffer fica travado só durante a cópia e é destravado em seguida.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut ptr, None, Some(&mut len))?;
        let data = std::slice::from_raw_parts(ptr, len as usize).to_vec();
        buffer.Unlock()?;
        let timestamp_us = (sample.GetSampleTime().unwrap_or(0) / 10).max(0) as u64;
        let clean_point = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
        Ok((data, timestamp_us, clean_point))
    }
}

/// Ajusta uma propriedade do encoder. Encoders diferentes aceitam propriedades diferentes;
/// uma recusada não impede o uso (o padrão do fabricante vale).
fn set(codec: &ICodecAPI, key: &windows::core::GUID, value: VARIANT) {
    // SAFETY: chave e valor válidos durante a chamada.
    unsafe {
        let _ = codec.SetValue(key, &value);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use sv_core::QualityLayer;
    use sv_gpu::Size;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

    use super::*;

    pub(crate) fn nv12_texture(gpu: &Gpu, size: Size) -> ID3D11Texture2D {
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
        unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut texture)).unwrap() };
        texture.unwrap()
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder H.264"]
    fn codifica_300_frames_em_1080p60_com_keyframe_inicial() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig::for_layer(QualityLayer::High);
        let (tx, rx) = mpsc::channel();
        let encoder = MfEncoder::start(&gpu, config, move |f| {
            let _ = tx.send(f);
        })
        .expect("encoder");
        eprintln!("encoder: {} ({:?})", encoder.name(), encoder.backend());

        let textures: Vec<_> = (0..4).map(|_| nv12_texture(&gpu, config.size)).collect();
        let mut submitted = 0;
        for i in 0..300u64 {
            let frame = GpuFrame {
                texture: textures[i as usize % 4].clone(),
                slice: 0,
                size: config.size,
                timestamp_us: i * 16_667,
            };
            if encoder.submit(frame) {
                submitted += 1;
            }
            std::thread::sleep(Duration::from_micros(16_667));
        }
        drop(encoder);

        let frames: Vec<EncodedFrame> = rx.try_iter().collect();
        assert!(submitted >= 290, "só {submitted} frames aceitos");
        assert!(
            frames.len() >= submitted - 5,
            "{} de {submitted} frames saíram",
            frames.len()
        );
        assert!(frames[0].keyframe, "o primeiro frame tem que ser keyframe");
        assert!(
            annexb::has_parameter_sets(&frames[0].data),
            "o primeiro keyframe precisa de SPS/PPS"
        );
        let mut times: Vec<u32> = frames.iter().map(|f| f.encode_us).collect();
        times.sort_unstable();
        eprintln!(
            "tempo de encode: mediana {} us, p95 {} us",
            times[times.len() / 2],
            times[times.len() * 95 / 100]
        );
    }
}
