//! Tipos comuns do SquadView, compartilhados por todos os crates.

use std::fmt;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_exibe_em_hexadecimal() {
        assert_eq!(PeerId(255).to_string(), "peer-00000000000000ff");
    }

    #[test]
    fn camada_alta_pede_mais_banda_que_a_baixa() {
        assert!(QualityLayer::High.target_kbps() > QualityLayer::Low.target_kbps());
    }
}
