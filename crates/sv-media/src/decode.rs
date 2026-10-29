//! Decoder H.264 por GPU via Media Foundation (DXVA). A saída fica na GPU como textura NV12.

use std::fmt;
use std::mem::ManuallyDrop;

use sv_gpu::{Gpu, GpuFrame, Size, mf as mfu};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_FRAME_SIZE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
    MF_SA_D3D11_AWARE, MFCreateMemoryBuffer, MFCreateSample, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoArea, MFVideoFormat_H264, MFVideoFormat_NV12,
};
use windows::core::{Interface, Result};

/// Decoder H.264. Use numa única thread (a de recepção do vídeo).
pub struct Decoder {
    transform: IMFTransform,
    size: Size,
    provides_samples: bool,
    /// Amostras de entrada reaproveitadas (o decoder pode segurar a anterior por um tempo).
    inputs: Vec<(IMFSample, IMFMediaBuffer, u32)>,
    next_input: usize,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoder").field("size", &self.size).finish()
    }
}

const INPUT_POOL: usize = 4;

impl Decoder {
    /// Cria o decoder para um stream de `size` a `fps`.
    pub fn new(gpu: &Gpu, size: Size, fps: u32) -> Result<Self> {
        mfu::init_thread()?;
        let found = mfu::find_transform(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_H264,
            MFVideoFormat_NV12,
        )?
        .ok_or_else(|| windows::core::Error::from(MF_E_TRANSFORM_NEED_MORE_INPUT))?;
        let transform = found.transform;

        // SAFETY: chamadas COM na thread atual com argumentos válidos.
        unsafe {
            let attrs = transform.GetAttributes()?;
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            if attrs.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) != 0 {
                let manager = mfu::device_manager(gpu)?;
                transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
            }
            transform.SetInputType(0, &mfu::video_type(MFVideoFormat_H264, size, fps)?, 0)?;
        }
        let mut decoder = Self {
            transform,
            size,
            provides_samples: true,
            inputs: Vec::new(),
            next_input: 0,
        };
        // O tamanho visível é o do stream; o tipo de saída pode vir alinhado (1080 → 1088).
        decoder.choose_output_type()?;
        // SAFETY: mensagens de início de stream na thread dona do MFT.
        unsafe {
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        Ok(decoder)
    }

    /// Entrega um frame Annex B. Cada imagem decodificada sai por `on_frame`, ainda válida só
    /// durante a chamada (o decoder reaproveita a textura depois).
    pub fn decode(&mut self, data: &[u8], timestamp_us: u64, mut on_frame: impl FnMut(&GpuFrame)) -> Result<()> {
        let sample = self.input_sample(data)?;
        // SAFETY: amostra válida, tempos em unidades de 100 ns.
        unsafe {
            sample.SetSampleTime(mfu::us_to_mf_time(timestamp_us))?;
            self.transform.ProcessInput(0, &sample, 0)?;
        }
        loop {
            match self.next_output()? {
                Output::Frame(frame) => on_frame(&frame),
                Output::NeedMoreInput => return Ok(()),
                // O stream mudou (outra camada do simulcast, ADR 0026): a imagem passa a ter o
                // tamanho do tipo de saída novo.
                Output::FormatChanged => {
                    let (frame, aperture) = self.choose_output_type()?;
                    self.size = visible_size(self.size, frame, aperture);
                }
            }
        }
    }

    /// Descarta o que estiver no meio do caminho (ex.: frame perdido; vamos esperar um keyframe).
    pub fn flush(&mut self) {
        // SAFETY: mensagem de flush na thread dona do MFT.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }

    fn input_sample(&mut self, data: &[u8]) -> Result<IMFSample> {
        let i = self.next_input;
        self.next_input = (i + 1) % INPUT_POOL;
        let needed = data.len() as u32;
        if let Some(capacity) = grow_to(self.inputs.get(i).map(|(_, _, cap)| *cap), needed) {
            // SAFETY: criação de amostra e buffer de memória.
            let entry = unsafe {
                let buffer = MFCreateMemoryBuffer(capacity)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                (sample, buffer, capacity)
            };
            if i < self.inputs.len() {
                self.inputs[i] = entry;
            } else {
                self.inputs.push(entry);
            }
        }
        let (sample, buffer, _) = &self.inputs[i];
        // SAFETY: o buffer tem capacidade >= data.len(); fica travado só durante a cópia.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(needed)?;
        }
        Ok(sample.clone())
    }

