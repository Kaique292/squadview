//! Teclado e mouse do jogador viram um controle (ADR 0023 e 0024): cada tecla aperta um botão ou
//! empurra um analógico, e o mouse vira o analógico direito.
//!
//! As teclas vêm da página como `KeyboardEvent.code` ("KeyW", "ArrowLeft", "Space"...), que é a
//! posição da tecla e não a letra: o mapeamento vale igual em teclado ABNT, US ou outro.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::PadState;
use crate::pad::buttons;

/// O que uma tecla faz no controle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PadInput {
    A,
    B,
    X,
    Y,
    Start,
    Back,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftThumb,
    RightThumb,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    RightStickUp,
    RightStickDown,
    RightStickLeft,
    RightStickRight,
}

/// Tecla (`KeyboardEvent.code`) → entrada do controle. Salvo em `arcade.json` pelo app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMap {
    pub keys: BTreeMap<String, PadInput>,
}

impl Default for KeyMap {
    /// WASD (e setas) no analógico esquerdo, J K L I nos botões, U/O nos ombros, Q/E nos
    /// gatilhos, Enter no Start, Backspace no Back e o teclado numérico no direcional.
    fn default() -> Self {
        use PadInput::*;
        let keys = [
            ("KeyW", LeftStickUp),
            ("KeyS", LeftStickDown),
            ("KeyA", LeftStickLeft),
            ("KeyD", LeftStickRight),
            ("ArrowUp", LeftStickUp),
            ("ArrowDown", LeftStickDown),
            ("ArrowLeft", LeftStickLeft),
            ("ArrowRight", LeftStickRight),
            ("KeyJ", A),
            ("Space", A),
            ("KeyK", B),
            ("KeyL", X),
            ("KeyI", Y),
            ("KeyU", LeftShoulder),
            ("KeyO", RightShoulder),
            ("KeyQ", LeftTrigger),
            ("KeyE", RightTrigger),
            ("Enter", Start),
            ("Backspace", Back),
            ("Numpad8", DpadUp),
            ("Numpad2", DpadDown),
            ("Numpad4", DpadLeft),
            ("Numpad6", DpadRight),
        ];
        Self {
            keys: keys.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        }
    }
}

/// O teclado e o mouse de um jogador, já como controle.
#[derive(Debug, Default)]
pub struct KeyboardPad {
    map: KeyMap,
    /// Teclas apertadas agora (o código), não as funções: com W e ↑ no mesmo analógico, soltar uma
    /// não solta a função enquanto a outra está apertada.
    held: HashSet<String>,
    stick: MouseStick,
    mouse_left: bool,
    mouse_right: bool,
}

impl KeyboardPad {
    pub fn new(map: KeyMap) -> Self {
        Self { map, ..Self::default() }
    }

    pub fn set_map(&mut self, map: KeyMap) {
        self.map = map;
        self.held.clear();
    }

    /// Uma tecla foi apertada ou solta. Devolve se o controle mudou.
    pub fn key(&mut self, code: &str, down: bool) -> bool {
        if !self.map.keys.contains_key(code) {
            return false;
        }
        let before = self.state();
        if down {
            self.held.insert(code.to_owned());
        } else {
            self.held.remove(code);
        }
        before != self.state()
    }

    /// Movimento do mouse (com o ponteiro preso na janela do fliperama).
    pub fn mouse_move(&mut self, dx: f32, dy: f32) {
        self.stick.push(dx, dy);
    }

    /// Botões do mouse: esquerdo no gatilho direito, direito no gatilho esquerdo (ADR 0024).
    pub fn mouse_buttons(&mut self, left: bool, right: bool) -> bool {
        let changed = (left, right) != (self.mouse_left, self.mouse_right);
        self.mouse_left = left;
        self.mouse_right = right;
        changed
    }

    /// Solta tudo (a janela perdeu o foco, ou o jogador saiu).
    pub fn release_all(&mut self) {
        self.held.clear();
        self.stick = MouseStick::default();
        self.mouse_left = false;
        self.mouse_right = false;
    }

    /// Passa o tempo para o analógico do mouse voltar ao centro. Chamar a cada leitura.
    pub fn tick(&mut self, dt: std::time::Duration) {
        self.stick.decay(dt);
    }

    /// O controle que esse teclado e mouse formam agora.
    pub fn state(&self) -> PadState {
        use PadInput::*;
        let mut pad = PadState::default();
        let pressed: HashSet<PadInput> = self.held.iter().filter_map(|k| self.map.keys.get(k).copied()).collect();
        let axis = |neg: PadInput, pos: PadInput| -> i16 {
            match (pressed.contains(&neg), pressed.contains(&pos)) {
                (true, false) => i16::MIN,
                (false, true) => i16::MAX,
                _ => 0,
            }
        };
        for &input in &pressed {
            pad.buttons |= match input {
                A => buttons::A,
                B => buttons::B,
                X => buttons::X,
                Y => buttons::Y,
                Start => buttons::START,
                Back => buttons::BACK,
                LeftShoulder => buttons::LEFT_SHOULDER,
                RightShoulder => buttons::RIGHT_SHOULDER,
                LeftThumb => buttons::LEFT_THUMB,
                RightThumb => buttons::RIGHT_THUMB,
                DpadUp => buttons::DPAD_UP,
                DpadDown => buttons::DPAD_DOWN,
                DpadLeft => buttons::DPAD_LEFT,
                DpadRight => buttons::DPAD_RIGHT,
                _ => 0,
            };
        }
        pad.left_trigger = if pressed.contains(&LeftTrigger) || self.mouse_right {
            255
        } else {
            0
        };
        pad.right_trigger = if pressed.contains(&RightTrigger) || self.mouse_left {
            255
        } else {
            0
        };
        pad.thumb_lx = axis(LeftStickLeft, LeftStickRight);
        // Para cima é positivo no XInput.
        pad.thumb_ly = axis(LeftStickDown, LeftStickUp);
        let (mx, my) = self.stick.value();
        pad.thumb_rx = match axis(RightStickLeft, RightStickRight) {
            0 => mx,
            v => v,
        };
        pad.thumb_ry = match axis(RightStickDown, RightStickUp) {
            0 => my,
            v => v,
        };
        pad
    }
}

