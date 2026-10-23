//! Controles do modo Lan House Fliperama (ADR 0023, plano em `docs/fliperama.md`).
//!
//! - [`pad`]: o estado de um controle ([`PadState`]), no layout do XInput.
//! - [`xinput`]: lê os controles físicos ligados ao PC.
//! - [`virtual_pad`]: cria controles virtuais de Xbox 360 pelo ViGEmBus (só no host).
//! - [`driver`]: diz se o ViGEmBus está instalado e onde está o desinstalador dele.
//! - [`wire`]: o que viaja na conexão do fliperama (controle e teclado/mouse).
//! - [`reader`]: lê o controle físico do jogador numa thread própria.
//! - [`keymap`]: teclado e mouse do jogador viram controle (ADR 0024).
//! - [`kbm`]: o "controle total" de teclado e mouse no host, com as regras de segurança (ADR 0024).

pub mod driver;
pub mod kbm;
pub mod keymap;
pub mod pad;
pub mod reader;
pub mod virtual_pad;
pub mod wire;
pub mod xinput;

pub use driver::{DriverStatus, Uninstaller, driver_status, find_uninstaller};
pub use keymap::{KeyMap, KeyboardPad, PadInput};
pub use pad::PadState;
pub use reader::PadReader;
pub use virtual_pad::{VirtualPad, VirtualPads};
pub use wire::{KbmEvent, MouseButton};
