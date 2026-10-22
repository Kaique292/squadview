//! O que viaja na conexão do fliperama (`squadview-arcade/1`, ADR 0023 e 0024).
//!
//! - **Controle** (datagrama, sem retransmissão): `[1][seq u32][PadState 12 bytes]`, sempre o
//!   estado completo. Perder um não importa: o próximo corrige.
//! - **Teclado e mouse do controle total** (stream confiável, em ordem): eventos de 8 bytes. Uma
//!   tecla solta que se perdesse ficaria "presa" no jogo, por isso não vão por datagrama.

use crate::PadState;

/// Tipo do datagrama de controle (o primeiro byte; outros tipos ficam para versões futuras).
pub const PAD_DATAGRAM: u8 = 1;
/// Tamanho do datagrama de controle.
pub const PAD_DATAGRAM_LEN: usize = 1 + 4 + PadState::BYTES;
/// Tamanho de um evento de teclado ou mouse.
pub const KBM_EVENT_LEN: usize = 8;

/// Monta o datagrama de controle. Não aloca.
pub fn encode_pad(seq: u32, pad: &PadState) -> [u8; PAD_DATAGRAM_LEN] {
    let mut out = [0u8; PAD_DATAGRAM_LEN];
    out[0] = PAD_DATAGRAM;
    out[1..5].copy_from_slice(&seq.to_be_bytes());
    out[5..].copy_from_slice(&pad.to_bytes());
    out
}

/// Lê o datagrama de controle; `None` se for de outro tipo ou estiver truncado.
pub fn decode_pad(datagram: &[u8]) -> Option<(u32, PadState)> {
    if datagram.len() != PAD_DATAGRAM_LEN || datagram[0] != PAD_DATAGRAM {
        return None;
    }
    let seq = u32::from_be_bytes(datagram[1..5].try_into().ok()?);
    Some((seq, PadState::from_bytes(datagram[5..].try_into().ok()?)))
}

/// `seq` mais novo que `last`, com a volta do contador (`u32` dá a volta depois de ~200 dias a
/// 250 por segundo, mas a conta certa não custa nada).
pub fn is_newer(seq: u32, last: Option<u32>) -> bool {
    last.is_none_or(|last| seq.wrapping_sub(last) as i32 > 0)
}

/// Botão do mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Um evento do teclado ou do mouse do jogador com o controle total (ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KbmEvent {
    /// Tecla (código virtual do Windows); `extended` para setas, Insert, Ctrl e Alt da direita etc.
    Key {
        vk: u16,
        extended: bool,
        down: bool,
    },
    /// Movimento relativo do mouse.
    Move {
        dx: i16,
        dy: i16,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    /// Rolagem (múltiplos de 120 por "dente", como no Windows).
    Wheel {
        delta: i16,
    },
}

impl KbmEvent {
    pub fn encode(&self) -> [u8; KBM_EVENT_LEN] {
        let mut out = [0u8; KBM_EVENT_LEN];
        match *self {
            Self::Key { vk, extended, down } => {
                out[0] = 1;
                out[1..3].copy_from_slice(&vk.to_be_bytes());
                out[3] = u8::from(extended);
                out[4] = u8::from(down);
            }
            Self::Move { dx, dy } => {
                out[0] = 2;
                out[1..3].copy_from_slice(&dx.to_be_bytes());
                out[3..5].copy_from_slice(&dy.to_be_bytes());
            }
            Self::Button { button, down } => {
                out[0] = 3;
                out[1] = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                };
                out[2] = u8::from(down);
            }
            Self::Wheel { delta } => {
                out[0] = 4;
                out[1..3].copy_from_slice(&delta.to_be_bytes());
            }
        }
        out
    }

    /// `None` para um evento que não conhecemos (de uma versão mais nova): é só ignorado.
    pub fn decode(bytes: &[u8; KBM_EVENT_LEN]) -> Option<Self> {
        let u16_at = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]);
        let i16_at = |i: usize| i16::from_be_bytes([bytes[i], bytes[i + 1]]);
        Some(match bytes[0] {
            1 => Self::Key {
                vk: u16_at(1),
                extended: bytes[3] != 0,
                down: bytes[4] != 0,
            },
            2 => Self::Move {
                dx: i16_at(1),
                dy: i16_at(3),
            },
            3 => Self::Button {
                button: match bytes[1] {
                    0 => MouseButton::Left,
                    1 => MouseButton::Right,
                    2 => MouseButton::Middle,
                    _ => return None,
                },
                down: bytes[2] != 0,
            },
            4 => Self::Wheel { delta: i16_at(1) },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::buttons;

    #[test]
    fn datagrama_de_controle_vai_e_volta_igual() {
        let pad = PadState {
            buttons: buttons::A | buttons::DPAD_UP,
            left_trigger: 7,
            right_trigger: 255,
            thumb_lx: -32768,
            thumb_ly: 32767,
            thumb_rx: -2,
            thumb_ry: 300,
        };
        let bytes = encode_pad(u32::MAX - 1, &pad);
        assert_eq!(bytes.len(), 17);
        assert_eq!(decode_pad(&bytes), Some((u32::MAX - 1, pad)));
    }

    #[test]
    fn datagrama_de_outro_tipo_ou_truncado_e_ignorado() {
        let bytes = encode_pad(1, &PadState::default());
        assert_eq!(decode_pad(&bytes[..16]), None);
        let mut other = bytes;
        other[0] = 9;
        assert_eq!(decode_pad(&other), None);
    }

    #[test]
    fn sequencia_mais_nova_mesmo_dando_a_volta() {
        assert!(is_newer(0, None));
        assert!(is_newer(5, Some(4)));
        assert!(!is_newer(4, Some(4)), "repetido");
        assert!(!is_newer(3, Some(4)), "atrasado");
        assert!(is_newer(1, Some(u32::MAX)), "deu a volta");
    }

    #[test]
    fn eventos_de_teclado_e_mouse_vao_e_voltam_iguais() {
        for event in [
            KbmEvent::Key {
                vk: 0x41,
                extended: false,
                down: true,
            },
            KbmEvent::Key {
                vk: 0x25,
                extended: true,
                down: false,
            },
            KbmEvent::Move { dx: -300, dy: 12 },
            KbmEvent::Button {
                button: MouseButton::Right,
                down: true,
            },
            KbmEvent::Wheel { delta: -120 },
        ] {
            assert_eq!(KbmEvent::decode(&event.encode()), Some(event));
        }
        assert_eq!(KbmEvent::decode(&[9, 0, 0, 0, 0, 0, 0, 0]), None);
    }
}
