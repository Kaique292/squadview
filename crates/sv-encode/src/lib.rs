//! Codificação de vídeo por hardware.
//!
//! Todo encoder implementa [`Encoder`], para o resto do app não depender do fabricante
//! da placa. NVENC entra na Fase 1 e AMF na Fase 5 (ver `docs/adr/0004-abstracao-de-encoder.md`).

use sv_core::QualityLayer;

/// Fabricante do encoder de hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// NVIDIA NVENC.
    Nvenc,
    /// AMD AMF.
    Amf,
}

/// Codec de saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

/// Configuração de uma sessão de encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderConfig {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl EncoderConfig {
    /// Configuração padrão de cada camada de qualidade, em H.264 para máxima compatibilidade.
    pub const fn for_layer(layer: QualityLayer) -> Self {
        let (width, height, fps) = match layer {
            QualityLayer::Low => (1280, 720, 30),
            QualityLayer::High => (1920, 1080, 60),
        };
        Self {
            codec: Codec::H264,
            width,
            height,
            fps,
            bitrate_kbps: layer.target_kbps(),
        }
    }
}

/// Um frame já codificado, pronto para ir para a rede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub timestamp_us: u64,
}

/// Contrato comum dos encoders de hardware.
pub trait Encoder {
    type Error: std::error::Error;

    /// Fabricante deste encoder.
    fn backend(&self) -> Backend;

    /// Pede que o próximo frame seja um keyframe (alguém entrou na sala ou perdeu pacotes demais).
    fn request_keyframe(&mut self);

    /// Devolve os frames que o encoder terminou de codificar.
    fn drain(&mut self) -> Result<Vec<EncodedFrame>, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camada_alta_e_1080p60() {
        let cfg = EncoderConfig::for_layer(QualityLayer::High);
        assert_eq!((cfg.width, cfg.height, cfg.fps), (1920, 1080, 60));
        assert_eq!(cfg.bitrate_kbps, QualityLayer::High.target_kbps());
    }

    #[test]
    fn camada_baixa_e_720p30() {
        let cfg = EncoderConfig::for_layer(QualityLayer::Low);
        assert_eq!((cfg.width, cfg.height, cfg.fps), (1280, 720, 30));
    }
}