    fn next_output(&mut self) -> Result<Output> {
        let provided = if self.provides_samples {
            None
        } else {
            // SAFETY: amostra própria para decoders que não fornecem a de saída (sem D3D).
            unsafe {
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreateMemoryBuffer(nv12_bytes(self.size))?)?;
                Some(sample)
            }
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(provided),
            ..Default::default()
        };
        let mut status = 0u32;
        // SAFETY: um buffer de saída válido; os campos ManuallyDrop são liberados logo depois.
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        drop(unsafe { ManuallyDrop::take(&mut output.pEvents) });

        match result {
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(Output::NeedMoreInput),
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => return Ok(Output::FormatChanged),
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        let Some(sample) = sample else {
            return Ok(Output::NeedMoreInput);
        };
        // SAFETY: a amostra de saída com D3D tem um IMFDXGIBuffer com a textura NV12 e a fatia.
        unsafe {
            let buffer = sample.GetBufferByIndex(0)?;
            let dxgi: IMFDXGIBuffer = buffer.cast()?;
            let mut raw = std::ptr::null_mut();
            dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw)?;
            let texture = ID3D11Texture2D::from_raw(raw);
            let slice = dxgi.GetSubresourceIndex()?;
            let timestamp_us = mfu::mf_time_to_us(sample.GetSampleTime().unwrap_or(0));
            Ok(Output::Frame(GpuFrame {
                texture,
                slice,
                size: self.size,
                timestamp_us,
            }))
        }
    }

    /// Escolhe a saída NV12 e devolve o tamanho dela e a área visível, se o decoder informar.
    fn choose_output_type(&mut self) -> Result<(Option<Size>, Option<Size>)> {
        let mut geometry = (None, None);
        // SAFETY: enumeração dos tipos de saída oferecidos pelo MFT.
        unsafe {
            for i in 0.. {
                let t = self.transform.GetOutputAvailableType(0, i)?;
                if t.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                    self.transform.SetOutputType(0, &t, 0)?;
                    let frame = t.GetUINT64(&MF_MT_FRAME_SIZE).ok().map(|v| {
                        let (w, h) = mfu::unpack(v);
                        Size::new(w, h)
                    });
                    let mut area = MFVideoArea::default();
                    let bytes = std::slice::from_raw_parts_mut(
                        (&raw mut area).cast::<u8>(),
                        std::mem::size_of::<MFVideoArea>(),
                    );
                    let aperture = t
                        .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None)
                        .ok()
                        .and_then(|()| {
                            let (w, h) = (u32::try_from(area.Area.cx).ok()?, u32::try_from(area.Area.cy).ok()?);
                            (w > 0 && h > 0).then(|| Size::new(w, h))
                        });
                    geometry = (frame, aperture);
                    break;
                }
            }
            let info = self.transform.GetOutputStreamInfo(0)?;
            self.provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        }
        Ok(geometry)
    }
}

enum Output {
    Frame(GpuFrame),
    NeedMoreInput,
    FormatChanged,
}

/// O tamanho visível depois de uma mudança de formato. A área visível do decoder vale primeiro;
/// sem ela, o tamanho do tipo de saída, a não ser que ele seja só o atual alinhado em 16
/// (1080 → 1088): aí o tamanho não mudou.
fn visible_size(current: Size, frame: Option<Size>, aperture: Option<Size>) -> Size {
    if let Some(aperture) = aperture {
        return aperture;
    }
    match frame {
        Some(f) if f.width == current.width.next_multiple_of(16) && f.height == current.height.next_multiple_of(16) => {
            current
        }
        Some(f) if f.width > 0 && f.height > 0 => f,
        _ => current,
    }
}

