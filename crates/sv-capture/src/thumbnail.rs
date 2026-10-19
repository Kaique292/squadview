//! Miniatura de uma fonte para o seletor: um frame, reduzido na GPU, copiado para a CPU.
//!
//! É o único lugar em que uma imagem sai da GPU, e só uma vez, pequena: quando o seletor abre e
//! na foto do último quadro ao parar um vídeo (para a animação de saída).

use std::sync::mpsc;
use std::time::Duration;

use sv_gpu::{Converter, Gpu, Size, fit_inside};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use crate::{CaptureSource, VideoCapture};

/// Imagem pequena em RGB (3 bytes por pixel, sem alfa), pronta para virar JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Tira uma miniatura de `source` que caiba em `max`. Devolve `None` se a fonte não mandar
/// nenhum frame a tempo (ex.: janela minimizada).
pub fn thumbnail(gpu: &Gpu, source: CaptureSource, max: Size, timeout: Duration) -> Option<Thumbnail> {
    let (tx, rx) = mpsc::sync_channel::<Option<Thumbnail>>(1);
    let thumb_gpu = gpu.clone();
    let mut done = false;
    let capture = VideoCapture::start(gpu, source, move |frame| {
        if done {
            return;
        }
        done = true;
        let _ = tx.try_send(read_frame(&thumb_gpu, frame, max).ok());
    })
    .ok()?;
    let result = rx.recv_timeout(timeout).ok().flatten();
    drop(capture);
    result
}

/// Reduz `frame` na GPU para caber em `max` e copia para a CPU. Use só em momentos pontuais
/// (miniatura, foto do último quadro ao parar), nunca a cada frame.
pub fn read_frame(gpu: &Gpu, frame: &sv_gpu::GpuFrame, max: Size) -> windows::core::Result<Thumbnail> {
    let rect = fit_inside(frame.size, max);
    let size = Size::new((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
    let target = texture(gpu, size, false)?;
    let staging = texture(gpu, size, true)?;
    Converter::new(gpu, frame.size, size)?.convert(&frame.texture, frame.slice, &target, None)?;

    // SAFETY: cópia entre texturas do mesmo device e leitura mapeada da textura de staging,
    // desmapeada antes de sair.
    unsafe {
        gpu.context.CopyResource(&staging, &target);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let pitch = mapped.RowPitch as usize;
        let data = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * size.height as usize);
        let rgb = bgra_rows_to_rgb(data, pitch, size.width as usize, size.height as usize);
        gpu.context.Unmap(&staging, 0);
        Ok(Thumbnail {
            width: size.width,
            height: size.height,
            rgb,
        })
    }
}

/// Converte linhas BGRA (com `pitch` bytes por linha, que pode ter sobra) em RGB contínuo.
pub(crate) fn bgra_rows_to_rgb(data: &[u8], pitch: usize, width: usize, height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width * height * 3);
    for row in data.chunks(pitch).take(height) {
        for px in row[..width * 4].chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    rgb
}

fn texture(gpu: &Gpu, size: Size, staging: bool) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.width,
        Height: size.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: if staging {
            D3D11_USAGE_STAGING
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: if staging { 0 } else { D3D11_BIND_RENDER_TARGET.0 as u32 },
        CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
        ..Default::default()
    };
    let mut texture = None;
    // SAFETY: descritor válido e ponteiro de saída vivo.
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    Ok(texture.expect("CreateTexture2D sem textura"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list_sources;

    #[test]
    fn converte_bgra_com_sobra_no_fim_da_linha_para_rgb() {
        // 2x2 pixels, pitch de 12 bytes (4 de sobra por linha).
        let data = [
            1, 2, 3, 255, 4, 5, 6, 255, 9, 9, 9, 9, //
            7, 8, 9, 255, 10, 11, 12, 255, 9, 9, 9, 9,
        ];
        assert_eq!(
            bgra_rows_to_rgb(&data, 12, 2, 2),
            vec![3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop"]
    fn tira_miniatura_do_monitor_principal() {
        let gpu = Gpu::new().unwrap();
        let monitor = list_sources()
            .into_iter()
            .find(|s| matches!(s.id, CaptureSource::Monitor(_)))
            .unwrap();
        let thumb = thumbnail(&gpu, monitor.id, Size::new(320, 180), Duration::from_secs(2)).expect("miniatura");
        assert!(thumb.width <= 320 && thumb.height <= 180);
        assert_eq!(thumb.rgb.len(), (thumb.width * thumb.height * 3) as usize);
        assert!(thumb.rgb.iter().any(|&b| b > 0), "a miniatura saiu toda preta");
    }
}
