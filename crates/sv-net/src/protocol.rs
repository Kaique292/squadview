//! O que viaja entre os membros.
//!
//! - **Sinais** (JSON, pela sinalização): combinar quem assiste quem, trocar endereços e
//!   fingerprints. Ver `services/signaling/PROTOCOL.md` para o envelope.
//! - **Mídia** (QUIC direto, ver `docs/adr/0008-midia-sobre-quic-um-stream-por-frame.md`):
//!   um stream unidirecional por frame de vídeo, datagramas para o áudio e um stream de controle.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Payload dos `signal` trocados pela sinalização.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Signal {
    /// Quem quer assistir manda seus endereços e sua fingerprint.
    WatchRequest {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
    },
    /// Quem transmite aceita e manda os dele (e já começa a "furar" o NAT na direção do outro).
    WatchAccept {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
        stream: StreamDescription,
        /// Nosso endereço no repassador TURN (ADR 0014): quem assiste tenta por ele se o direto
        /// não fechar. Ausente em apps antigos ou sem repassador.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<SocketAddr>,
    },
    /// Quem transmite recusou (não está ao vivo, ou atingiu o limite de espectadores).
    WatchDeny { reason: String },
    /// Repasse pela sala (ADR 0014): quem não conecta em `streamer` pede a quem já assiste para
    /// receber por ele. Quem ajuda responde com um `WatchAccept` (ou `WatchDeny`). Apps antigos
    /// ignoram, e quem pediu tenta o próximo.
    RelayRequest {
        streamer: String,
        candidates: Vec<SocketAddr>,
        fingerprint: String,
    },
    /// Voz (ADR 0010): quem tem o id menor chama, com seus endereços e fingerprint.
    VoiceOffer {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
    },
    /// Voz: o outro aceita, manda os dele e "fura" o NAT; quem chamou conecta.
    VoiceAnswer {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
        /// Nosso endereço no repassador TURN, como no [`Signal::WatchAccept`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<SocketAddr>,
    },
    /// Voz repassada (ADR 0020): a voz com `to` não abriu; quem pede quer que a gente repasse.
    VoiceRelayRequest { to: String },
    /// Quem ajuda aceitou repassar a voz entre quem pediu e `to` (e vai abrir a conexão de repasse).
    VoiceRelayAccept { to: String },
    /// Quem ajuda não pode repassar (não fala com `to`, ou já repassa o máximo de pares).
    VoiceRelayDeny { to: String, reason: String },
    /// Quem pediu manda à outra ponta a chave do par (`key`, hex) e quem vai repassar (`via`). Vai
    /// direto para ela pela sinalização: quem repassa não vê a chave.
    VoiceRelayKey { via: String, key: String },
}

/// O que quem assiste precisa saber para preparar o decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDescription {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub audio: bool,
}

/// Cabeçalho de cada stream de vídeo (seguido pelos bytes do frame Annex B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHeader {
    pub seq: u32,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub encode_us: u32,
}

/// Mensagens do stream de controle (bidirecional, um por conexão).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    /// Quem assiste perdeu a cadeia de referências e precisa de um keyframe.
    KeyframeRequest,
    /// Medição do relógio: quem assiste manda a hora local dele.
    Ping { sent_us: u64 },
    /// Resposta de quem transmite, com a hora dele no momento da resposta.
    Pong { sent_us: u64, remote_us: u64 },
}

/// Maior frame aceito (um keyframe 4K em bitrate alto cabe com folga).
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Maior mensagem de controle ou cabeçalho.
pub const MAX_HEADER_BYTES: usize = 64;

/// Datagrama de áudio: `seq: u32 | timestamp_us: u64 | peak: u8 | opus...` (big-endian).
pub fn encode_audio(seq: u32, timestamp_us: u64, peak: f32, opus: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&timestamp_us.to_be_bytes());
    out.push((peak.clamp(0.0, 1.0) * 255.0).round() as u8);
    out.extend_from_slice(opus);
}

/// Lê um datagrama de áudio; `None` se ele estiver truncado.
pub fn decode_audio(datagram: &[u8]) -> Option<(u32, u64, f32, &[u8])> {
    if datagram.len() < 13 {
        return None;
    }
    let seq = u32::from_be_bytes(datagram[0..4].try_into().ok()?);
    let ts = u64::from_be_bytes(datagram[4..12].try_into().ok()?);
    let peak = f32::from(datagram[12]) / 255.0;
    Some((seq, ts, peak, &datagram[13..]))
}

/// Bytes do id de um membro (16 hex) no começo de cada pacote repassado (ADR 0020).
pub const RELAY_PEER_BYTES: usize = 8;
/// Bytes da assinatura de cada pacote repassado (HMAC-SHA256 truncado).
pub const RELAY_TAG_BYTES: usize = 8;
/// Bytes da chave de um par que conversa por repasse.
pub const RELAY_KEY_BYTES: usize = 32;

