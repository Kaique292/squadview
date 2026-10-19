//! Captura de tela e de áudio no Windows.
//!
//! - [`sources`]: lista monitores e janelas que podem ser transmitidos.
//! - [`video`]: Windows.Graphics.Capture com a textura mantida na GPU (zero cópia para a CPU).
//! - [`throttle`]: limita os frames ao fps escolhido.

pub mod audio;
pub mod sources;
pub mod throttle;
pub mod thumbnail;
pub mod video;

pub use audio::AudioCapture;
pub use sources::{CaptureSource, Source, find_replacement, is_alive, list_sources};
pub use throttle::FrameThrottle;
pub use thumbnail::{Thumbnail, read_frame, thumbnail};
pub use video::VideoCapture;
