//! Transporte P2P sobre QUIC.
//!
//! - [`stun`]: descobre o endereço público do nosso socket.
//! - [`endpoint`]: o ponto QUIC de cada membro (furo de NAT, conectar, aceitar).
//! - [`cert`]: certificados por sessão, conferidos pela fingerprint combinada na sinalização.
//! - [`media`]: vídeo (um stream por frame), áudio (datagramas) e controle.
//! - [`protocol`]: os formatos que viajam; [`clock`]: estimativa do relógio do outro lado.
//! - [`signaling`]: a sala; [`friends`]: o registro de amigos e presença.
//!
//! Quando dois membros não conseguem se ver (CGNAT simétrico), a conexão falha com uma mensagem
//! clara; a ponte por um terceiro membro da sala vem na Fase 3.

pub mod cert;
pub mod clock;
pub mod endpoint;
pub mod friends;
pub mod media;
pub mod protocol;
pub mod signaling;
pub mod stun;

pub use endpoint::{Accepted, ConnectError, NatMapping, Peer, Purpose};
pub use media::{Broadcaster, KeyframeRequester, Receiver, ReceiverSink, VideoFrame};
pub use signaling::{DiscordUser, JoinRequester, Member, RoomClient, RoomEvent};

/// Versão do protocolo de rede. Membros com versões diferentes não entram na mesma sala.
pub const PROTOCOL_VERSION: u16 = 1;

/// ALPN usado na negociação QUIC.
pub const ALPN: &[u8] = b"squadview/1";
/// ALPN das conexões de voz (ADR 0010): quem aceita sabe pelo ALPN que a conexão é de voz.
pub const VOICE_ALPN: &[u8] = b"squadview-voice/1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpn_carrega_a_versao_do_protocolo() {
        assert!(ALPN.ends_with(PROTOCOL_VERSION.to_string().as_bytes()));
    }
}
