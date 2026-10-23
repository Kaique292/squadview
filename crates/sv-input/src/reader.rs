//! Lê o controle físico do jogador sem parar, numa thread própria (ADR 0023): 1000 leituras por
//! segundo, e só avisa quando o estado muda (com a zona morta aplicada). A leitura do XInput custa
//! microssegundos, e ler a cada 1 ms faz o atraso daqui ficar abaixo do próprio controle (que manda
//! a cada 4 a 8 ms).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};

use crate::virtual_pad::is_ours;
use crate::{PadState, xinput};

/// Intervalo entre leituras (1000 por segundo).
pub const READ_EVERY: Duration = Duration::from_millis(1);
/// Sem controle ligado, procura de novo a cada tanto (o jogador pode ligar um no meio do jogo).
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// A thread que lê o controle. Para ao ser descartada.
#[derive(Debug)]
pub struct PadReader {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PadReader {
    /// Começa a ler o primeiro controle ligado. `on_change` recebe cada estado novo (e um estado
    /// solto quando o controle é desligado). Não aloca no laço.
    pub fn start(mut on_change: impl FnMut(PadState) + Send + 'static) -> std::io::Result<Self> {
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
                let mut slot: Option<u32> = None;
                let mut last = PadState::default();
                let mut next_look = Instant::now();
                while alive.load(Ordering::Relaxed) {
                    let tick = Instant::now();
                    if slot.is_none() && tick >= next_look {
                        // Os controles virtuais que nós mesmos criamos (host no mesmo PC) não contam.
                        slot = (0..xinput::SLOTS).find(|&s| !is_ours(s) && xinput::read(s).is_some());
                        next_look = tick + LOOK_EVERY;
                    }
                    let state = match slot.filter(|&s| !is_ours(s)).and_then(xinput::read) {
                        Some(raw) => raw.with_deadzone(),
                        None => {
                            // Desligou: solta tudo e volta a procurar.
                            slot = None;
                            PadState::default()
                        }
                    };
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
        let reader = PadReader::start(|_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(reader);
    }
}