/// O id de um membro (16 caracteres hex) em bytes, como vai no pacote repassado.
pub fn relay_peer(member: &str) -> Option<[u8; RELAY_PEER_BYTES]> {
    if member.len() != RELAY_PEER_BYTES * 2 {
        return None;
    }
    let mut out = [0u8; RELAY_PEER_BYTES];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(member.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Os bytes de volta para o id do membro (hex minúsculo, como o Worker sorteia).
pub fn relay_member(peer: &[u8; RELAY_PEER_BYTES]) -> String {
    peer.iter().map(|b| format!("{b:02x}")).collect()
}

/// A chave que só as duas pontas de um repasse de voz conhecem (quem repassa não): cada pacote
/// leva uma assinatura com ela, e quem repassa não consegue inventar nem desviar a voz de ninguém.
pub struct RelayKey(ring::hmac::Key);

impl std::fmt::Debug for RelayKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nunca imprime a chave (os registros vão para arquivo).
        f.write_str("RelayKey(..)")
    }
}

impl RelayKey {
    /// Uma chave nova, sorteada; `None` se o sorteio do sistema falhar.
    pub fn random() -> Option<[u8; RELAY_KEY_BYTES]> {
        let mut key = [0u8; RELAY_KEY_BYTES];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut key).ok()?;
        Some(key)
    }

    pub fn new(key: &[u8; RELAY_KEY_BYTES]) -> Self {
        Self(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key))
    }

    /// A chave em hex, como viaja no `signal`.
    pub fn to_hex(key: &[u8; RELAY_KEY_BYTES]) -> String {
        key.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A chave a partir do hex do `signal`; `None` se não tiver o tamanho certo.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != RELAY_KEY_BYTES * 2 {
            return None;
        }
        let mut key = [0u8; RELAY_KEY_BYTES];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(Self::new(&key))
    }

    /// Assinatura do pacote de voz `audio` que `from` manda para `to` (a direção entra na conta:
    /// o pacote não vale para outra pessoa nem no sentido contrário).
    pub fn tag(
        &self,
        from: &[u8; RELAY_PEER_BYTES],
        to: &[u8; RELAY_PEER_BYTES],
        audio: &[u8],
    ) -> [u8; RELAY_TAG_BYTES] {
        let mut ctx = ring::hmac::Context::with_key(&self.0);
        ctx.update(from);
        ctx.update(to);
        ctx.update(audio);
        let full = ctx.sign();
        let mut tag = [0u8; RELAY_TAG_BYTES];
        tag.copy_from_slice(&full.as_ref()[..RELAY_TAG_BYTES]);
        tag
    }

    /// Confere a assinatura (em tempo constante).
    pub fn verify(
        &self,
        from: &[u8; RELAY_PEER_BYTES],
        to: &[u8; RELAY_PEER_BYTES],
        audio: &[u8],
        tag: &[u8; RELAY_TAG_BYTES],
    ) -> bool {
        let expected = self.tag(from, to, audio);
        expected.iter().zip(tag).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// Pacote da conexão de repasse de voz: `membro: 8 | assinatura: 8 | datagrama de áudio`. Quem
/// manda para quem repassa põe o destino; quem repassa troca pela origem. Só o `membro`, sem o
/// resto, avisa que a voz dele parou de ser repassada.
pub fn encode_relayed(peer: &[u8; RELAY_PEER_BYTES], tag: &[u8; RELAY_TAG_BYTES], audio: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.extend_from_slice(peer);
    out.extend_from_slice(tag);
    out.extend_from_slice(audio);
}

/// Um pacote da conexão de repasse.
#[derive(Debug, PartialEq, Eq)]
pub enum Relayed<'a> {
    /// A voz de (ou para) `peer`, com a assinatura.
    Audio {
        peer: [u8; RELAY_PEER_BYTES],
        tag: [u8; RELAY_TAG_BYTES],
        audio: &'a [u8],
    },
    /// A voz de `peer` parou de ser repassada.
    Ended { peer: [u8; RELAY_PEER_BYTES] },
}

/// Lê um pacote da conexão de repasse; `None` se ele estiver truncado.
pub fn decode_relayed(datagram: &[u8]) -> Option<Relayed<'_>> {
    let peer: [u8; RELAY_PEER_BYTES] = datagram.get(..RELAY_PEER_BYTES)?.try_into().ok()?;
    if datagram.len() == RELAY_PEER_BYTES {
        return Some(Relayed::Ended { peer });
    }
    let tag = datagram
        .get(RELAY_PEER_BYTES..RELAY_PEER_BYTES + RELAY_TAG_BYTES)?
        .try_into()
        .ok()?;
    Some(Relayed::Audio {
        peer,
        tag,
        audio: &datagram[RELAY_PEER_BYTES + RELAY_TAG_BYTES..],
    })
}

