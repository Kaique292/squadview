//! Controles que não são de Xbox (PlayStation, Switch e genéricos), pelo Windows.Gaming.Input com o
//! mapeamento do SDL (crate `gilrs`, ADR 0031). O XInput não enxerga esses controles: antes, quem
//! jogava com um DualShock ou um DualSense via o controle "não conectar".
//!
//! Os de Xbox e compatíveis continuam pelo XInput ([`crate::xinput`]), lido a 1000 Hz: aqui eles
//! são pulados, para não aparecerem duas vezes.

use gilrs::{Axis, Button, EventType, Gamepad, Gilrs, GilrsBuilder, MappingSource};
use serde::Serialize;

use crate::PadState;
use crate::pad::buttons;

/// A família de um controle, para a interface dizer qual foi achado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PadKind {
    Xbox,
    PlayStation,
    Nintendo,
    Other,
}

impl PadKind {
    /// Pela fabricante do controle (o `vendor id` do USB).
    pub fn from_vendor(vendor: Option<u16>) -> Self {
        match vendor {
            Some(0x045e) => Self::Xbox,
            Some(0x054c) => Self::PlayStation,
            Some(0x057e) => Self::Nintendo,
            _ => Self::Other,
        }
    }
}

/// Um controle ligado ao PC do jogador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadDevice {
    pub name: String,
    pub kind: PadKind,
    /// O app sabe quais são os botões dele. Sem mapeamento, os botões sairiam trocados, então ele
    /// fica de fora (e a interface avisa).
    pub supported: bool,
}

/// Os controles do Windows.Gaming.Input que não são de Xbox, e o estado deles somado.
pub struct OtherPads {
    gilrs: Gilrs,
    state: PadState,
    devices: Vec<PadDevice>,
}

impl std::fmt::Debug for OtherPads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtherPads")
            .field("state", &self.state)
            .field("devices", &self.devices)
            .finish()
    }
}

/// O que mudou numa leitura.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Polled {
    pub state: bool,
    pub devices: bool,
}

impl OtherPads {
    /// `None` se o Windows.Gaming.Input não está disponível (aí só o XInput vale).
    pub fn new() -> Option<Self> {
        let gilrs = GilrsBuilder::new().set_update_state(true).build().ok()?;
        let mut pads = Self {
            gilrs,
            state: PadState::default(),
            devices: Vec::new(),
        };
        pads.refresh_devices();
        Some(pads)
    }

    /// Processa o que chegou desde a última leitura. Não aloca, a não ser quando um controle é
    /// ligado ou desligado (a lista muda).
    pub fn poll(&mut self) -> Polled {
        let mut polled = Polled::default();
        while let Some(event) = self.gilrs.next_event() {
            polled.state = true;
            if matches!(event.event, EventType::Connected | EventType::Disconnected) {
                polled.devices = true;
            }
        }
        if polled.devices {
            self.refresh_devices();
        }
        if polled.state {
            let state = self
                .gilrs
                .gamepads()
                .filter(|(_, g)| !is_xinput(g) && is_mapped(g))
                .fold(PadState::default(), |acc, (_, g)| acc.merge(read(&g)));
            polled.state = state != self.state;
            self.state = state;
        }
        polled
    }

    /// Todos os controles daqui somados (com a zona morta).
    pub fn state(&self) -> PadState {
        self.state
    }

    pub fn devices(&self) -> &[PadDevice] {
        &self.devices
    }

    fn refresh_devices(&mut self) {
        self.devices = self
            .gilrs
            .gamepads()
            .filter(|(_, g)| !is_xinput(g))
            .map(|(_, g)| PadDevice {
                name: g.name().to_owned(),
                kind: PadKind::from_vendor(g.vendor_id()),
                supported: is_mapped(&g),
            })
            .collect();
    }
}

/// O controle é um "Gamepad" do Windows, isto é, de Xbox ou compatível: o XInput já lê. No `gilrs`,
/// só esses têm vibração (`is_ff_supported`), porque só eles passam pela classe `Gamepad`.
fn is_xinput(g: &Gamepad<'_>) -> bool {
    g.is_ff_supported()
}

/// Tem mapeamento do SDL (os botões estão no lugar certo).
fn is_mapped(g: &Gamepad<'_>) -> bool {
    g.mapping_source() == MappingSource::SdlMappings
}

fn read(g: &Gamepad<'_>) -> PadState {
    to_pad(
        |b| g.is_pressed(b),
        |a| g.value(a),
        |b| g.button_data(b).map_or(0.0, |d| d.value()),
    )
    .with_deadzone()
}

