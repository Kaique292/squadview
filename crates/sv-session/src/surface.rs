//! A superfície de vídeo de uma sessão (prévia ou stream de um amigo): guarda o que a interface
//! pediu (posição, escondida, eventos de tela cheia) até a janela nativa existir, e aplica nela.

use std::sync::{Arc, Mutex};

use sv_media::{Bounds, WindowControl, WindowEvent};

pub(crate) type OnWindowEvent = Arc<dyn Fn(WindowEvent) + Send + Sync>;

/// Posição pedida pela interface, guardada até a superfície existir (ela abre no primeiro quadro).
#[derive(Default)]
pub(crate) struct Surface {
    pub control: Option<WindowControl>,
    pub bounds: Bounds,
    pub hidden: bool,
    pub on_event: Option<OnWindowEvent>,
}

impl Surface {
    pub fn apply(&self) {
        if let Some(c) = &self.control {
            c.set_bounds(if self.hidden { Bounds::default() } else { self.bounds });
        }
    }
}

impl Surface {
    /// A janela nativa abriu: registra o callback de tela cheia e aplica a posição pedida.
    pub fn attach(&mut self, control: WindowControl) {
        if let Some(cb) = self.on_event.clone() {
            control.on_event(move |e| cb(e));
        }
        self.control = Some(control);
        self.apply();
    }

    pub fn set_on_event(&mut self, cb: OnWindowEvent) {
        if let Some(c) = &self.control {
            let cb = cb.clone();
            c.on_event(move |e| cb(e));
        }
        self.on_event = Some(cb);
    }
}

/// Atalho para travar um Mutex mesmo se outra thread entrou em pânico segurando ele.
pub(crate) fn lock<T: ?Sized>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