/// Codifica uma mensagem curta com prefixo de tamanho (u16), para streams.
pub fn frame_message<T: Serialize>(msg: &T) -> Vec<u8> {
    let body = postcard::to_stdvec(msg).expect("serializar mensagem do protocolo");
    let mut out = Vec::with_capacity(body.len() + 2);
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinal_vira_json_com_kind() {
        let s = Signal::WatchRequest {
            candidates: vec!["1.2.3.4:5000".parse().unwrap()],
            fingerprint: "ab".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"watch-request","candidates":["1.2.3.4:5000"],"fingerprint":"ab"}"#
        );
        assert_eq!(serde_json::from_str::<Signal>(&json).unwrap(), s);
    }

    #[test]
    fn sinal_desconhecido_e_recusado() {
        assert!(serde_json::from_str::<Signal>(r#"{"kind":"hack"}"#).is_err());
    }

    #[test]
    fn datagrama_de_audio_ida_e_volta() {
        let mut buf = Vec::new();
        encode_audio(7, 123_456_789, 0.5, &[9, 8, 7], &mut buf);
        let (seq, ts, peak, opus) = decode_audio(&buf).unwrap();
        assert_eq!((seq, ts, opus), (7, 123_456_789, &[9u8, 8, 7][..]));
        assert!((peak - 0.5).abs() < 0.01);
        assert_eq!(decode_audio(&buf[..5]), None);
    }

    #[test]
    fn sinais_do_repasse_de_voz_viram_json() {
        let s = Signal::VoiceRelayKey {
            via: "0123456789abcdef".into(),
            key: "ff".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"voice-relay-key","via":"0123456789abcdef","key":"ff"}"#
        );
        assert_eq!(serde_json::from_str::<Signal>(&json).unwrap(), s);
        let r = serde_json::from_str::<Signal>(r#"{"kind":"voice-relay-request","to":"ab"}"#).unwrap();
        assert_eq!(r, Signal::VoiceRelayRequest { to: "ab".into() });
    }

    #[test]
    fn id_do_membro_vira_bytes_e_volta() {
        let peer = relay_peer("0123456789abcdef").unwrap();
        assert_eq!(peer, [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
        assert_eq!(relay_member(&peer), "0123456789abcdef");
        assert_eq!(relay_peer("curto"), None);
        assert_eq!(relay_peer("0123456789abcdeg"), None);
        assert_eq!(relay_peer("ção3456789abcd"), None);
    }

    #[test]
    fn so_quem_tem_a_chave_assina_e_a_direcao_conta() {
        let bytes = RelayKey::random().unwrap();
        let key = RelayKey::from_hex(&RelayKey::to_hex(&bytes)).unwrap();
        let (ana, beto, caio) = ([1u8; 8], [2u8; 8], [3u8; 8]);
        let tag = key.tag(&ana, &beto, b"oi");
        assert!(key.verify(&ana, &beto, b"oi", &tag));
        // Outro áudio, outra direção, outro destino ou outra chave: não confere.
        assert!(!key.verify(&ana, &beto, b"ol", &tag));
        assert!(!key.verify(&beto, &ana, b"oi", &tag));
        assert!(!key.verify(&ana, &caio, b"oi", &tag));
        let other = RelayKey::new(&RelayKey::random().unwrap());
        assert!(!other.verify(&ana, &beto, b"oi", &tag));
        assert!(RelayKey::from_hex("abc").is_none());
        assert!(RelayKey::from_hex(&"zz".repeat(32)).is_none());
    }

    #[test]
    fn pacote_repassado_ida_e_volta() {
        let mut buf = Vec::new();
        encode_relayed(&[7; 8], &[9; 8], &[1, 2, 3], &mut buf);
        assert_eq!(
            decode_relayed(&buf),
            Some(Relayed::Audio {
                peer: [7; 8],
                tag: [9; 8],
                audio: &[1, 2, 3]
            })
        );
        assert_eq!(decode_relayed(&buf[..8]), Some(Relayed::Ended { peer: [7; 8] }));
        assert_eq!(decode_relayed(&buf[..5]), None);
        assert_eq!(decode_relayed(&buf[..12]), None);
    }

    #[test]
    fn mensagem_curta_cabe_no_limite() {
        let m = frame_message(&FrameHeader {
            seq: u32::MAX,
            timestamp_us: u64::MAX,
            keyframe: true,
            encode_us: u32::MAX,
        });
        assert!(m.len() - 2 <= MAX_HEADER_BYTES);
        let back: FrameHeader = postcard::from_bytes(&m[2..]).unwrap();
        assert_eq!(back.seq, u32::MAX);
    }
}
