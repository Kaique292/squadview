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
    /// Tamanho do estado na rede (ver [`crate::wire`]).
    pub const BYTES: usize = 12;

    pub fn pressed(&self, button: u16) -> bool {
        self.buttons & button == button
    }

    /// O estado em 12 bytes (big-endian), para o datagrama do fliperama.
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        out[0..2].copy_from_slice(&self.buttons.to_be_bytes());
        out[2] = self.left_trigger;
        out[3] = self.right_trigger;
        out[4..6].copy_from_slice(&self.thumb_lx.to_be_bytes());
        out[6..8].copy_from_slice(&self.thumb_ly.to_be_bytes());
        out[8..10].copy_from_slice(&self.thumb_rx.to_be_bytes());
        out[10..12].copy_from_slice(&self.thumb_ry.to_be_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTES]) -> Self {
        let i16_at = |i: usize| i16::from_be_bytes([bytes[i], bytes[i + 1]]);
        Self {
            buttons: u16::from_be_bytes([bytes[0], bytes[1]]),
            left_trigger: bytes[2],
            right_trigger: bytes[3],
            thumb_lx: i16_at(4),
            thumb_ly: i16_at(6),
            thumb_rx: i16_at(8),
            thumb_ry: i16_at(10),
        }
    }

    /// Junta dois controles do mesmo jogador (o físico e o teclado, por exemplo): botões somados,
    /// e em cada gatilho ou analógico vale o mais apertado.
    pub fn merge(self, other: Self) -> Self {
        let stick = |a: i16, b: i16| if a.unsigned_abs() >= b.unsigned_abs() { a } else { b };
        Self {
            buttons: self.buttons | other.buttons,
            left_trigger: self.left_trigger.max(other.left_trigger),
            right_trigger: self.right_trigger.max(other.right_trigger),
            thumb_lx: stick(self.thumb_lx, other.thumb_lx),
            thumb_ly: stick(self.thumb_ly, other.thumb_ly),
            thumb_rx: stick(self.thumb_rx, other.thumb_rx),
            thumb_ry: stick(self.thumb_ry, other.thumb_ry),
        }
    }

    /// Zera os analógicos e gatilhos dentro da zona morta (os valores recomendados pelo XInput):
    /// um controle parado não fica mandando estados "novos" pela rede.
    pub fn with_deadzone(self) -> Self {
        const LEFT: i16 = 7849;
        const RIGHT: i16 = 8689;
        const TRIGGER: u8 = 30;
        let stick = |x: i16, y: i16, dead: i16| {
            let magnitude = (f32::from(x).powi(2) + f32::from(y).powi(2)).sqrt();
            if magnitude <= f32::from(dead) { (0, 0) } else { (x, y) }
        };
        let (thumb_lx, thumb_ly) = stick(self.thumb_lx, self.thumb_ly, LEFT);
        let (thumb_rx, thumb_ry) = stick(self.thumb_rx, self.thumb_ry, RIGHT);
        let trigger = |t: u8| if t <= TRIGGER { 0 } else { t };
        Self {
            left_trigger: trigger(self.left_trigger),
            right_trigger: trigger(self.right_trigger),
            thumb_lx,
            thumb_ly,
            thumb_rx,
            thumb_ry,
            ..self
        }
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
    fn juntar_soma_botoes_e_fica_com_o_mais_apertado() {
        let pad = PadState {
            buttons: buttons::A,
            left_trigger: 10,
            thumb_lx: -100,
            thumb_ry: 5,
            ..PadState::default()
        };
        let keys = PadState {
            buttons: buttons::B,
            left_trigger: 255,
            thumb_lx: 32767,
            ..PadState::default()
        };
        let both = pad.merge(keys);
        assert_eq!(both.buttons, buttons::A | buttons::B);
        assert_eq!(both.left_trigger, 255);
        assert_eq!(both.thumb_lx, 32767);
        assert_eq!(both.thumb_ry, 5);
    }

    #[test]
    fn zona_morta_zera_o_controle_parado() {
        let still = PadState {
            thumb_lx: 3000,
            thumb_ly: -2000,
            thumb_rx: 8000,
            left_trigger: 20,
            buttons: buttons::X,
            ..PadState::default()
        };
        let cleaned = still.with_deadzone();
        assert_eq!(
            (
                cleaned.thumb_lx,
                cleaned.thumb_ly,
                cleaned.thumb_rx,
                cleaned.left_trigger
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(cleaned.buttons, buttons::X, "botão não tem zona morta");
        let pushed = PadState {
            thumb_lx: 20_000,
            ..PadState::default()
        };
        assert_eq!(pushed.with_deadzone().thumb_lx, 20_000);
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
