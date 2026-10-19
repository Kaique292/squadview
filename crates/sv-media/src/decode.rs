//! Decoder H.264 por GPU via Media Foundation (DXVA). A saída fica na GPU como textura NV12.

use std::fmt;
use std::mem::ManuallyDrop;

use sv_gpu::{Gpu, GpuFrame, Size, mf as mfu};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_SUBTYPE, MF_SA_D3D11_AWARE, MFCreateMemoryBuffer,
    MFCreateSample, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoFormat_H264,
    MFVideoFormat_NV12,
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
            sample.SetSampleTime(timestamp_us as i64 * 10)?;
            self.transform.ProcessInput(0, &sample, 0)?;
        }
        loop {
            match self.next_output()? {
                Output::Frame(frame) => on_frame(&frame),
                Output::NeedMoreInput => return Ok(()),
                Output::FormatChanged => self.choose_output_type()?,
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
        if self.inputs.get(i).is_none_or(|(_, _, cap)| *cap < needed) {
            // Cresce em degraus para não realocar a cada keyframe um pouco maior.
            let capacity = needed.next_power_of_two().max(256 * 1024);
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
                sample.AddBuffer(&MFCreateMemoryBuffer(self.size.width * self.size.height * 3 / 2)?)?;
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
            let timestamp_us = (sample.GetSampleTime().unwrap_or(0) / 10).max(0) as u64;
            Ok(Output::Frame(GpuFrame {
                texture,
                slice,
                size: self.size,
                timestamp_us,
            }))
        }
    }

    fn choose_output_type(&mut self) -> Result<()> {
        // SAFETY: enumeração dos tipos de saída oferecidos pelo MFT.
        unsafe {
            for i in 0.. {
                let t = self.transform.GetOutputAvailableType(0, i)?;
                if t.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                    // O decoder pode alinhar a altura (1080 → 1088). A área visível continua sendo
                    // o tamanho do stream (`self.size`), então o tamanho do tipo de saída é ignorado.
                    self.transform.SetOutputType(0, &t, 0)?;
                    break;
                }
            }
            let info = self.transform.GetOutputStreamInfo(0)?;
            self.provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        }
        Ok(())
    }
}

enum Output {
    Frame(GpuFrame),
    NeedMoreInput,
    FormatChanged,
}
