//! Estado de um controle, no mesmo layout do `XINPUT_GAMEPAD`: é o que o jogador lê, o que viaja
//! pela rede e o que o controle virtual do host recebe.

/// Botões, no layout do XInput (`XINPUT_GAMEPAD_*`).
pub mod buttons {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// O estado completo de um controle num instante. `Default` é o controle solto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PadState {
    /// Botões apertados (ver [`buttons`]).
    pub buttons: u16,
    /// Gatilhos: `0` solto, `255` apertado até o fim.
    pub left_trigger: u8,
    pub right_trigger: u8,
    /// Analógicos: `-32768..=32767`, `0` no centro; para cima é positivo.
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

impl PadState {
    pub fn pressed(&self, button: u16) -> bool {
        self.buttons & button == button
    }

    /// O mesmo estado no relatório do controle virtual de Xbox 360.
    pub fn to_x360(self) -> vigem_rust::X360Report {
        vigem_rust::X360Report {
            buttons: vigem_rust::X360Button::from_bits_retain(self.buttons),
            left_trigger: self.left_trigger,
            right_trigger: self.right_trigger,
            thumb_lx: self.thumb_lx,
            thumb_ly: self.thumb_ly,
            thumb_rx: self.thumb_rx,
            thumb_ry: self.thumb_ry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controle_solto_nao_aperta_nada() {
        let pad = PadState::default();
        assert!(!pad.pressed(buttons::A));
        assert_eq!(pad.to_x360(), vigem_rust::X360Report::default());
    }

    #[test]
    fn passa_cada_campo_para_o_controle_virtual_sem_mudar() {
        let pad = PadState {
            buttons: buttons::A | buttons::DPAD_LEFT | buttons::START,
            left_trigger: 10,
            right_trigger: 255,
            thumb_lx: -32768,
            thumb_ly: 32767,
            thumb_rx: -1,
            thumb_ry: 1234,
        };
        let report = pad.to_x360();
        assert_eq!(report.buttons.bits(), pad.buttons);
        assert!(report.buttons.contains(vigem_rust::X360Button::A));
        assert!(report.buttons.contains(vigem_rust::X360Button::DPAD_LEFT));
        assert_eq!((report.left_trigger, report.right_trigger), (10, 255));
        assert_eq!(
            (report.thumb_lx, report.thumb_ly, report.thumb_rx, report.thumb_ry),
            (-32768, 32767, -1, 1234)
        );
    }

    #[test]
    fn botoes_seguem_o_layout_do_xinput() {
        // O mesmo valor do XINPUT_GAMEPAD_* e do controle virtual: um botão nunca troca de lugar.
        use vigem_rust::X360Button as V;
        for (ours, theirs) in [
            (buttons::DPAD_UP, V::DPAD_UP),
            (buttons::START, V::START),
            (buttons::BACK, V::BACK),
            (buttons::LEFT_SHOULDER, V::LEFT_SHOULDER),
            (buttons::RIGHT_SHOULDER, V::RIGHT_SHOULDER),
            (buttons::GUIDE, V::GUIDE),
            (buttons::A, V::A),
            (buttons::B, V::B),
            (buttons::X, V::X),
            (buttons::Y, V::Y),
        ] {
            assert_eq!(ours, theirs.bits());
        }
    }
}
