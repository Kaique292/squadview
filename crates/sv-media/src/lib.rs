//! Lado de quem assiste: decodificação por GPU, janela de vídeo e áudio Opus.

mod decode;
pub mod jitter;
pub mod opus;
mod playback;
mod window;

pub use decode::Decoder;
pub use playback::{AudioPlayer, MAX_BUFFERED_MS, MAX_VOLUME};
pub use window::{Bounds, Renderer, VideoWindow, WindowControl, WindowEvent, WindowTarget};