/// Menor buffer de entrada: cabe um keyframe 1080p comum sem realocar.
const MIN_INPUT_CAPACITY: u32 = 256 * 1024;

/// Novo tamanho para o buffer de entrada que hoje tem `current` bytes (ou nenhum), se ele não
/// comporta `needed`. Cresce em degraus (potências de 2) para não realocar a cada keyframe um
/// pouco maior; `None` quando o buffer atual serve.
fn grow_to(current: Option<u32>, needed: u32) -> Option<u32> {
    if current.is_some_and(|cap| cap >= needed) {
        return None;
    }
    Some(needed.next_power_of_two().max(MIN_INPUT_CAPACITY))
}

/// Bytes de uma imagem NV12 compacta (Y inteiro + UV com metade da altura).
fn nv12_bytes(size: Size) -> u32 {
    size.width * size.height * 3 / 2
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sv_core::QualityLayer;
    use sv_encode::{EncodedFrame, Encoder, EncoderConfig, MfEncoder};
    use sv_gpu::testing::texture;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

    use super::*;

    #[test]
    fn buffer_de_entrada_so_cresce_quando_nao_cabe() {
        assert_eq!(grow_to(None, 1_000), Some(MIN_INPUT_CAPACITY), "primeiro uso: 256 KB");
        assert_eq!(grow_to(Some(MIN_INPUT_CAPACITY), 1_000), None, "cabe: reaproveita");
        assert_eq!(grow_to(Some(MIN_INPUT_CAPACITY), MIN_INPUT_CAPACITY), None);
        // Keyframe de 300 KB: sobe para o próximo degrau.
        assert_eq!(grow_to(Some(MIN_INPUT_CAPACITY), 300_000), Some(512 * 1024));
        assert_eq!(grow_to(None, 3_000_000), Some(4 * 1024 * 1024));
    }

    #[test]
    fn tamanho_visivel_vem_da_area_do_decoder_ou_do_tipo_de_saida() {
        let hd = Size::new(1920, 1080);
        // Com a área visível, ela vale (a camada baixa do simulcast chegou).
        assert_eq!(
            visible_size(hd, Some(Size::new(1280, 720)), Some(Size::new(1280, 720))),
            Size::new(1280, 720)
        );
        assert_eq!(visible_size(hd, Some(Size::new(1920, 1088)), Some(hd)), hd);
        // Sem ela, o alinhamento de 16 não muda o tamanho, mas outra resolução muda.
        assert_eq!(visible_size(hd, Some(Size::new(1920, 1088)), None), hd);
        assert_eq!(visible_size(hd, Some(Size::new(1280, 720)), None), Size::new(1280, 720));
        assert_eq!(visible_size(hd, None, None), hd);
        assert_eq!(visible_size(hd, Some(Size::new(0, 0)), None), hd);
    }

    #[test]
    fn tamanho_de_uma_imagem_nv12() {
        assert_eq!(nv12_bytes(Size::new(1920, 1080)), 1920 * 1080 * 3 / 2);
        assert_eq!(nv12_bytes(Size::new(2, 2)), 6);
    }

    /// Codifica `count` frames com o encoder de hardware e devolve o que saiu.
    fn encode_frames(gpu: &Gpu, config: EncoderConfig, count: u64) -> Vec<EncodedFrame> {
        let (tx, rx) = std::sync::mpsc::channel();
        let encoder = MfEncoder::start(gpu, config, move |f| {
            let _ = tx.send(f);
        })
        .expect("encoder");
        let input = texture(gpu, config.size, DXGI_FORMAT_NV12, None).unwrap();
        for i in 0..count {
            let frame = GpuFrame {
                texture: input.clone(),
                slice: 0,
                size: config.size,
                timestamp_us: i * 33_333,
            };
            while !encoder.submit(frame.clone()) {
                std::thread::sleep(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(encoder);
        rx.try_iter().collect()
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder e decoder H.264"]
    fn decodifica_o_que_o_encoder_de_hardware_produziu() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig::for_layer(QualityLayer::Low);
        let frames = encode_frames(&gpu, config, 30);
        assert!(frames.len() >= 25, "só {} frames codificados", frames.len());

        let mut decoder = Decoder::new(&gpu, config.size, config.fps).expect("decoder");
        assert!(format!("{decoder:?}").contains("1280"));
        let mut decoded = Vec::new();
        for f in &frames {
            decoder
                .decode(&f.data, f.timestamp_us, |out| {
                    decoded.push((out.size, out.timestamp_us, out.slice));
                })
                .unwrap();
        }
        assert!(
            decoded.len() >= frames.len() - 3,
            "{} de {}",
            decoded.len(),
            frames.len()
        );
        assert!(decoded.iter().all(|(size, _, _)| *size == config.size));
        // Os timestamps atravessam o decoder.
        let first_ts = decoded[0].1;
        assert!(frames.iter().any(|f| f.timestamp_us == first_ts));
        // As amostras de entrada são reaproveitadas: nunca mais que o pool.
        assert!(decoder.inputs.len() <= INPUT_POOL);
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder e decoder H.264"]
    fn depois_do_flush_volta_a_decodificar_no_proximo_keyframe() {
        let gpu = Gpu::new().unwrap();
        let config = EncoderConfig::for_layer(QualityLayer::Low);
        let frames = encode_frames(&gpu, config, 10);
        let mut decoder = Decoder::new(&gpu, config.size, config.fps).unwrap();
        for f in &frames[..5] {
            decoder.decode(&f.data, f.timestamp_us, |_| {}).unwrap();
        }
        decoder.flush();
        // Recomeça pelo keyframe (o primeiro frame, que traz SPS/PPS).
        let mut out = 0;
        for f in &frames {
            decoder.decode(&f.data, f.timestamp_us, |_| out += 1).unwrap();
        }
        assert!(out > 0, "nada saiu depois do flush");
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder e decoder H.264"]
    fn troca_de_camada_no_meio_do_stream_muda_o_tamanho_da_imagem() {
        // Simulcast (ADR 0026): quem assiste passa da camada alta para a baixa num keyframe, com o
        // mesmo decoder. Antes, a imagem de 720p saía marcada como 1080p.
        let gpu = Gpu::new().unwrap();
        let high = EncoderConfig::for_layer(QualityLayer::High);
        let low = EncoderConfig::for_layer(QualityLayer::Low);
        let mut decoder = Decoder::new(&gpu, high.size, high.fps).unwrap();
        let mut sizes = Vec::new();
        for f in encode_frames(&gpu, high, 10)
            .iter()
            .chain(&encode_frames(&gpu, low, 10))
        {
            decoder
                .decode(&f.data, f.timestamp_us, |out| sizes.push(out.size))
                .unwrap();
        }
        assert_eq!(sizes.first(), Some(&high.size), "{sizes:?}");
        assert_eq!(sizes.last(), Some(&low.size), "{sizes:?}");
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com decoder H.264"]
    fn lixo_na_entrada_nao_derruba_o_decoder() {
        let gpu = Gpu::new().unwrap();
        let mut decoder = Decoder::new(&gpu, Size::new(640, 360), 30).unwrap();
        // Frame grande (força o buffer de entrada a crescer) e sem nada decodificável.
        let junk = vec![0x5a; 600 * 1024];
        let mut out = 0;
        let _ = decoder.decode(&junk, 0, |_| out += 1);
        assert_eq!(out, 0);
        assert!(decoder.inputs[0].2 >= 600 * 1024);
    }
}
