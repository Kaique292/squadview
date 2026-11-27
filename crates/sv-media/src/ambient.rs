//! A fonte da luz ambiente (ADR 0034): o renderer da live em foco desenha, além da live, uma
//! imagem minúscula dela (32 × 18) num swap chain próprio, que o compositor estica até a sala.
//!
//! Tudo fica na GPU: o `VideoProcessor` reduz o quadro para 256 × 144, o `GenerateMips` tira a
//! média até 32 × 18 e a cópia vai para o swap chain minúsculo. Nada volta para a CPU, e as views
//! e texturas são criadas uma vez só. O desfoque do próprio compositor, que seria o caminho óbvio,
//! custava ~16% da GPU (medido com `examples/luz_ambiente.rs`); assim, o custo some no ruído.

use std::sync::Arc;

use sv_gpu::{Converter, Gpu, GpuFrame, Size};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_GENERATE_MIPS, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2, IDXGISwapChain1,
};
use windows::core::Result;

use crate::composition::{AMBIENT_SIZE, Layer};

/// O primeiro passo da redução, feito pelo `VideoProcessor` (com filtro): 8 × a imagem final.
const REDUCED: Size = Size::new(AMBIENT_SIZE.width * 8, AMBIENT_SIZE.height * 8);
/// Quantas metades o `GenerateMips` tira de [`REDUCED`] até chegar a [`AMBIENT_SIZE`] (8 = 2³).
const MIP_LEVEL: u32 = 3;
/// A luz é atualizada a cada 6 quadros (10 vezes por segundo numa live de 60 fps): para uma luz
/// difusa, parece contínuo. Medido no caminho do app (`examples/luz_ambiente.rs app`): a cada 2
/// quadros, a luz somava 3 a 5 pontos de GPU; a redução da imagem cheia é o que pesa.
const EVERY: u32 = 6;

/// O que a live fonte usa para desenhar a luz. Ao ser descartada, a luz some (se ainda é dela).
pub(crate) struct AmbientFeed {
    gpu: Gpu,
    layer: Arc<Layer>,
    swapchain: IDXGISwapChain1,
    reduced: ID3D11Texture2D,
    mips: ID3D11ShaderResourceView,
    converter: Option<Converter>,
    frames: u32,
}

impl std::fmt::Debug for AmbientFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmbientFeed").field("frames", &self.frames).finish()
    }
}

impl AmbientFeed {
    /// Cria o swap chain minúsculo e a textura das reduções, e põe a luz na janela do app.
    pub fn new(gpu: &Gpu, layer: &Arc<Layer>) -> Result<Self> {
        // SAFETY: fábrica DXGI, swap chain de composição e textura no dispositivo da GPU.
        let (swapchain, reduced, mips) = unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let swapchain = factory.CreateSwapChainForComposition(
                &gpu.device,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: AMBIENT_SIZE.width,
                    Height: AMBIENT_SIZE.height,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    ..Default::default()
                },
                None,
            )?;
            let desc = D3D11_TEXTURE2D_DESC {
                Width: REDUCED.width,
                Height: REDUCED.height,
                MipLevels: MIP_LEVEL + 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            };
            let mut reduced = None;
            gpu.device.CreateTexture2D(&desc, None, Some(&mut reduced))?;
            let reduced: ID3D11Texture2D = reduced.ok_or_else(windows::core::Error::from_thread)?;
            let mut mips = None;
            gpu.device.CreateShaderResourceView(&reduced, None, Some(&mut mips))?;
            (swapchain, reduced, mips.ok_or_else(windows::core::Error::from_thread)?)
        };
        layer.show_ambient(&swapchain)?;
        Ok(Self {
            gpu: gpu.clone(),
            layer: layer.clone(),
            swapchain,
            reduced,
            mips,
            converter: None,
            frames: 0,
        })
    }

    /// Mais um quadro da live: a cada [`EVERY`], a luz é atualizada com ele.
    pub fn update(&mut self, frame: &GpuFrame) -> Result<()> {
        self.frames = self.frames.wrapping_add(1);
        if !self.frames.is_multiple_of(EVERY) {
            return Ok(());
        }
        let converter = match &mut self.converter {
            Some(c) => {
                c.ensure_sizes(frame.size, REDUCED)?;
                c
            }
            None => self.converter.insert(Converter::new(&self.gpu, frame.size, REDUCED)?),
        };
        // A imagem inteira, esticada para 16:9: para a luz, a proporção não importa.
        converter.convert(&frame.texture, frame.slice, &self.reduced, None)?;
        // SAFETY: textura, view e swap chain vivos, no mesmo dispositivo.
        unsafe {
            self.gpu.context.GenerateMips(&self.mips);
            let back: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            self.gpu
                .context
                .CopySubresourceRegion(&back, 0, 0, 0, 0, &self.reduced, MIP_LEVEL, None);
            self.swapchain.Present(0, DXGI_PRESENT(0)).ok()
        }
    }
}

impl Drop for AmbientFeed {
    fn drop(&mut self) {
        self.layer.hide_ambient(&self.swapchain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reducao_chega_exatamente_ao_tamanho_da_luz() {
        assert_eq!(
            Size::new(REDUCED.width >> MIP_LEVEL, REDUCED.height >> MIP_LEVEL),
            AMBIENT_SIZE
        );
    }
}
