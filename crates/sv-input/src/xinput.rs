//! Leitura dos controles físicos pelo XInput (controles de Xbox e compatíveis).
//!
//! O XInput tem 4 vagas (`0..4`) para o PC inteiro. Os controles virtuais do ViGEmBus também
//! ocupam vagas: quem lê precisa saber quais são as suas (ver [`crate::VirtualPad::user_index`]).

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

use crate::PadState;

/// Quantas vagas o XInput tem.
pub const SLOTS: u32 = 4;

/// O estado do controle na vaga `slot`, ou `None` se não há controle nela (ou se `slot` não existe).
/// Não aloca: pode ser chamado a cada leitura.
pub fn read(slot: u32) -> Option<PadState> {
    if slot >= SLOTS {
        return None;
    }
    let mut state = XINPUT_STATE::default();
    // SAFETY: `state` é um XINPUT_STATE válido e vive durante a chamada.
    let result = unsafe { XInputGetState(slot, &mut state) };
    if result != ERROR_SUCCESS.0 {
        return None;
    }
    let g = state.Gamepad;
    Some(PadState {
        buttons: g.wButtons.0,
        left_trigger: g.bLeftTrigger,
        right_trigger: g.bRightTrigger,
        thumb_lx: g.sThumbLX,
        thumb_ly: g.sThumbLY,
        thumb_rx: g.sThumbRX,
        thumb_ry: g.sThumbRY,
    })
}

/// Quais vagas têm controle agora.
pub fn connected() -> [bool; SLOTS as usize] {
    std::array::from_fn(|slot| read(slot as u32).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaga_que_nao_existe_nao_tem_controle() {
        assert_eq!(read(SLOTS), None);
        assert_eq!(read(u32::MAX), None);
    }

    #[test]
    fn a_lista_bate_com_a_leitura_de_cada_vaga() {
        // Sem controle (como na CI) as quatro vêm vazias; com controle, a lista e a leitura batem.
        let slots = connected();
        for (slot, present) in slots.iter().enumerate() {
            assert_eq!(*present, read(slot as u32).is_some(), "vaga {slot}");
        }
    }
}
