//! GPU compartilhada pelo pipeline de mídia.
//!
//! Um único [`Gpu`] (device D3D11) é usado pela captura, pela conversão de formato e pelo
//! encoder/decoder do Media Foundation. Assim a imagem nunca sai da GPU.

pub mod clock;
mod convert;
mod device;
mod geometry;
pub mod mf;

pub use convert::Converter;
pub use device::Gpu;
pub use geometry::{Rect, Size, fit_inside, scaled_to_fit};

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

/// Um frame de vídeo que vive na GPU.
#[derive(Debug, Clone)]
pub struct GpuFrame {
    pub texture: ID3D11Texture2D,
    /// Índice da fatia quando a textura é um array (o decoder entrega assim); 0 caso contrário.
    pub slice: u32,
    pub size: Size,
    /// Momento em que o frame ficou disponível, em µs de [`clock::now_us`] no PC que capturou.
    pub timestamp_us: u64,
}
