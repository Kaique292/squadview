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

use crate::{BoundedCache, Gpu, Rect, Size};

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
    input_views: BoundedCache<(usize, u32), ID3D11VideoProcessorInputView>,
    output_views: BoundedCache<usize, ID3D11VideoProcessorOutputView>,
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
            input_views: BoundedCache::new(VIEW_CACHE_LIMIT),
            output_views: BoundedCache::new(VIEW_CACHE_LIMIT),
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
        let full = rect(whole(self.output));
        let dest = target.map(rect).unwrap_or(full);

        // SAFETY: todas as interfaces são válidas e pertencem ao mesmo device. A struct do stream
        // guarda uma referência extra à view de entrada, liberada logo depois do Blt.
        unsafe {
            ctx.VideoProcessorSetStreamColorSpace1(&self.processor, 0, color_space(input_format));
            ctx.VideoProcessorSetOutputColorSpace1(&self.processor, color_space(output_format));
            ctx.VideoProcessorSetStreamFrameFormat(&self.processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            ctx.VideoProcessorSetStreamSourceRect(&self.processor, 0, true, Some(&rect(whole(self.input))));
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
        let (gpu, enumerator) = (&self.gpu, &self.enumerator);
        self.input_views
            .get_or_try_insert((texture.as_raw() as usize, slice), || {
                let desc = input_view_desc(slice);
                let mut view = None;
                let resource: ID3D11Resource = texture.cast()?;
                // SAFETY: recurso e enumerador válidos; ponteiro de saída vive até o fim da chamada.
                unsafe {
                    gpu.video_device
                        .CreateVideoProcessorInputView(&resource, enumerator, &desc, Some(&mut view))?;
                }
                Ok(view.expect("CreateVideoProcessorInputView sem view"))
            })
    }

    fn output_view(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11VideoProcessorOutputView> {
        let (gpu, enumerator) = (&self.gpu, &self.enumerator);
        self.output_views.get_or_try_insert(texture.as_raw() as usize, || {
            let desc = output_view_desc();
            let mut view = None;
            let resource: ID3D11Resource = texture.cast()?;
            // SAFETY: recurso e enumerador válidos; ponteiro de saída vive até o fim da chamada.
            unsafe {
                gpu.video_device
                    .CreateVideoProcessorOutputView(&resource, enumerator, &desc, Some(&mut view))?;
            }
            Ok(view.expect("CreateVideoProcessorOutputView sem view"))
        })
    }

    /// Quantas views de entrada e de saída estão em cache (para os testes).
    #[cfg(test)]
    fn cached_views(&self) -> (usize, usize) {
        (self.input_views.len(), self.output_views.len())
    }
}

fn input_view_desc(slice: u32) -> D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
        FourCC: 0,
        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_VPIV {
                MipSlice: 0,
                ArraySlice: slice,
            },
        },
    }
}

fn output_view_desc() -> D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
        },
    }
}

/// Descrição do conteúdo para o enumerador: progressivo a 60 fps, otimizado para velocidade.
fn content_desc(input: Size, output: Size) -> D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
    let rate = DXGI_RATIONAL {
        Numerator: 60,
        Denominator: 1,
    };
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: rate,
        InputWidth: input.width,
        InputHeight: input.height,
        OutputFrameRate: rate,
        OutputWidth: output.width,
        OutputHeight: output.height,
        Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
    }
}

/// A imagem inteira de `size`.
fn whole(size: Size) -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: size.width,
        bottom: size.height,
    }
}

