//! Lado de quem assiste: decodificação por GPU, janela de vídeo e áudio Opus.

mod composition;
mod decode;
pub mod devices;
pub mod jitter;
pub mod level;
mod mic;
pub mod opus;
mod playback;
pub mod voice;
mod wasapi;
mod window;

pub use composition::Layer;
pub use decode::Decoder;
pub use devices::{AudioDevice, Flow, list_devices};
pub use level::{AutoGain, soft_clip, volume_gain};
pub use mic::{MIC_SAMPLE_RATE, MicCapture};
pub use playback::{AudioPlayer, MAX_BUFFERED_MS, MAX_VOLUME};
pub use window::{Bounds, Hole, Renderer, VideoWindow, WindowControl, WindowEvent, WindowTarget};
