//! Malha da sala: decide quem repassa o stream para quem.
//!
//! É lógica pura, sem rede de verdade, para ser testada com cenários simulados.
//! O protocolo está descrito em `docs/protocolo-malha.md`.
//!
//! - [`plan`]: o planejador em enxame (várias árvores, uma por faixa do vídeo; ADR 0012).
//! - [`sim`]: o simulador de sala, com a população e a internet do Brasil.

pub mod plan;
pub mod sim;

use sv_core::{PeerId, QualityLayer};

/// O que sabemos da conexão de um membro da sala.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeerStats {
    pub id: PeerId,
    /// Upload livre medido, em kbit/s.
    pub upload_kbps: u32,
    /// Latência até o streamer, em milissegundos.
    pub rtt_ms: u32,
    /// Perda de pacotes recente, de 0.0 a 1.0.
    pub loss: f32,
    /// Está jogando agora? Quem joga repassa o mínimo, para não ter lag na partida.
    pub in_game: bool,
}

impl PeerStats {
    /// Quantos membros este peer consegue alimentar numa camada, guardando 25% do upload de folga.
    pub fn relay_slots(&self, layer: QualityLayer) -> u32 {
        let usable = self.upload_kbps.saturating_mul(3) / 4;
        let slots = usable / layer.target_kbps();
        if self.in_game { slots.min(1) } else { slots }
    }

    /// Nota para escolher repassadores: quanto maior, melhor candidato a "pai".
    pub fn relay_score(&self, layer: QualityLayer) -> f32 {
        let slots = self.relay_slots(layer) as f32;
        let latency_penalty = 1.0 + self.rtt_ms as f32 / 50.0;
        let loss_penalty = 1.0 + self.loss * 20.0;
        slots / (latency_penalty * loss_penalty)
    }
}

/// Camada que um membro consegue receber com o download livre que tem.
pub fn layer_for_download(download_kbps: u32) -> QualityLayer {
    if download_kbps >= QualityLayer::High.target_kbps() * 5 / 4 {
        QualityLayer::High
    } else {
        QualityLayer::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(upload_kbps: u32, rtt_ms: u32, loss: f32, in_game: bool) -> PeerStats {
        PeerStats {
            id: PeerId(1),
            upload_kbps,
            rtt_ms,
            loss,
            in_game,
        }
    }

    #[test]
    fn fibra_alimenta_varios_membros() {
        // 50 Mbps de upload, 75% utilizável, 8 Mbps por membro em 1080p60.
        assert_eq!(peer(50_000, 10, 0.0, false).relay_slots(QualityLayer::High), 4);
    }

    #[test]
    fn quem_esta_jogando_repassa_no_maximo_um() {
        assert_eq!(peer(50_000, 10, 0.0, true).relay_slots(QualityLayer::High), 1);
    }

    #[test]
    fn perda_de_pacotes_derruba_a_nota() {
        let limpo = peer(50_000, 20, 0.0, false).relay_score(QualityLayer::High);
        let instavel = peer(50_000, 20, 0.05, false).relay_score(QualityLayer::High);
        assert!(limpo > instavel);
    }

    #[test]
    fn download_fraco_recebe_camada_baixa() {
        assert_eq!(layer_for_download(4_000), QualityLayer::Low);
        assert_eq!(layer_for_download(50_000), QualityLayer::High);
    }
}
