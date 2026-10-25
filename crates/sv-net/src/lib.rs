//! Transporte P2P sobre QUIC.
//!
//! - [`stun`]: descobre o endereço público do nosso socket.
//! - [`endpoint`]: o ponto QUIC de cada membro (furo de NAT, conectar, aceitar).
//! - [`cert`]: certificados por sessão, conferidos pela fingerprint combinada na sinalização.
//! - [`media`]: vídeo (um stream por frame), áudio (datagramas) e controle.
//! - [`protocol`]: os formatos que viajam; [`clock`]: estimativa do relógio do outro lado.
//! - [`signaling`]: a sala; [`friends`]: o registro de amigos e presença.
//! - [`turn`]: o repassador na nuvem, para quando dois membros não se veem (CGNAT simétrico).
//!
//! A conexão vai em camadas (ADR 0014): direto; se não der, pelo repassador TURN de quem aceita.

pub mod cert;
pub mod clock;
pub mod endpoint;
pub mod friends;
pub mod media;
pub mod protocol;
pub mod signaling;
pub mod simulcast;
pub mod stripes;
pub mod stun;
pub mod turn;

pub use endpoint::{Accepted, ConnectError, NatMapping, Peer, Purpose};
pub use media::{
    Broadcaster, KeyframeRequester, OnKeyframe, Receiver, ReceiverSink, StripeMask, StripeReceiver, StripeRelay,
    StripeSink, StripedFrame, VideoFrame, send_stripe,
};
pub use signaling::{Arcade, DiscordUser, JoinRequester, Member, Resume, RoomClient, RoomEvent, SpeakingEffect};
pub use simulcast::Layer;
pub use turn::{TurnError, TurnServer};

/// Versão do protocolo de rede. Membros com versões diferentes não entram na mesma sala.
pub const PROTOCOL_VERSION: u16 = 1;

/// ALPN usado na negociação QUIC.
pub const ALPN: &[u8] = b"squadview/1";
/// ALPN das conexões de voz (ADR 0010): quem aceita sabe pelo ALPN que a conexão é de voz.
pub const VOICE_ALPN: &[u8] = b"squadview-voice/1";
/// ALPN das conversas privadas entre dois amigos (ADR 0017), fora de qualquer sala.
pub const DM_ALPN: &[u8] = b"squadview-dm/1";
/// ALPN da conexão de repasse de voz (ADR 0020): quem ajuda copia por ela a voz de quem a gente
/// não alcança, cada pacote com a origem.
pub const VOICE_RELAY_ALPN: &[u8] = b"squadview-voice-relay/1";
/// ALPN da conexão do fliperama (ADR 0023 e 0024): o controle (e, com o controle total, o teclado e
/// o mouse) de um jogador para quem hospeda.
pub const ARCADE_ALPN: &[u8] = b"squadview-arcade/1";
/// ALPN das conexões da malha (ADR 0030): um pai manda por ela as faixas da sua casa a um filho.
/// Apps antigos não negociam este ALPN e nunca recebem faixas.
pub const MESH_ALPN: &[u8] = b"squadview-mesh/1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpn_carrega_a_versao_do_protocolo() {
        assert!(ALPN.ends_with(PROTOCOL_VERSION.to_string().as_bytes()));
    }
}
