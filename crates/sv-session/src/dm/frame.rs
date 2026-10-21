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
    /// Mensagem de texto. `id` é de quem manda; repetir o mesmo `id` não duplica.
    Text { id: String, at: u64, body: String },
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
    fn formato_do_quadro_e_o_combinado() {
        let json = serde_json::to_string(&Frame::FileRequest {
            id: "a".into(),
            offset: 7,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"file-request","id":"a","offset":7}"#);
    }
}
