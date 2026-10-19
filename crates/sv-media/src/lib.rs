//! Lado de quem assiste: decodificação por GPU, janela de vídeo e áudio Opus.

mod decode;
pub mod devices;
pub mod jitter;
mod mic;
pub mod opus;
mod playback;
mod window;

pub use decode::Decoder;
pub use devices::{AudioDevice, Flow, list_devices};
pub use mic::{MIC_SAMPLE_RATE, MicCapture};
pub use playback::{AudioPlayer, MAX_BUFFERED_MS, MAX_VOLUME};
pub use window::{Bounds, Renderer, VideoWindow, WindowControl, WindowEvent, WindowTarget};
