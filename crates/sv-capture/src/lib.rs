//! Captura de tela e de áudio no Windows.
//!
//! - [`sources`]: lista monitores e janelas que podem ser transmitidos.
//! - [`video`]: Windows.Graphics.Capture com a textura mantida na GPU (zero cópia para a CPU).
//! - [`throttle`]: limita os frames ao fps escolhido.
//! - [`follow`] e [`observe`]: seguem o jogo de janela em janela e para o monitor em tela cheia
//!   (ver `docs/captura.md`).

pub mod audio;
pub mod family;
pub mod follow;
pub mod observe;
pub mod sources;
#[cfg(test)]
mod test_fullscreen;
#[cfg(test)]
mod test_window;
pub mod throttle;
pub mod thumbnail;
pub mod video;

pub use audio::AudioCapture;
pub use family::{install_root, same_family};
pub use follow::{GameFollower, Observation, Status as FollowStatus, Step as FollowStep, SwitchReason, WindowFacts};
pub use observe::Observer;
pub use sources::{CaptureSource, Source, is_alive, list_sources};
pub use throttle::FrameThrottle;
pub use thumbnail::{Thumbnail, read_frame, thumbnail};
pub use video::VideoCapture;
