//! Ajudantes para testes: criar texturas com conteúdo conhecido e ler o resultado de volta.
//!
//! Ler de volta copia a imagem para a CPU, o que o pipeline nunca faz. Por isso este módulo só
//! existe nos testes (`cfg(test)` ou a feature `testing`, ligada só em `dev-dependencies`).

use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_FLAG, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC,
};
use windows::core::Result;

use crate::{Gpu, Size};

/// Bytes por linha e número de linhas (somando os planos) de uma imagem compacta.
pub fn layout(format: DXGI_FORMAT, size: Size) -> (usize, usize) {
    let (w, h) = (size.width as usize, size.height as usize);
    if format == DXGI_FORMAT_NV12 {
        // Plano Y (h linhas) seguido do plano UV intercalado (h/2 linhas da mesma largura).
        (w, h + h / 2)
    } else {
        (w * 4, h)
    }
}

/// Textura que o VideoProcessor aceita como entrada e saída, opcionalmente preenchida com
/// `data` compacto (sem sobra no fim das linhas, ver [`layout`]).
pub fn texture(gpu: &Gpu, size: Size, format: DXGI_FORMAT, data: Option<&[u8]>) -> Result<ID3D11Texture2D> {
    let bind: D3D11_BIND_FLAG = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.width,
        Height: size.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind.0 as u32,
        ..Default::default()
    };
    let (pitch, rows) = layout(format, size);
    if let Some(data) = data {
        assert_eq!(data.len(), pitch * rows, "dados com tamanho errado para a textura");
    }
    let init = data.map(|d| D3D11_SUBRESOURCE_DATA {
        pSysMem: d.as_ptr().cast(),
        SysMemPitch: pitch as u32,
        SysMemSlicePitch: 0,
    });
    let mut texture = None;
    // SAFETY: descritor válido; `init` aponta para `data`, que vive até o fim da chamada.
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, init.as_ref().map(|i| i as *const _), Some(&mut texture))?
    };
    Ok(texture.expect("CreateTexture2D sem textura"))
}

/// Textura BGRA de uma cor só.
pub fn solid_bgra(gpu: &Gpu, size: Size, bgra: [u8; 4]) -> Result<ID3D11Texture2D> {
    let data: Vec<u8> = (0..size.width * size.height).flat_map(|_| bgra).collect();
    texture(gpu, size, DXGI_FORMAT_B8G8R8A8_UNORM, Some(&data))
}

/// Copia a textura para a CPU e devolve os pixels compactos (ver [`layout`]).
pub fn read_back(gpu: &Gpu, source: &ID3D11Texture2D) -> Result<Vec<u8>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: leitura do descritor de uma textura válida.
    unsafe { source.GetDesc(&mut desc) };
    let size = Size::new(desc.Width, desc.Height);
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..desc
    };
    let mut staging = None;
    // SAFETY: descritor válido e ponteiro de saída vivo.
    unsafe { gpu.device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
    let staging = staging.expect("CreateTexture2D sem textura");
    let (row_bytes, rows) = layout(desc.Format, size);
    let mut out = Vec::with_capacity(row_bytes * rows);
    // SAFETY: cópia entre texturas do mesmo device; a leitura fica entre Map e Unmap e respeita o
    // RowPitch informado pelo driver (os planos do NV12 ficam um depois do outro com o mesmo pitch).
    unsafe {
        gpu.context.CopyResource(&staging, source);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let pitch = mapped.RowPitch as usize;
        let data = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * rows);
        for row in data.chunks(pitch).take(rows) {
            out.extend_from_slice(&row[..row_bytes]);
        }
        gpu.context.Unmap(&staging, 0);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_do_nv12_soma_os_dois_planos() {
        assert_eq!(layout(DXGI_FORMAT_NV12, Size::new(64, 32)), (64, 48));
        assert_eq!(layout(DXGI_FORMAT_B8G8R8A8_UNORM, Size::new(64, 32)), (256, 32));
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn textura_bgra_volta_igual_da_gpu() {
        let gpu = Gpu::new().unwrap();
        let tex = solid_bgra(&gpu, Size::new(8, 4), [10, 20, 30, 255]).unwrap();
        let back = read_back(&gpu, &tex).unwrap();
        assert_eq!(back.len(), 8 * 4 * 4);
        assert!(back.chunks_exact(4).all(|p| p == [10, 20, 30, 255]));
    }
}
