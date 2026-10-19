//! Transporte P2P sobre QUIC.
//!
//! Fase 2: conexão direta com hole punching, com a sinalização feita pelo Worker em
//! `services/signaling`. Quando dois membros não conseguem se ver (CGNAT), um terceiro
//! membro da sala faz a ponte.

/// Versão do protocolo de rede. Membros com versões diferentes não entram na mesma sala.
pub const PROTOCOL_VERSION: u16 = 1;

/// ALPN usado na negociação QUIC.
pub const ALPN: &[u8] = b"squadview/1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpn_carrega_a_versao_do_protocolo() {
        assert!(ALPN.ends_with(PROTOCOL_VERSION.to_string().as_bytes()));
    }
}