/// O mouse como analógico (ADR 0024): o desvio acompanha a velocidade do mouse e volta ao centro
/// quando ele para.
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseStick {
    /// Movimento acumulado, em "pixels", já decaindo.
    x: f32,
    y: f32,
}

/// Quanto movimento (em pixels) leva o analógico até o fim.
const FULL_TILT_PX: f32 = 24.0;
/// Meia-vida do movimento acumulado: em ~30 ms parado, o analógico volta à metade.
const HALF_LIFE_S: f32 = 0.030;

impl MouseStick {
    pub fn push(&mut self, dx: f32, dy: f32) {
        self.x = (self.x + dx).clamp(-FULL_TILT_PX, FULL_TILT_PX);
        self.y = (self.y + dy).clamp(-FULL_TILT_PX, FULL_TILT_PX);
    }

    pub fn decay(&mut self, dt: std::time::Duration) {
        let keep = 0.5f32.powf(dt.as_secs_f32() / HALF_LIFE_S);
        self.x *= keep;
        self.y *= keep;
        if self.x.abs() < 0.05 {
            self.x = 0.0;
        }
        if self.y.abs() < 0.05 {
            self.y = 0.0;
        }
    }

    /// O analógico (para cima é positivo, ao contrário da tela).
    pub fn value(&self) -> (i16, i16) {
        let to_axis = |v: f32| (v / FULL_TILT_PX * f32::from(i16::MAX)).round() as i16;
        (to_axis(self.x), to_axis(-self.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn wasd_e_botoes_viram_controle() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        assert!(pad.key("KeyW", true));
        assert!(pad.key("KeyJ", true));
        assert!(pad.key("Enter", true));
        let s = pad.state();
        assert_eq!(s.thumb_ly, i16::MAX, "W empurra o analógico para cima");
        assert!(s.pressed(buttons::A) && s.pressed(buttons::START));
        pad.key("KeyW", false);
        assert_eq!(pad.state().thumb_ly, 0);
    }

    #[test]
    fn duas_teclas_na_mesma_funcao_so_soltam_juntas() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        pad.key("KeyW", true);
        assert!(!pad.key("ArrowUp", true), "a função já estava apertada");
        assert!(!pad.key("KeyW", false), "a seta continua apertando");
        assert_eq!(pad.state().thumb_ly, i16::MAX);
        assert!(pad.key("ArrowUp", false));
        assert_eq!(pad.state().thumb_ly, 0);
    }

    #[test]
    fn direcoes_opostas_se_anulam_e_tecla_sem_funcao_nao_muda_nada() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        pad.key("KeyA", true);
        pad.key("KeyD", true);
        assert_eq!(pad.state().thumb_lx, 0);
        assert!(!pad.key("KeyZ", true));
    }

    #[test]
    fn gatilhos_pelo_teclado_e_pelo_mouse() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        assert!(pad.mouse_buttons(true, false));
        assert_eq!(pad.state().right_trigger, 255, "botão esquerdo é o gatilho direito");
        assert!(!pad.mouse_buttons(true, false), "nada mudou");
        pad.key("KeyQ", true);
        assert_eq!(pad.state().left_trigger, 255);
    }

    #[test]
    fn mouse_empurra_o_analogico_direito_e_ele_volta_ao_centro() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        pad.mouse_move(100.0, -5.0);
        let (rx, ry) = (pad.state().thumb_rx, pad.state().thumb_ry);
        assert_eq!(rx, i16::MAX, "movimento grande vai até o fim");
        assert!(ry > 0, "mouse para cima empurra o analógico para cima");
        pad.tick(Duration::from_millis(30));
        assert!(pad.state().thumb_rx < i16::MAX / 2 + 100, "em 30 ms volta à metade");
        pad.tick(Duration::from_millis(500));
        assert_eq!(pad.state().thumb_rx, 0, "parado, volta ao centro");
    }

    #[test]
    fn soltar_tudo_limpa_teclas_mouse_e_analogico() {
        let mut pad = KeyboardPad::new(KeyMap::default());
        pad.key("KeyW", true);
        pad.mouse_buttons(true, true);
        pad.mouse_move(10.0, 10.0);
        pad.release_all();
        assert_eq!(pad.state(), PadState::default());
    }

    #[test]
    fn mapeamento_vai_e_volta_em_json() {
        let map = KeyMap::default();
        let json = serde_json::to_string(&map).unwrap();
        assert!(json.contains(r#""KeyW":"left-stick-up""#));
        assert_eq!(serde_json::from_str::<KeyMap>(&json).unwrap(), map);
    }
}
