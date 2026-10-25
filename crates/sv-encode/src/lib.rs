//! Codificação de vídeo por hardware.
//!
//! Todo encoder implementa [`Encoder`], para o resto do app não depender do fabricante da placa.
//! A primeira implementação usa os MFTs de hardware do Media Foundation, que por baixo chamam
//! NVENC, AMF ou QuickSync (ver `docs/adr/0007-media-foundation-para-encode-e-decode.md`).

pub mod annexb;
mod mf;

pub use mf::MfEncoder;
use sv_core::{QualityLayer, StreamQuality};
use sv_gpu::{GpuFrame, Size};

/// Quem faz a codificação de verdade, descoberto pelo nome do MFT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// NVIDIA NVENC.
    Nvenc,
    /// AMD AMF.
    Amf,
    /// Intel QuickSync.
    QuickSync,
    /// Outro encoder de hardware.
    Other,
}

impl Backend {
    /// Deduz o fabricante pelo nome amigável do MFT (ex.: "NVIDIA H.264 Encoder MFT").
    pub fn from_mft_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("nvidia") {
            Self::Nvenc
        } else if lower.contains("amd") || lower.contains("amf") {
            Self::Amf
        } else if lower.contains("intel") || lower.contains("quick sync") {
            Self::QuickSync
        } else {
            Self::Other
        }
    }
}

/// Configuração de uma sessão de encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub size: Size,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl EncoderConfig {
    /// Tamanho máximo, fps e bitrate de cada camada de qualidade.
    pub const fn for_layer(layer: QualityLayer) -> Self {
        let (width, height, fps) = match layer {
            QualityLayer::Low => (1280, 720, 30),
            QualityLayer::High => (1920, 1080, 60),
            QualityLayer::Ultra => (2560, 1440, 60),
        };
        Self {
            size: Size::new(width, height),
            fps,
            bitrate_kbps: layer.target_kbps(),
        }
    }

    /// Configuração para a qualidade escolhida pelo usuário, com a fonte de `source` reduzida
    /// para caber na resolução (nunca aumentada) e o bitrate calculado para o tamanho final.
    pub fn for_quality(quality: StreamQuality, source: Size) -> Self {
        let (w, h) = quality.resolution.max_box();
        let size = sv_gpu::scaled_to_fit(source, Size::new(w, h));
        Self {
            size,
            fps: quality.fps(),
            bitrate_kbps: quality.bitrate_kbps(size.width, size.height),
        }
    }

    /// A camada baixa do simulcast (ADR 0026) para quem transmite em `self`: 720p30 com a mesma
    /// proporção da camada principal. `None` quando a principal já é tão leve quanto ela (não
    /// adianta um segundo encoder para mandar a mesma coisa).
    pub fn low_layer(self) -> Option<Self> {
        let low = Self::for_layer(QualityLayer::Low);
        if self.size.height <= low.size.height && self.fps <= low.fps {
            return None;
        }
        let size = sv_gpu::scaled_to_fit(self.size, low.size);
        Some(Self {
            size,
            fps: low.fps.min(self.fps),
            bitrate_kbps: low.bitrate_kbps.min(self.bitrate_kbps),
        })
    }

    /// A mesma camada, mas com o tamanho real da fonte reduzido para caber no máximo dela.
    pub fn fitted_to(self, source: Size) -> Self {
        Self {
            size: sv_gpu::scaled_to_fit(source, self.size),
            ..self
        }
    }
}

/// Um frame já codificado (H.264 Annex B), pronto para ir para a rede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Momento da captura (mesmo relógio de [`GpuFrame::timestamp_us`]).
    pub timestamp_us: u64,
    /// Quanto o encoder demorou, da entrega do frame até a saída.
    pub encode_us: u32,
}

/// Contrato comum dos encoders de hardware.
///
/// O encoder roda na própria thread: [`Encoder::submit`] entrega um frame (sem bloquear) e cada
/// frame codificado sai pelo callback passado na criação, assim que fica pronto.
pub trait Encoder: Send {
    /// Quem está codificando (NVENC, AMF...).
    fn backend(&self) -> Backend;

    /// Entrega um frame NV12 do tamanho configurado. Devolve `false` se o encoder ainda estava
    /// ocupado com o anterior e o frame foi descartado (melhor perder um frame que atrasar todos).
    fn submit(&self, frame: GpuFrame) -> bool;

