//! Lê os controles físicos do jogador sem parar, numa thread própria (ADR 0023 e 0031): 1000
//! leituras por segundo, e só avisa quando o estado muda (com a zona morta aplicada). A leitura do
//! XInput custa microssegundos, e ler a cada 1 ms faz o atraso daqui ficar abaixo do próprio
//! controle (que manda a cada 4 a 8 ms).
//!
//! Lê **todos** os controles ligados e junta os estados: os de Xbox pelo XInput e os de PlayStation,
//! Switch e genéricos pelo Windows.Gaming.Input ([`crate::gamepads`]). Antes só o primeiro controle
//! do XInput valia: com dois ligados, o leitor podia ficar com o errado, e um DualSense nem aparecia.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};

use crate::gamepads::{OtherPads, PadDevice, PadKind};
use crate::virtual_pad::is_ours;
use crate::{PadState, xinput};

/// Intervalo entre leituras (1000 por segundo).
pub const READ_EVERY: Duration = Duration::from_millis(1);
/// Vagas do XInput sem controle são olhadas de novo a cada tanto (o jogador pode ligar um no meio
/// do jogo). Ler uma vaga vazia é caro, então não é a cada leitura.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// A thread que lê os controles. Para ao ser descartada.
#[derive(Debug)]
pub struct PadReader {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PadReader {
    /// Começa a ler os controles ligados. `on_change` recebe cada estado novo (todos os controles
    /// somados, e o controle solto quando o último é desligado). `on_devices` recebe a lista dos
    /// controles achados logo no começo e sempre que um é ligado ou desligado. O laço não aloca;
    /// só a lista, quando ela muda.
    pub fn start(
        mut on_change: impl FnMut(PadState) + Send + 'static,
        mut on_devices: impl FnMut(&[PadDevice]) + Send + 'static,
    ) -> std::io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let alive = running.clone();
        let thread = std::thread::Builder::new()
            .name("sv-pad-reader".into())
            .spawn(move || {
                // Acima do normal: o jogador pode estar com o PC ocupado, e um botão não pode
                // esperar a vez de outra thread.
                // SAFETY: muda só a prioridade da própria thread.
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
                }
                // O `gilrs` não é `Send`: nasce aqui dentro.
                let mut others = OtherPads::new();
                let mut present = [false; xinput::SLOTS as usize];
                let mut last = PadState::default();
                let mut next_look = Instant::now();
                let mut first = true;
                while alive.load(Ordering::Relaxed) {
                    let tick = Instant::now();
                    let mut devices_changed = first;
                    first = false;
                    if tick >= next_look {
                        for slot in 0..xinput::SLOTS {
                            // Os controles virtuais que nós mesmos criamos (host no mesmo PC) não contam.
                            if !present[slot as usize] && !is_ours(slot) && xinput::read(slot).is_some() {
                                present[slot as usize] = true;
                                devices_changed = true;
                            }
                        }
                        next_look = tick + LOOK_EVERY;
                    }
                    let mut state = PadState::default();
                    for slot in 0..xinput::SLOTS {
                        if !present[slot as usize] {
                            continue;
                        }
                        match xinput::read(slot).filter(|_| !is_ours(slot)) {
                            Some(raw) => state = state.merge(raw.with_deadzone()),
                            None => {
                                // Desligou: sai da soma e da lista.
                                present[slot as usize] = false;
                                devices_changed = true;
                            }
                        }
                    }
                    if let Some(others) = others.as_mut() {
                        devices_changed |= others.poll().devices;
                        state = state.merge(others.state());
                    }
                    if devices_changed {
                        on_devices(&device_list(&present, others.as_ref()));
                    }
                    if state != last {
                        last = state;
                        on_change(state);
                    }
                    if let Some(rest) = READ_EVERY.checked_sub(tick.elapsed()) {
                        std::thread::sleep(rest);
                    }
                }
            })?;
        Ok(Self {
            running,
            thread: Some(thread),
        })
    }
}

/// Os controles achados: os do XInput (que não dizem o nome) e os do Windows.Gaming.Input.
fn device_list(present: &[bool], others: Option<&OtherPads>) -> Vec<PadDevice> {
    let xbox = present.iter().filter(|p| **p).map(|_| PadDevice {
        name: "Controle de Xbox".into(),
        kind: PadKind::Xbox,
        supported: true,
    });
    xbox.chain(others.map_or(&[][..], OtherPads::devices).iter().cloned())
        .collect()
}

impl Drop for PadReader {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn para_ao_ser_descartado() {
        // Sem controle (como na CI) não chega nada, e o descarte não trava.
        let reader = PadReader::start(|_| {}, |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(reader);
    }

    /// Regressão: o jogador não sabia se o controle tinha sido achado. A lista chega logo no
    /// começo, mesmo vazia, para a interface dizer "nenhum controle".
    #[test]
    fn avisa_a_lista_de_controles_logo_no_comeco() {
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = PadReader::start(
            |_| {},
            move |devices| {
                let _ = tx.send(devices.len());
            },
        )
        .unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok(), "a lista chegou");
        drop(reader);
    }

    #[test]
    fn controles_do_xinput_entram_na_lista_como_xbox() {
        let list = device_list(&[false, true, false, true], None);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|d| d.kind == PadKind::Xbox && d.supported));
        assert!(device_list(&[false; 4], None).is_empty());
    }
}
