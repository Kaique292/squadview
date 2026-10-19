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
    },
    /// Quem transmite recusou (não está ao vivo, ou atingiu o limite de espectadores).
    WatchDeny { reason: String },
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