    /// Pede que o próximo frame seja um keyframe (alguém entrou na sala ou perdeu um frame).
    fn request_keyframe(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camada_alta_e_1080p60() {
        let cfg = EncoderConfig::for_layer(QualityLayer::High);
        assert_eq!((cfg.size, cfg.fps), (Size::new(1920, 1080), 60));
        assert_eq!(cfg.bitrate_kbps, QualityLayer::High.target_kbps());
    }

    #[test]
    fn camada_baixa_e_720p30() {
        let cfg = EncoderConfig::for_layer(QualityLayer::Low);
        assert_eq!((cfg.size, cfg.fps), (Size::new(1280, 720), 30));
    }

    #[test]
    fn camada_baixa_do_simulcast_e_720p30_na_proporcao_da_principal() {
        let main = EncoderConfig::for_layer(QualityLayer::High);
        let low = main.low_layer().expect("1080p60 tem camada baixa");
        assert_eq!((low.size, low.fps), (Size::new(1280, 720), 30));
        // Ultrawide: a baixa segue a proporção da principal.
        let uw = EncoderConfig::for_layer(QualityLayer::High).fitted_to(Size::new(3440, 1440));
        let low = uw.low_layer().unwrap();
        assert_eq!(low.size.width, 1280);
        assert!(low.size.height < 720);
        // 720p60: a baixa só corta os fps.
        let p720 = EncoderConfig {
            fps: 60,
            ..EncoderConfig::for_layer(QualityLayer::Low)
        };
        assert_eq!(
            p720.low_layer().map(|l| (l.size, l.fps)),
            Some((Size::new(1280, 720), 30))
        );
        // Já em 720p30 (ou menos), não há camada baixa.
        assert_eq!(EncoderConfig::for_layer(QualityLayer::Low).low_layer(), None);
        let small = EncoderConfig {
            size: Size::new(960, 540),
            fps: 15,
            bitrate_kbps: 1_500,
        };
        assert_eq!(small.low_layer(), None);
    }

    #[test]
    fn monitor_ultrawide_mantem_a_proporcao() {
        let cfg = EncoderConfig::for_layer(QualityLayer::High).fitted_to(Size::new(3440, 1440));
        assert_eq!(cfg.size.width, 1920);
        assert!(cfg.size.height < 1080);
    }

    #[test]
    fn qualidade_1440p_encaixa_a_fonte_pela_altura() {
        use sv_core::Resolution;
        let q = StreamQuality {
            resolution: Resolution::P1440,
            fps: 30,
        };
        // Monitor 4K: reduz para 2560x1440.
        let cfg = EncoderConfig::for_quality(q, Size::new(3840, 2160));
        assert_eq!((cfg.size, cfg.fps), (Size::new(2560, 1440), 30));
        assert_eq!(cfg.bitrate_kbps, q.bitrate_kbps(2560, 1440));
        // Monitor 1080p: não aumenta.
        assert_eq!(
            EncoderConfig::for_quality(q, Size::new(1920, 1080)).size,
            Size::new(1920, 1080)
        );
        // Ultrawide 3440x1440 em 720p: limita pela altura.
        let uw = EncoderConfig::for_quality(
            StreamQuality {
                resolution: Resolution::P720,
                fps: 60,
            },
            Size::new(3440, 1440),
        );
        assert_eq!(uw.size.height, 720);
    }

    #[test]
    fn fonte_original_respeita_o_limite_do_h264() {
        let q = StreamQuality {
            resolution: sv_core::Resolution::Source,
            fps: 60,
        };
        let cfg = EncoderConfig::for_quality(q, Size::new(5120, 2880));
        assert!(cfg.size.width <= 4096 && cfg.size.height <= 2304);
    }

    #[test]
    fn reconhece_o_fabricante_pelo_nome_do_mft() {
        assert_eq!(Backend::from_mft_name("NVIDIA H.264 Encoder MFT"), Backend::Nvenc);
        assert_eq!(Backend::from_mft_name("AMDh264Encoder"), Backend::Amf);
        assert_eq!(
            Backend::from_mft_name("Intel® Quick Sync Video H.264 Encoder MFT"),
            Backend::QuickSync
        );
        assert_eq!(Backend::from_mft_name("Qualquer"), Backend::Other);
    }
}
