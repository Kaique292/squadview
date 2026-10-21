//! Sessões de mídia: liga captura, encoder, decoder e janela.
//!
//! - [`VideoSender`]: captura → NV12 → encoder (lado de quem transmite).
//! - [`VideoViewer`]: frames codificados → decoder → janela (lado de quem assiste).
//! - [`LocalPreview`]: os dois ligados no mesmo PC, para medir antes de usar a rede.

mod audio;
pub mod dm;
mod log;
mod preview;
mod remote;
mod room;
mod sender;
mod stats;
mod surface;
mod system;
mod viewer;
mod voice;
mod voice_mesh;

pub use sv_media::{Bounds, WindowEvent, WindowTarget};

use std::fmt;

pub use audio::{AudioPacket, AudioReceiver, AudioSender, ReceiverEvent};
pub use log::{SessionLog, recent_logs};
pub use preview::{KeyframeHandle, LocalPreview, StreamTap};
pub use remote::{RelaySource, RemoteView};
pub use room::{JoinAs, Room, RoomUpdate, WatchPath};
pub use sender::{SenderEvent, StreamInfo, VideoSender};
pub use stats::{StatsMeter, StreamStats};
pub use sv_net::{DiscordUser, JoinRequester, Member, SpeakingEffect};
pub use system::{SystemLoad, SystemMonitor};
pub use viewer::{ClockOffset, VideoViewer, ViewerEvent};
pub use voice::{OnPacket, VoiceEngine, VoiceEvent, VoiceSettings};

/// O que pode dar errado ao começar uma sessão, com mensagem para o usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    Gpu(String),
    Capture(String),
    Encoder(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gpu(e) => write!(f, "Não deu para usar a placa de vídeo: {e}"),
            Self::Capture(e) => write!(f, "Não deu para capturar essa tela ou janela: {e}"),
            Self::Encoder(e) => write!(f, "Não deu para iniciar o encoder de vídeo: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_erro_de_sessao_explica_o_que_falhou_para_o_usuario() {
        assert_eq!(
            SessionError::Gpu("sem placa".into()).to_string(),
            "Não deu para usar a placa de vídeo: sem placa"
        );
        assert_eq!(
            SessionError::Capture("janela fechou".into()).to_string(),
            "Não deu para capturar essa tela ou janela: janela fechou"
        );
        assert_eq!(
            SessionError::Encoder("ocupado".into()).to_string(),
            "Não deu para iniciar o encoder de vídeo: ocupado"
        );
        let erro: Box<dyn std::error::Error> = Box::new(SessionError::Gpu("x".into()));
        assert!(erro.source().is_none());
    }
}