fn create_processor(
    gpu: &Gpu,
    input: Size,
    output: Size,
) -> Result<(ID3D11VideoProcessorEnumerator, ID3D11VideoProcessor)> {
    let desc = content_desc(input, output);
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

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    use super::*;
    use crate::fit_inside;
    use crate::testing::{read_back, solid_bgra, texture};

    fn perto(valor: u8, esperado: u8, folga: u8) -> bool {
        valor.abs_diff(esperado) <= folga
    }

    /// Vermelho puro em BGRA vira o vermelho do BT.709 com faixa de estúdio no NV12.
    fn bgra_vermelho_vira_nv12(gpu: &Gpu) {
        let size = Size::new(64, 64);
        let input = solid_bgra(gpu, size, [0, 0, 255, 255]).unwrap();
        let output = texture(gpu, size, DXGI_FORMAT_NV12, None).unwrap();
        let mut conv = Converter::new(gpu, size, size).unwrap();
        conv.convert(&input, 0, &output, None).unwrap();

        let nv12 = read_back(gpu, &output).unwrap();
        let (y, uv) = nv12.split_at(64 * 64);
        // Y = 16 + 219 × 0,2126 ≈ 63; Cb ≈ 102; Cr = 240.
        assert!(y.iter().all(|&v| perto(v, 63, 6)), "Y {:?}", &y[..4]);
        assert!(
            uv.chunks_exact(2).all(|c| perto(c[0], 102, 6) && perto(c[1], 240, 6)),
            "UV {:?}",
            &uv[..4]
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn converte_bgra_da_captura_em_nv12_do_encoder() {
        bgra_vermelho_vira_nv12(&Gpu::new().unwrap());
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn nv12_do_decoder_vira_bgra_centralizado_com_barras_pretas() {
        let gpu = Gpu::new().unwrap();
        let video = Size::new(64, 32);
        let window = Size::new(64, 64);
        // Branco no NV12 de estúdio: Y = 235, U = V = 128.
        let mut data = vec![235u8; 64 * 32];
        data.extend(std::iter::repeat_n(128u8, 64 * 16));
        let input = texture(&gpu, video, DXGI_FORMAT_NV12, Some(&data)).unwrap();
        let output = texture(&gpu, window, DXGI_FORMAT_B8G8R8A8_UNORM, None).unwrap();
        let target = fit_inside(video, window);
        assert_eq!((target.top, target.bottom), (16, 48));

        let mut conv = Converter::new(&gpu, video, window).unwrap();
        conv.convert(&input, 0, &output, Some(target)).unwrap();

        let bgra = read_back(&gpu, &output).unwrap();
        let pixel = |x: usize, y: usize| &bgra[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
        assert!(pixel(32, 32).iter().all(|&c| c >= 245), "meio {:?}", pixel(32, 32));
        assert!(
            pixel(32, 2).iter().all(|&c| c <= 10),
            "barra de cima {:?}",
            pixel(32, 2)
        );
        assert!(
            pixel(32, 61).iter().all(|&c| c <= 10),
            "barra de baixo {:?}",
            pixel(32, 61)
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn reduz_a_imagem_mantendo_a_cor() {
        let gpu = Gpu::new().unwrap();
        let input = solid_bgra(&gpu, Size::new(128, 96), [200, 100, 50, 255]).unwrap();
        let output = texture(&gpu, Size::new(32, 24), DXGI_FORMAT_B8G8R8A8_UNORM, None).unwrap();
        Converter::new(&gpu, Size::new(128, 96), Size::new(32, 24))
            .unwrap()
            .convert(&input, 0, &output, None)
            .unwrap();
        let bgra = read_back(&gpu, &output).unwrap();
        assert_eq!(bgra.len(), 32 * 24 * 4);
        assert!(
            bgra.chunks_exact(4)
                .all(|p| perto(p[0], 200, 3) && perto(p[1], 100, 3) && perto(p[2], 50, 3)),
            "{:?}",
            &bgra[..4]
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn views_ficam_em_cache_e_o_cache_tem_limite() {
        let gpu = Gpu::new().unwrap();
        let size = Size::new(16, 16);
        let input = solid_bgra(&gpu, size, [1, 2, 3, 255]).unwrap();
        let outputs: Vec<_> = (0..=VIEW_CACHE_LIMIT)
            .map(|_| texture(&gpu, size, DXGI_FORMAT_NV12, None).unwrap())
            .collect();
        let mut conv = Converter::new(&gpu, size, size).unwrap();

        // A mesma textura várias vezes: uma view de cada lado, sem criar outras por frame.
        for _ in 0..5 {
            conv.convert(&input, 0, &outputs[0], None).unwrap();
        }
        assert_eq!(conv.cached_views(), (1, 1));

        // Mais texturas de saída que o limite: o cache recomeça em vez de crescer sem fim.
        for out in &outputs {
            conv.convert(&input, 0, out, None).unwrap();
        }
        assert_eq!(conv.cached_views(), (1, 1));
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn recria_o_processador_so_quando_o_tamanho_muda() {
        let gpu = Gpu::new().unwrap();
        let small = Size::new(16, 16);
        let big = Size::new(32, 32);
        let input = solid_bgra(&gpu, small, [0, 0, 0, 255]).unwrap();
        let output = texture(&gpu, small, DXGI_FORMAT_NV12, None).unwrap();
        let mut conv = Converter::new(&gpu, small, small).unwrap();
        conv.convert(&input, 0, &output, None).unwrap();

        conv.ensure_sizes(small, small).unwrap();
        assert_eq!(conv.cached_views(), (1, 1), "mesmo tamanho: mantém tudo");

        conv.ensure_sizes(small, big).unwrap();
        assert_eq!(conv.cached_views(), (0, 0), "tamanho novo: processador e views novos");
        let big_out = texture(&gpu, big, DXGI_FORMAT_NV12, None).unwrap();
        conv.convert(&input, 0, &big_out, None).unwrap();
        assert!(format!("{conv:?}").contains("Converter"));
    }

    #[test]
    fn espaco_de_cor_segue_o_formato() {
        assert_eq!(
            color_space(DXGI_FORMAT_NV12),
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709
        );
        assert_eq!(
            color_space(DXGI_FORMAT_P010),
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709
        );
        assert_eq!(
            color_space(DXGI_FORMAT_B8G8R8A8_UNORM),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709
        );
    }

    #[test]
    fn descritores_do_processador() {
        let r = rect(whole(Size::new(1920, 1080)));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 0, 1920, 1080));

        let d = content_desc(Size::new(3840, 2160), Size::new(1920, 1080));
        assert_eq!((d.InputWidth, d.InputHeight), (3840, 2160));
        assert_eq!((d.OutputWidth, d.OutputHeight), (1920, 1080));
        assert_eq!(d.InputFrameRate.Numerator, 60);
        assert_eq!(d.Usage, D3D11_VIDEO_USAGE_OPTIMAL_SPEED);

        let input = input_view_desc(3);
        assert_eq!(input.ViewDimension, D3D11_VPIV_DIMENSION_TEXTURE2D);
        // SAFETY: a união foi montada com o campo Texture2D.
        assert_eq!(unsafe { input.Anonymous.Texture2D.ArraySlice }, 3);
        assert_eq!(output_view_desc().ViewDimension, D3D11_VPOV_DIMENSION_TEXTURE2D);
    }
}
