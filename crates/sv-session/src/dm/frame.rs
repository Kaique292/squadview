//! O que vai pela conexão direta de uma conversa (ADR 0017).
//!
//! Um stream bidirecional de controle por conexão, com quadros `u32` (tamanho, big-endian) +
//! JSON. Os bytes de um arquivo vão num stream unidirecional próprio, que começa com um quadro
//! [`FileHeader`] e segue com o conteúdo cru até o fim.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maior quadro de controle (uma mensagem de texto cabe com folga; arquivo nunca vai aqui).
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
/// Maior texto de uma mensagem, em caracteres (como o limite do Discord, com folga).
pub const MAX_TEXT_CHARS: usize = 4000;

/// Quadro de controle da conversa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Frame {
    /// Primeiro quadro de quem conectou: abre o stream de controle para os dois lados poderem falar.
    Hello,
    /// Mensagem de texto. `id` é de quem manda; repetir o mesmo `id` não duplica. `reply_to`: a
    /// mensagem que ela responde (apps antigos ignoram o campo e mostram só o texto).
    Text {
        id: String,
        at: u64,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
    /// Chegou (texto ou o aviso de um arquivo).
    Ack { id: String },
    /// Há um arquivo para baixar; quem recebe decide se e quando pede.
    FileOffer {
        id: String,
        at: u64,
        name: String,
        size: u64,
        mime: String,
    },
    /// Manda o arquivo a partir de `offset` (o que já chegou antes fica: é a retomada).
    FileRequest { id: String, offset: u64 },
    /// As nossas reações agora na mensagem `id` (o conjunto inteiro: tirar uma também chega).
    /// Só vai para quem avisou que entende (ver `FEATURE_REACTIONS`): app antigo derrubava a conexão
    /// ao receber um quadro que não conhece.
    Reactions { id: String, emojis: Vec<String> },
}

/// Recurso que cada lado anuncia no `dm-offer` e no `dm-answer`: entende o quadro `reactions`.
pub const FEATURE_REACTIONS: &str = "reactions";
/// Reações diferentes numa mensagem, no máximo.
pub const MAX_REACTIONS: usize = 20;
/// Tamanho máximo de um emoji (bytes): cabe qualquer emoji composto, não cabe texto.
pub const MAX_EMOJI_BYTES: usize = 32;

/// Um emoji que dá para guardar e mandar (não vazio, curto e sem espaço).
pub fn valid_emoji(emoji: &str) -> bool {
    !emoji.is_empty() && emoji.len() <= MAX_EMOJI_BYTES && !emoji.chars().any(char::is_whitespace)
}

/// Um quadro de controle: o que este app entende, ou `None` para um tipo que ele não conhece
/// (de um app mais novo), que é ignorado em vez de derrubar a conexão.
pub fn parse_frame(value: serde_json::Value) -> Option<Frame> {
    serde_json::from_value(value).ok()
}

/// Começo do stream de um arquivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHeader {
    pub id: String,
    pub offset: u64,
}

/// Escreve um quadro: tamanho em `u32` big-endian + JSON.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, value: &T) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::other("quadro grande demais"));
    }
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await
}

/// Lê um quadro. `Ok(None)`: o outro lado fechou o stream entre quadros.
pub async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R) -> std::io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::other("quadro grande demais"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn quadros_vao_e_voltam_iguais() {
        let frames = vec![
            Frame::Text {
                id: "a-1".into(),
                at: 1,
                body: "oi, bora jogar?".into(),
                reply_to: None,
            },
            Frame::Text {
                id: "a-3".into(),
                at: 3,
                body: "bora".into(),
                reply_to: Some("b-1".into()),
            },
            Frame::Reactions {
                id: "b-1".into(),
                emojis: vec!["🔥".into(), "❤️".into()],
            },
            Frame::Ack { id: "a-1".into() },
            Frame::FileOffer {
                id: "a-2".into(),
                at: 2,
                name: "clipe.mp4".into(),
                size: 10 << 20,
                mime: "video/mp4".into(),
            },
            Frame::FileRequest {
                id: "a-2".into(),
                offset: 4096,
            },
        ];
        let mut wire = Vec::new();
        for f in &frames {
            write_frame(&mut wire, f).await.unwrap();
        }
        let mut r = wire.as_slice();
        for f in &frames {
            assert_eq!(read_frame::<_, Frame>(&mut r).await.unwrap().as_ref(), Some(f));
        }
        // Acabou entre quadros: fim normal.
        assert_eq!(read_frame::<_, Frame>(&mut r).await.unwrap(), None);
    }

    #[tokio::test]
    async fn recusa_quadro_grande_ou_quebrado() {
        let grande = Frame::Text {
            id: "x".into(),
            at: 0,
            body: "a".repeat(MAX_FRAME_BYTES),
            reply_to: None,
        };
        assert!(write_frame(&mut Vec::new(), &grande).await.is_err());
        let mut falso = (MAX_FRAME_BYTES as u32 + 1).to_be_bytes().to_vec();
        falso.extend_from_slice(b"{}");
        assert!(read_frame::<_, Frame>(&mut falso.as_slice()).await.is_err());
        let mut lixo = 3u32.to_be_bytes().to_vec();
        lixo.extend_from_slice(b"oi!");
        assert!(read_frame::<_, Frame>(&mut lixo.as_slice()).await.is_err());
        // Cortado no meio do tamanho conta como fim; no meio do corpo, como erro.
        assert_eq!(read_frame::<_, Frame>(&mut [0u8, 0].as_slice()).await.unwrap(), None);
        let mut cortado = 10u32.to_be_bytes().to_vec();
        cortado.extend_from_slice(b"{\"ki");
        assert!(read_frame::<_, Frame>(&mut cortado.as_slice()).await.is_err());
    }

    #[test]
    fn tipo_desconhecido_e_ignorado_e_texto_antigo_sem_resposta_ainda_vale() {
        // Um quadro de um app mais novo: este app ignora em vez de derrubar a conexão.
        assert_eq!(parse_frame(serde_json::json!({"kind": "do-futuro", "x": 1})), None);
        // Texto de um app antigo (sem `reply_to`).
        assert_eq!(
            parse_frame(serde_json::json!({"kind": "text", "id": "a", "at": 1, "body": "oi"})),
            Some(Frame::Text {
                id: "a".into(),
                at: 1,
                body: "oi".into(),
                reply_to: None
            })
        );
        // Sem resposta, o campo nem vai (app antigo lê igual).
        let json = serde_json::to_value(Frame::Text {
            id: "a".into(),
            at: 1,
            body: "oi".into(),
            reply_to: None,
        })
        .unwrap();
        assert!(json.get("replyTo").is_none() && json.get("reply_to").is_none());
    }

    #[test]
    fn emoji_valido_e_curto_e_sem_espaco() {
        assert!(valid_emoji("🔥"));
        assert!(valid_emoji("👨‍👩‍👧‍👦"));
        assert!(!valid_emoji(""));
        assert!(!valid_emoji("oi tudo"));
        assert!(!valid_emoji(&"a".repeat(MAX_EMOJI_BYTES + 1)));
    }

    #[test]
    fn formato_do_quadro_e_o_combinado() {
        let json = serde_json::to_string(&Frame::FileRequest {
            id: "a".into(),
            offset: 7,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"file-request","id":"a","offset":7}"#);
    }
}
