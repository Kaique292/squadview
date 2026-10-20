//! Encoder H.264 de hardware via Media Foundation (MFT assíncrono com entrada D3D11).

use std::collections::VecDeque;
use std::fmt;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;

use sv_gpu::{BoundedCache, Gpu, GpuFrame, clock, mf as mfu};
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

        match wait_ready(&ready_rx) {
            Ok(name) => Ok(Self {
                backend: Backend::from_mft_name(&name),
                name,
                frames: Some(frames_tx),
                keyframe,
                thread: Some(thread),
            }),
            Err(e) => {
                let _ = thread.join();
                Err(e)
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
        try_submit(self.frames.as_ref(), frame)
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

/// Entrega sem bloquear: se o encoder ainda está com o frame anterior (canal cheio) ou já parou,
/// devolve `false` e o frame é descartado.
fn try_submit<T>(frames: Option<&SyncSender<T>>, frame: T) -> bool {
    match frames.map(|tx| tx.try_send(frame)) {
        Some(Ok(())) => true,
        Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => false,
    }
}

/// Espera a thread do encoder dizer se conseguiu abrir o MFT. Se ela morreu sem responder,
/// trata como falta de encoder.
fn wait_ready<T>(ready: &Receiver<Result<T, EncoderError>>) -> Result<T, EncoderError> {
    ready.recv().unwrap_or(Err(EncoderError::NoHardwareEncoder))
}

/// Quanto tempo cada frame passa dentro do encoder. Guarda poucos frames: se a saída de algum
/// nunca vier, ele sai da fila sozinho.
#[derive(Debug, Default)]
struct InFlight {
    /// (timestamp do frame, momento em que foi entregue ao encoder), em µs.
    entries: VecDeque<(u64, u64)>,
}

impl InFlight {
    const LIMIT: usize = 8;

    fn push(&mut self, timestamp_us: u64, submitted_at_us: u64) {
        if self.entries.len() > Self::LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back((timestamp_us, submitted_at_us));
    }

    /// Tira o frame da fila e devolve quando ele foi entregue, se ainda estava lá.
    fn take(&mut self, timestamp_us: u64) -> Option<u64> {
        let i = self.entries.iter().position(|&(ts, _)| ts == timestamp_us)?;
        self.entries.remove(i).map(|(_, at)| at)
    }
}

/// Decide se a saída é keyframe e, se for e não trouxer SPS/PPS, coloca o cabeçalho de sequência
/// na frente (quem entra no meio do stream precisa dele para decodificar).
fn finish_frame(data: &mut Vec<u8>, clean_point: bool, sequence_header: &[u8]) -> bool {
    let keyframe = clean_point || annexb::is_keyframe(data);
    if keyframe && !annexb::has_parameter_sets(data) && !sequence_header.is_empty() {
        data.splice(0..0, sequence_header.iter().copied());
    }
    keyframe
}

/// GOP longo (2 min): keyframes saem sob demanda, quando alguém entra ou perde um frame.
const fn gop_size(fps: u32) -> u32 {
    fps * 120
}

/// Propriedades de baixa latência pedidas ao encoder: CBR no bitrate da configuração, modo de
/// baixa latência, sem B-frames e GOP longo.
fn low_latency_settings(config: &EncoderConfig) -> [(&'static windows::core::GUID, VARIANT); 5] {
    [
        (
            &CODECAPI_AVEncCommonRateControlMode,
            VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
        ),
        (
            &CODECAPI_AVEncCommonMeanBitRate,
            VARIANT::from(config.bitrate_kbps * 1000),
        ),
        (&CODECAPI_AVLowLatencyMode, VARIANT::from(true)),
        (&CODECAPI_AVEncMPVDefaultBPictureCount, VARIANT::from(0u32)),
        (&CODECAPI_AVEncMPVGOPSize, VARIANT::from(gop_size(config.fps))),
    ]
}

/// Tamanho do buffer de saída quando somos nós que fornecemos a amostra: o que o MFT pede, com
/// no mínimo 1 MB.
fn output_buffer_size(requested: u32) -> u32 {
    requested.max(1 << 20)
}

/// Limite de amostras de entrada em cache (uma por textura de entrada).
const SAMPLE_CACHE_LIMIT: usize = 16;

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
    samples: BoundedCache<usize, IMFSample>,
    /// Momento em que cada frame (pelo timestamp) foi entregue, para medir o tempo de encode.
    in_flight: InFlight,
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
            for (key, value) in low_latency_settings(&config) {
                set(&codec, key, value);
            }

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
                output_buffer_size: output_buffer_size(info.cbSize),
                sequence_header: Vec::new(),
                samples: BoundedCache::new(SAMPLE_CACHE_LIMIT),
                in_flight: InFlight::default(),
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
        // SAFETY: amostra válida desta thread; os tempos do MF são em unidades de 100 ns.
        unsafe {
            sample.SetSampleTime(mfu::us_to_mf_time(frame.timestamp_us))?;
            sample.SetSampleDuration(mfu::frame_duration(self.config.fps))?;
            self.transform.ProcessInput(0, &sample, 0)?;
        }
        self.in_flight.push(frame.timestamp_us, clock::now_us());
        Ok(())
    }

    fn sample_for(&mut self, texture: &ID3D11Texture2D) -> windows::core::Result<IMFSample> {
        self.samples.get_or_try_insert(texture.as_raw() as usize, || {
            // SAFETY: a textura é válida e pertence ao device entregue ao MFT.
            unsafe {
                let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false)?;
                let length = buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()?;
                buffer.SetCurrentLength(length)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                Ok(sample)
            }
        })
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
        let keyframe = finish_frame(&mut data, clean_point, &self.sequence_header);
        let encode_us = self
            .in_flight
            .take(timestamp_us)
            .map(|at| clock::now_us().saturating_sub(at) as u32)
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
        let timestamp_us = mfu::mf_time_to_us(sample.GetSampleTime().unwrap_or(0));
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

    // SPS, PPS e IDR; e um slice comum (mesmos bytes dos testes de annexb).
    const SPS_PPS: &[u8] = &[0, 0, 0, 1, 0x67, 0x42, 0x00, 0, 0, 0, 1, 0x68, 0xce];
    const IDR: &[u8] = &[0, 0, 1, 0x65, 0x88, 0x84];
    const DELTA: &[u8] = &[0, 0, 0, 1, 0x41, 0x9a, 0x02];

    #[test]
    fn entrega_sem_bloquear_e_descarta_quando_o_encoder_esta_ocupado() {
        let (tx, rx) = mpsc::sync_channel::<u32>(1);
        assert!(try_submit(Some(&tx), 1));
        assert!(!try_submit(Some(&tx), 2), "canal cheio: descarta em vez de esperar");
        assert_eq!(rx.recv().unwrap(), 1);
        assert!(try_submit(Some(&tx), 3));
        drop(rx);
        assert!(!try_submit(Some(&tx), 4), "encoder parado");
        assert!(!try_submit::<u32>(None, 5), "encoder já descartado");
    }

    #[test]
    fn espera_a_thread_do_encoder_responder() {
        let (tx, rx) = mpsc::channel();
        tx.send(Ok("NVIDIA H.264 Encoder MFT")).unwrap();
        assert_eq!(wait_ready(&rx).unwrap(), "NVIDIA H.264 Encoder MFT");

        tx.send(Err(EncoderError::NoHardwareEncoder)).unwrap();
        assert!(matches!(wait_ready(&rx), Err(EncoderError::NoHardwareEncoder)));

        // A thread morreu sem responder: vira "sem encoder", sem travar.
        drop(tx);
        assert!(matches!(wait_ready::<&str>(&rx), Err(EncoderError::NoHardwareEncoder)));
    }

    #[test]
    fn mede_o_tempo_de_cada_frame_dentro_do_encoder() {
        let mut f = InFlight::default();
        f.push(100, 5_000);
        f.push(200, 5_010);
        assert_eq!(f.take(200), Some(5_010));
        assert_eq!(f.take(200), None, "cada frame só é medido uma vez");
        assert_eq!(f.take(100), Some(5_000));
        assert_eq!(f.take(999), None);
    }

    #[test]
    fn frames_que_nunca_saem_nao_acumulam() {
        let mut f = InFlight::default();
        for ts in 0..100 {
            f.push(ts, ts * 10);
        }
        assert_eq!(f.entries.len(), InFlight::LIMIT + 1);
        assert_eq!(f.take(0), None, "o mais antigo já saiu da fila");
        assert_eq!(f.take(99), Some(990));
    }

    #[test]
    fn keyframe_sem_sps_e_pps_ganha_o_cabecalho_de_sequencia() {
        let mut data = IDR.to_vec();
        assert!(finish_frame(&mut data, false, SPS_PPS));
        assert_eq!(data, [SPS_PPS, IDR].concat());
        assert!(annexb::has_parameter_sets(&data));
    }

    #[test]
    fn keyframe_que_ja_traz_sps_e_pps_fica_como_esta() {
        let original = [SPS_PPS, IDR].concat();
        let mut data = original.clone();
        assert!(finish_frame(&mut data, false, SPS_PPS));
        assert_eq!(data, original);
    }

    #[test]
    fn frame_comum_nao_ganha_cabecalho() {
        let mut data = DELTA.to_vec();
        assert!(!finish_frame(&mut data, false, SPS_PPS));
        assert_eq!(data, DELTA);
    }

    #[test]
    fn clean_point_do_media_foundation_conta_como_keyframe() {
        // Alguns encoders marcam o keyframe só no atributo da amostra.
        let mut data = DELTA.to_vec();
        assert!(finish_frame(&mut data, true, SPS_PPS));
        assert!(data.starts_with(SPS_PPS));
        // Sem cabeçalho conhecido, não inventa nada.
        let mut data = IDR.to_vec();
        assert!(finish_frame(&mut data, false, &[]));
        assert_eq!(data, IDR);
    }

    #[test]
    fn configuracao_de_baixa_latencia_cbr_sem_b_frames() {
        let config = EncoderConfig::for_layer(QualityLayer::High);
        let settings = low_latency_settings(&config);
        let value = |key: &windows::core::GUID| {
            settings
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .expect("propriedade configurada")
        };
        assert_eq!(
            u32::try_from(value(&CODECAPI_AVEncCommonRateControlMode)).unwrap(),
            eAVEncCommonRateControlMode_CBR.0 as u32
        );
        assert_eq!(
            u32::try_from(value(&CODECAPI_AVEncCommonMeanBitRate)).unwrap(),
            config.bitrate_kbps * 1000
        );
        assert!(bool::try_from(value(&CODECAPI_AVLowLatencyMode)).unwrap());
        assert_eq!(u32::try_from(value(&CODECAPI_AVEncMPVDefaultBPictureCount)).unwrap(), 0);
        // 60 fps × 120 s: keyframe só sob demanda.
        assert_eq!(u32::try_from(value(&CODECAPI_AVEncMPVGOPSize)).unwrap(), 7_200);
    }

    #[test]
    fn buffer_de_saida_tem_pelo_menos_1_mb() {
        assert_eq!(output_buffer_size(0), 1 << 20);
        assert_eq!(output_buffer_size(4 << 20), 4 << 20);
    }

    #[test]
    fn erros_do_encoder_explicam_o_problema() {
        use std::error::Error;
        let sem = EncoderError::NoHardwareEncoder;
        assert!(sem.to_string().contains("driver"));
        assert!(sem.source().is_none());
        let win: EncoderError = windows::core::Error::from(MF_E_TRANSFORM_STREAM_CHANGE).into();
        assert!(matches!(win, EncoderError::Windows(_)));
        assert!(win.to_string().starts_with("erro do Windows no encoder"));
        assert!(format!("{win:?}").contains("Windows"));
    }

    fn frame(texture: &ID3D11Texture2D, size: Size, i: u64) -> GpuFrame {
        GpuFrame {
            texture: texture.clone(),
            slice: 0,
            size,
            timestamp_us: i * 16_667,
        }
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder H.264"]
    fn keyframe_sob_demanda_sai_no_meio_do_stream_com_sps_e_pps() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig::for_layer(QualityLayer::Low);
        let (tx, rx) = mpsc::channel();
        let encoder = MfEncoder::start(&gpu, config, move |f| {
            let _ = tx.send(f);
        })
        .expect("encoder");
        assert!(format!("{encoder:?}").contains("backend"));
        let texture = nv12_texture(&gpu, config.size);
        for i in 0..60u64 {
            if i == 30 {
                encoder.request_keyframe();
            }
            while !encoder.submit(frame(&texture, config.size, i)) {
                std::thread::sleep(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(encoder);
        let frames: Vec<EncodedFrame> = rx.try_iter().collect();
        let keyframes: Vec<u64> = frames
            .iter()
            .filter(|f| f.keyframe)
            .map(|f| f.timestamp_us / 16_667)
            .collect();
        assert_eq!(keyframes.first(), Some(&0));
        assert!(
            keyframes.iter().any(|&i| (30..40).contains(&i)),
            "nenhum keyframe depois do pedido: {keyframes:?}"
        );
        assert!(
            frames
                .iter()
                .filter(|f| f.keyframe)
                .all(|f| annexb::has_parameter_sets(&f.data))
        );
        assert!(frames.iter().any(|f| f.encode_us > 0), "o tempo de encode foi medido");
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder H.264"]
    fn frames_entregues_rapido_demais_sao_descartados_sem_travar() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig::for_layer(QualityLayer::High);
        let encoder = MfEncoder::start(&gpu, config, |_| {}).expect("encoder");
        let textures: Vec<_> = (0..20).map(|_| nv12_texture(&gpu, config.size)).collect();
        let aceitos = textures
            .iter()
            .enumerate()
            .filter(|(i, t)| encoder.submit(frame(t, config.size, *i as u64)))
            .count();
        assert!(
            aceitos < textures.len(),
            "aceitou todos os {aceitos}: não descartou nada"
        );
        let start = std::time::Instant::now();
        drop(encoder);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "parar o encoder demorou demais"
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder H.264"]
    fn tamanho_invalido_vira_erro_e_nao_trava() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig {
            size: Size::new(0, 0),
            fps: 60,
            bitrate_kbps: 8_000,
        };
        let err = MfEncoder::start(&gpu, config, |_| {}).expect_err("0x0 não é um tamanho de vídeo");
        assert!(matches!(err, EncoderError::Windows(_)), "{err}");
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
