//! Controles virtuais de Xbox 360 pelo ViGEmBus: o jogo do host enxerga cada jogador do
//! fliperama como um controle plugado no PC (ADR 0023).

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use vigem_rust::{Client, ClientError, TargetHandle, Xbox360};

use crate::{PadState, xinput};

/// Quanto esperar o Windows aceitar um controle recém-plugado.
const READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Conexão com o ViGEmBus. Só existe enquanto um fliperama está aberto: sem ela, o app não
/// fala com o driver.
pub struct VirtualPads {
    client: Client,
}

impl std::fmt::Debug for VirtualPads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualPads")
            .field("pads", &self.client.target_count())
            .finish()
    }
}

impl VirtualPads {
    /// Conecta ao driver. Falha se o ViGEmBus não está instalado (ver [`crate::driver_status`]).
    pub fn connect() -> Result<Self, ClientError> {
        Ok(Self {
            client: Client::connect()?,
        })
    }

    /// Pluga um controle de Xbox 360 novo, espera o Windows aceitar e descobre a vaga do XInput
    /// que ele ganhou (`slot` fica `None` se não deu para saber em [`SLOT_TIMEOUT`]).
    pub fn plug(&self) -> Result<VirtualPad, ClientError> {
        let before = xinput::connected();
        let target = self
            .client
            .new_x360_target()
            .plug()?
            .wait_for_ready_timeout(READY_TIMEOUT)?;
        let slot = find_slot(&target, before);
        if let Some(s) = slot {
            OUR_SLOTS.fetch_or(1 << s, Ordering::SeqCst);
        }
        Ok(VirtualPad { target, slot })
    }
}

/// Vagas do XInput ocupadas pelos controles virtuais que este app criou (um bit por vaga). Quem lê
/// o controle físico do jogador ignora essas vagas: com o host e o jogador no mesmo PC, ele leria
/// o controle virtual do host como se fosse o próprio, e um botão apertado ficaria preso em laço.
static OUR_SLOTS: AtomicU8 = AtomicU8::new(0);

/// A vaga `slot` do XInput é um controle virtual criado por este app.
pub fn is_ours(slot: u32) -> bool {
    slot < xinput::SLOTS && OUR_SLOTS.load(Ordering::SeqCst) & (1 << slot) != 0
}

/// Quanto esperar o Windows dar uma vaga do XInput ao controle novo.
pub const SLOT_TIMEOUT: Duration = Duration::from_secs(2);

/// A vaga do controle recém-plugado. O driver só sabe depois que o Windows atribui a vaga (logo
/// depois do plug ele responde com erro); enquanto isso, a vaga que passou a estar ocupada no
/// XInput também diz qual é.
fn find_slot(target: &TargetHandle<Xbox360>, before: [bool; xinput::SLOTS as usize]) -> Option<u32> {
    let deadline = Instant::now() + SLOT_TIMEOUT;
    loop {
        if let Ok(slot) = target.user_index()
            && slot < xinput::SLOTS
        {
            return Some(slot);
        }
        if let Some(slot) = new_slot(before, xinput::connected()) {
            return Some(slot);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A única vaga que estava livre e agora está ocupada (com duas ou mais, não dá para saber).
fn new_slot(before: [bool; xinput::SLOTS as usize], after: [bool; xinput::SLOTS as usize]) -> Option<u32> {
    let mut appeared = (0..xinput::SLOTS).filter(|&s| !before[s as usize] && after[s as usize]);
    let slot = appeared.next()?;
    appeared.next().is_none().then_some(slot)
}

/// Um controle virtual plugado. Sai do PC quando é descartado.
#[derive(Debug)]
pub struct VirtualPad {
    target: TargetHandle<Xbox360>,
    slot: Option<u32>,
}

impl VirtualPad {
    /// Aplica o estado do controle. Não aloca: pode ser chamado a cada mudança.
    pub fn update(&self, state: &PadState) -> Result<(), ClientError> {
        self.target.update(&state.to_x360())
    }

    /// A vaga do XInput que o Windows deu a este controle (`0..4`): o jogador é o P`vaga + 1`
    /// para os jogos que numeram pelo XInput. `None` se o Windows não deu vaga a tempo.
    pub fn user_index(&self) -> Option<u32> {
        self.slot
    }
}

impl Drop for VirtualPad {
    fn drop(&mut self) {
        // Solta tudo antes de sair, para nenhum botão ficar "preso" no jogo.
        let _ = self.target.update(&PadState::default().to_x360());
        let _ = self.target.unplug();
        if let Some(s) = self.slot {
            OUR_SLOTS.fetch_and(!(1 << s), Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaga_nova_so_quando_uma_unica_aparece() {
        let free = [false; 4];
        assert_eq!(new_slot(free, [false, true, false, false]), Some(1));
        assert_eq!(
            new_slot([true, false, false, false], [true, false, true, false]),
            Some(2)
        );
        assert_eq!(new_slot(free, free), None, "nada apareceu");
        assert_eq!(new_slot(free, [true, true, false, false]), None, "duas apareceram");
        assert_eq!(
            new_slot([true, true, false, false], [true, false, false, false]),
            None,
            "uma saiu"
        );
    }

    #[test]
    #[ignore = "precisa do ViGEmBus instalado"]
    fn pluga_atualiza_e_tira_um_controle_virtual() {
        let pads = VirtualPads::connect().expect("ViGEmBus instalado");
        let pad = pads.plug().expect("controle plugado");
        let slot = pad.user_index().expect("vaga do XInput");
        assert!(slot < crate::xinput::SLOTS, "vaga {slot}");

        let state = PadState {
            buttons: crate::pad::buttons::A,
            thumb_lx: 20_000,
            ..PadState::default()
        };
        pad.update(&state).unwrap();
        // O Windows passa a ver o controle virtual na vaga dele, com o estado aplicado.
        let seen = (0..50)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(20));
                crate::xinput::read(slot).filter(|s| *s == state)
            })
            .expect("o XInput vê o estado do controle virtual");
        assert!(seen.pressed(crate::pad::buttons::A));

        assert!(is_ours(slot), "a vaga do controle virtual é marcada como nossa");

        drop(pad);
        assert!(!is_ours(slot), "ao sair, a vaga deixa de ser nossa");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(crate::xinput::read(slot), None, "o controle saiu do PC");
    }
}
