//! Controles do modo Lan House Fliperama (ADR 0023, plano em `docs/fliperama.md`).
//!
//! - [`pad`]: o estado de um controle ([`PadState`]), no layout do XInput.
//! - [`xinput`]: lê os controles físicos ligados ao PC.
//! - [`virtual_pad`]: cria controles virtuais de Xbox 360 pelo ViGEmBus (só no host).
//! - [`driver`]: diz se o ViGEmBus está instalado e onde está o desinstalador dele.

pub mod driver;
pub mod pad;
pub mod virtual_pad;
pub mod xinput;

pub use driver::{DriverStatus, Uninstaller, driver_status, find_uninstaller};
pub use pad::PadState;
pub use virtual_pad::{VirtualPad, VirtualPads};
