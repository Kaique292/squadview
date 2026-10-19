use std::collections::HashMap;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{RECT, TRUE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Resource,
    ID3D11Texture2D, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_TYPE, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_RATIONAL,
};
use windows::core::{Interface, Result};

use crate::{Gpu, Rect, Size};

/// Converte e redimensiona texturas na GPU com o `VideoProcessor` do D3D11
/// (ex.: BGRA da captura → NV12 do encoder, NV12 do decoder → BGRA da janela).
///
/// As views de entrada e saída ficam em cache por textura: a captura e o decoder
/// reaproveitam um pequeno conjunto de texturas, então não há alocação por frame.
#[derive(Debug)]
pub struct Converter {
    gpu: Gpu,
    input: Size,
    output: Size,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input_views: HashMap<(usize, u32), ID3D11VideoProcessorInputView>,
    output_views: HashMap<usize, ID3D11VideoProcessorOutputView>,
}

/// Limite de views em cache; acima disso o cache é limpo (as texturas mudaram).
const VIEW_CACHE_LIMIT: usize = 16;

impl Converter {
    pub fn new(gpu: &Gpu, input: Size, output: Size) -> Result<Self> {
        let (enumerator, processor) = create_processor(gpu, input, output)?;
        Ok(Self {
            gpu: gpu.clone(),
            input,
            output,
            enumerator,
            processor,
            input_views: HashMap::new(),
            output_views: HashMap::new(),
        })
    }

    /// Recria o processador se o tamanho da entrada ou da saída mudou (ex.: janela redimensionada).
    pub fn ensure_sizes(&mut self, input: Size, output: Size) -> Result<()> {
        if input != self.input || output != self.output {
            *self = Self::new(&self.gpu, input, output)?;
        }
        Ok(())
    }

    /// Converte `input` (fatia `input_slice` de um array de texturas, 0 se não for array)
    /// para `output`, desenhando em `target` (ou na textura inteira, se `None`).
    pub fn convert(
        &mut self,
        input: &ID3D11Texture2D,
        input_slice: u32,
        output: &ID3D11Texture2D,
        target: Option<Rect>,
    ) -> Result<()> {
        let input_format = texture_format(input);
        let output_format = texture_format(output);
        let input_view = self.input_view(input, input_slice)?;
        let output_view = self.output_view(output)?;

        let ctx = &self.gpu.video_context;
        let full = rect(Rect {
            left: 0,
            top: 0,
            right: self.output.width,
            bottom: self.output.height,
        });
        let dest = target.map(rect).unwrap_or(full);

        // SAFETY: todas as interfaces são válidas e pertencem ao mesmo device. A struct do stream
        // guarda uma referência extra à view de entrada, liberada logo depois do Blt.
        unsafe {
            ctx.VideoProcessorSetStreamColorSpace1(&self.processor, 0, color_space(input_format));
            ctx.VideoProcessorSetOutputColorSpace1(&self.processor, color_space(output_format));
            ctx.VideoProcessorSetStreamFrameFormat(&self.processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            ctx.VideoProcessorSetStreamSourceRect(
                &self.processor,
                0,
                true,
                Some(&rect(Rect {
                    left: 0,
                    top: 0,
                    right: self.input.width,
                    bottom: self.input.height,
                })),
            );
            ctx.VideoProcessorSetStreamDestRect(&self.processor, 0, true, Some(&dest));
            ctx.VideoProcessorSetOutputTargetRect(&self.processor, true, Some(&full));

            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: TRUE,
                pInputSurface: ManuallyDrop::new(Some(input_view)),
                ..Default::default()
            };
            let result = ctx.VideoProcessorBlt(&self.processor, &output_view, 0, std::slice::from_ref(&stream));
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result
        }
    }

    fn input_view(&mut self, texture: &ID3D11Texture2D, slice: u32) -> Result<ID3D11VideoProcessorInputView> {
        let key = (texture.as_raw() as usize, slice);
        if let Some(view) = self.input_views.get(&key) {
            return Ok(view.clone());
        }
        if self.input_views.len() >= VIEW_CACHE_LIMIT {
            self.input_views.clear();
        }
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: slice,
                },
            },
        };
        let mut view = None;
        let resource: ID3D11Resource = texture.cast()?;
        // SAFETY: recurso e enumerador válidos; ponteiro de saída vive até o fim da chamada.
        unsafe {
            self.gpu
                .video_device
                .CreateVideoProcessorInputView(&resource, &self.enumerator, &desc, Some(&mut view))?;
        }
        let view = view.expect("CreateVideoProcessorInputView sem view");
        self.input_views.insert(key, view.clone());
        Ok(view)
    }

    fn output_view(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11VideoProcessorOutputView> {
        let key = texture.as_raw() as usize;
        if let Some(view) = self.output_views.get(&key) {
            return Ok(view.clone());
        }
        if self.output_views.len() >= VIEW_CACHE_LIMIT {
            self.output_views.clear();
        }
        let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut view = None;
        let resource: ID3D11Resource = texture.cast()?;
        // SAFETY: recurso e enumerador válidos; ponteiro de saída vive até o fim da chamada.
        unsafe {
            self.gpu.video_device.CreateVideoProcessorOutputView(
                &resource,
                &self.enumerator,
                &desc,
                Some(&mut view),
            )?;
        }
        let view = view.expect("CreateVideoProcessorOutputView sem view");
        self.output_views.insert(key, view.clone());
        Ok(view)
    }
}

fn create_processor(
    gpu: &Gpu,
    input: Size,
    output: Size,
) -> Result<(ID3D11VideoProcessorEnumerator, ID3D11VideoProcessor)> {
    let rate = DXGI_RATIONAL {
        Numerator: 60,
        Denominator: 1,
    };
    let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: rate,
        InputWidth: input.width,
        InputHeight: input.height,
        OutputFrameRate: rate,
        OutputWidth: output.width,
        OutputHeight: output.height,
        Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
    };
    // SAFETY: chamadas COM com descritores válidos.
    unsafe {
        let enumerator = gpu.video_device.CreateVideoProcessorEnumerator(&desc)?;
        let processor = gpu.video_device.CreateVideoProcessor(&enumerator, 0)?;
        // Sem processamento automático (realce, redução de ruído): só converter, rápido.
        gpu.video_context
            .VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
        Ok((enumerator, processor))
    }
}

fn texture_format(texture: &ID3D11Texture2D) -> DXGI_FORMAT {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: leitura do descritor de uma textura válida.
    unsafe { texture.GetDesc(&mut desc) };
    desc.Format
}

fn color_space(format: DXGI_FORMAT) -> DXGI_COLOR_SPACE_TYPE {
    if format == DXGI_FORMAT_NV12 || format == DXGI_FORMAT_P010 {
        DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709
    } else {
        DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709
    }
}

fn rect(r: Rect) -> RECT {
    RECT {
        left: r.left as i32,
        top: r.top as i32,
        right: r.right as i32,
        bottom: r.bottom as i32,
    }
}