/// O estado de um controle do `gilrs` no layout do XInput. Os botões vão pela posição (o de baixo é
/// o A, como no controle de Xbox), e o "para cima" dos analógicos já é positivo nos dois.
pub(crate) fn to_pad(
    pressed: impl Fn(Button) -> bool,
    value: impl Fn(Axis) -> f32,
    trigger: impl Fn(Button) -> f32,
) -> PadState {
    const MAP: [(Button, u16); 15] = [
        (Button::South, buttons::A),
        (Button::East, buttons::B),
        (Button::West, buttons::X),
        (Button::North, buttons::Y),
        (Button::LeftTrigger, buttons::LEFT_SHOULDER),
        (Button::RightTrigger, buttons::RIGHT_SHOULDER),
        (Button::Select, buttons::BACK),
        (Button::Start, buttons::START),
        (Button::Mode, buttons::GUIDE),
        (Button::LeftThumb, buttons::LEFT_THUMB),
        (Button::RightThumb, buttons::RIGHT_THUMB),
        (Button::DPadUp, buttons::DPAD_UP),
        (Button::DPadDown, buttons::DPAD_DOWN),
        (Button::DPadLeft, buttons::DPAD_LEFT),
        (Button::DPadRight, buttons::DPAD_RIGHT),
    ];
    let stick = |a: Axis| (value(a).clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
    let trig = |b: Button| (trigger(b).clamp(0.0, 1.0) * 255.0).round() as u8;
    PadState {
        buttons: MAP
            .iter()
            .filter(|(b, _)| pressed(*b))
            .fold(0, |acc, (_, bit)| acc | bit),
        left_trigger: trig(Button::LeftTrigger2),
        right_trigger: trig(Button::RightTrigger2),
        thumb_lx: stick(Axis::LeftStickX),
        thumb_ly: stick(Axis::LeftStickY),
        thumb_rx: stick(Axis::RightStickX),
        thumb_ry: stick(Axis::RightStickY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fabricante_diz_a_familia_do_controle() {
        assert_eq!(PadKind::from_vendor(Some(0x054c)), PadKind::PlayStation);
        assert_eq!(PadKind::from_vendor(Some(0x057e)), PadKind::Nintendo);
        assert_eq!(PadKind::from_vendor(Some(0x045e)), PadKind::Xbox);
        assert_eq!(PadKind::from_vendor(Some(0x2dc8)), PadKind::Other, "8BitDo");
        assert_eq!(PadKind::from_vendor(None), PadKind::Other);
    }

    /// Regressão: um DualSense não era lido (só o XInput). O X de baixo do PlayStation tem que
    /// virar o A, o L2 o gatilho esquerdo e o analógico para cima continuar positivo.
    #[test]
    fn controle_de_playstation_vira_o_layout_do_xinput() {
        let pad = to_pad(
            |b| {
                matches!(
                    b,
                    Button::South | Button::Start | Button::DPadLeft | Button::LeftTrigger
                )
            },
            |a| match a {
                Axis::LeftStickY => 1.0,
                Axis::RightStickX => -0.5,
                _ => 0.0,
            },
            |b| if b == Button::LeftTrigger2 { 1.0 } else { 0.0 },
        );
        assert_eq!(
            pad.buttons,
            buttons::A | buttons::START | buttons::DPAD_LEFT | buttons::LEFT_SHOULDER
        );
        assert_eq!(pad.left_trigger, 255);
        assert_eq!(pad.right_trigger, 0);
        assert_eq!(pad.thumb_ly, i16::MAX);
        assert_eq!(pad.thumb_rx, -16384);
        assert_eq!(pad.thumb_lx, 0);
    }

    #[test]
    fn solto_e_fora_da_faixa_ficam_dentro_do_xinput() {
        let solto = to_pad(|_| false, |_| 0.0, |_| 0.0);
        assert_eq!(solto, PadState::default());
        let demais = to_pad(|_| false, |_| -3.0, |_| 2.0);
        assert_eq!(demais.thumb_lx, -i16::MAX);
        assert_eq!(demais.left_trigger, 255);
    }

    #[test]
    fn lista_dos_controles_vai_para_a_interface_em_camel_case() {
        let device = PadDevice {
            name: "DualSense Wireless Controller".into(),
            kind: PadKind::PlayStation,
            supported: true,
        };
        assert_eq!(
            serde_json::to_string(&device).unwrap(),
            r#"{"name":"DualSense Wireless Controller","kind":"playstation","supported":true}"#
        );
    }
}
