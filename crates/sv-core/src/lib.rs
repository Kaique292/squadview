//! Tipos comuns do SquadView, compartilhados por todos os crates.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Identificador de um membro da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub u64);

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer-{:016x}", self.0)
    }
}

/// Camada de qualidade de um stream. Quem tem internet fraca recebe a camada baixa
/// sem derrubar a qualidade dos outros membros da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityLayer {
    /// 720p30, para conexões fracas.
    Low,
    /// 1080p60, a qualidade padrão.
    High,
}

impl QualityLayer {
    /// Bitrate alvo da camada, em kbit/s.
    pub const fn target_kbps(self) -> u32 {
        match self {
            Self::Low => 2_500,
            Self::High => 8_000,
        }
    }
}

/// Resolução escolhida para transmitir (pela altura, como no Discord). O vídeo nunca é aumentado:
/// uma janela de 900 px de altura em "1080p" sai com 900 px.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Resolution {
    #[serde(rename = "720")]
    P720,
    #[serde(rename = "1080")]
    P1080,
    #[serde(rename = "1440")]
    P1440,
    /// O tamanho original da fonte (até 4K).
    #[serde(rename = "source")]
    Source,
}

impl Resolution {
    /// Caixa em que a fonte é encaixada. A largura é folgada para monitores ultrawide
    /// limitarem pela altura, como o usuário espera.
    pub const fn max_box(self) -> (u32, u32) {
        match self {
            Self::P720 => (720 * 4, 720),
            Self::P1080 => (1080 * 4, 1080),
            Self::P1440 => (1440 * 4, 1440),
            // Limite do H.264 nos encoders de hardware.
            Self::Source => (4096, 2304),
        }
    }
}

/// Qualidade escolhida pelo usuário para transmitir. Tudo liberado, sem plano pago.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQuality {
    pub resolution: Resolution,
    pub fps: u32,
}

impl StreamQuality {
    /// Os fps que a interface oferece.
    pub const FPS_OPTIONS: [u32; 3] = [15, 30, 60];

    pub const DEFAULT: Self = Self {
        resolution: Resolution::P1080,
        fps: 60,
    };

    /// fps dentro do que o pipeline suporta (15 a 60).
    pub const fn fps(self) -> u32 {
        if self.fps < 15 {
            15
        } else if self.fps > 60 {
            60
        } else {
            self.fps
        }
    }

    /// Bitrate para um vídeo de `width`×`height`: ~0,064 bit por pixel por quadro
    /// (1080p60 ≈ 8 Mbps, 1440p60 ≈ 14 Mbps), entre 1,5 e 25 Mbps.
    pub fn bitrate_kbps(self, width: u32, height: u32) -> u32 {
        let pixels_per_second = u64::from(width) * u64::from(height) * u64::from(self.fps());
        ((pixels_per_second * 64 / 1_000_000) as u32).clamp(1_500, 25_000)
    }
}

impl Default for StreamQuality {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<QualityLayer> for StreamQuality {
    fn from(layer: QualityLayer) -> Self {
        match layer {
            QualityLayer::Low => Self {
                resolution: Resolution::P720,
                fps: 30,
            },
            QualityLayer::High => Self {
                resolution: Resolution::P1080,
                fps: 60,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualidade_vem_da_interface_como_no_seletor() {
        let q: StreamQuality = serde_json::from_str(r#"{"resolution":"1440","fps":30}"#).unwrap();
        assert_eq!(
            q,
            StreamQuality {
                resolution: Resolution::P1440,
                fps: 30
            }
        );
        let s: StreamQuality = serde_json::from_str(r#"{"resolution":"source","fps":60}"#).unwrap();
        assert_eq!(s.resolution, Resolution::Source);
    }

    #[test]
    fn bitrate_cresce_com_pixels_e_fps_dentro_dos_limites() {
        let q = |fps| StreamQuality {
            resolution: Resolution::P1080,
            fps,
        };
        assert_eq!(q(60).bitrate_kbps(1920, 1080), 7_962);
        assert!(q(60).bitrate_kbps(2560, 1440) > 14_000);
        assert_eq!(q(60).bitrate_kbps(3840, 2160), 25_000, "teto de 25 Mbps");
        assert_eq!(q(15).bitrate_kbps(640, 360), 1_500, "piso de 1,5 Mbps");
    }

    #[test]
    fn fps_fora_da_faixa_e_ajustado() {
        let q = |fps| {
            StreamQuality {
                resolution: Resolution::P720,
                fps,
            }
            .fps()
        };
        assert_eq!((q(5), q(30), q(144)), (15, 30, 60));
    }

    #[test]
    fn camadas_antigas_viram_qualidades() {
        assert_eq!(StreamQuality::from(QualityLayer::High), StreamQuality::DEFAULT);
        assert_eq!(StreamQuality::from(QualityLayer::Low).fps, 30);
    }

    #[test]
    fn peer_id_exibe_em_hexadecimal() {
        assert_eq!(PeerId(255).to_string(), "peer-00000000000000ff");
    }

    #[test]
    fn camada_vem_da_interface_em_minusculas() {
        assert_eq!(
            serde_json::from_str::<QualityLayer>("\"high\"").unwrap(),
            QualityLayer::High
        );
        assert_eq!(serde_json::to_string(&QualityLayer::Low).unwrap(), "\"low\"");
    }

    #[test]
    fn caixa_de_cada_resolucao_limita_pela_altura() {
        assert_eq!(Resolution::P720.max_box(), (2880, 720));
        assert_eq!(Resolution::P1080.max_box(), (4320, 1080));
        assert_eq!(Resolution::P1440.max_box(), (5760, 1440));
        assert_eq!(
            Resolution::Source.max_box(),
            (4096, 2304),
            "limite do H.264 de hardware"
        );
    }

    #[test]
    fn resolucao_vai_e_volta_da_interface() {
        for (r, json) in [
            (Resolution::P720, "\"720\""),
            (Resolution::P1080, "\"1080\""),
            (Resolution::P1440, "\"1440\""),
            (Resolution::Source, "\"source\""),
        ] {
            assert_eq!(serde_json::to_string(&r).unwrap(), json);
            assert_eq!(serde_json::from_str::<Resolution>(json).unwrap(), r);
        }
        assert!(serde_json::from_str::<Resolution>("\"8k\"").is_err());
    }

    #[test]
    fn qualidade_padrao_e_1080p60_e_os_fps_do_seletor_sao_validos() {
        assert_eq!(StreamQuality::default(), StreamQuality::DEFAULT);
        assert_eq!(
            serde_json::to_string(&StreamQuality::DEFAULT).unwrap(),
            r#"{"resolution":"1080","fps":60}"#
        );
        for fps in StreamQuality::FPS_OPTIONS {
            let q = StreamQuality {
                resolution: Resolution::P1080,
                fps,
            };
            assert_eq!(q.fps(), fps, "a interface só oferece fps aceitos sem ajuste");
        }
    }

    #[test]
    fn peer_ids_ordenam_pelo_numero() {
        let mut ids = vec![PeerId(3), PeerId(1), PeerId(2)];
        ids.sort();
        assert_eq!(ids, vec![PeerId(1), PeerId(2), PeerId(3)]);
        assert_eq!(PeerId(u64::MAX).to_string(), "peer-ffffffffffffffff");
    }

    #[test]
    fn camada_alta_pede_mais_banda_que_a_baixa() {
        assert!(QualityLayer::High.target_kbps() > QualityLayer::Low.target_kbps());
    }
}
