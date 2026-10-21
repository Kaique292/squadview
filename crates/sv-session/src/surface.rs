//! A superfície de vídeo de uma sessão (prévia ou stream de um amigo): guarda o que a interface
//! pediu (posição, escondida, eventos de tela cheia) até a janela nativa existir, e aplica nela.

use std::sync::{Arc, Mutex};

use sv_media::{Bounds, Hole, WindowControl, WindowEvent};

pub(crate) type OnWindowEvent = Arc<dyn Fn(WindowEvent) + Send + Sync>;

/// Posição pedida pela interface, guardada até a superfície existir (ela abre no primeiro quadro).
#[derive(Default)]
pub(crate) struct Surface {
    pub control: Option<WindowControl>,
    pub bounds: Bounds,
    pub hidden: bool,
    /// Onde a interface está por cima do vídeo agora (pixels físicos, relativos à janela do app).
    pub holes: Vec<Hole>,
    /// Solto da janela do app, pequeno e sempre no topo (picture in picture).
    pub pip: bool,
    pub on_event: Option<OnWindowEvent>,
}

impl Surface {
    pub fn apply(&self) {
        if let Some(c) = &self.control {
            // Primeiro o lugar dentro do app (fica guardado enquanto o vídeo está solto), depois o PiP.
            c.set_bounds(if self.hidden { Bounds::default() } else { self.bounds });
            c.set_holes(&local_holes(self.bounds, &self.holes));
            c.set_pip(self.pip);
        }
    }
}

/// Os buracos que caem sobre o vídeo, relativos ao canto dele (o que fica fora é ignorado).
///
/// O buraco **não** é cortado na borda do vídeo: um menu que começa na barra lateral e entra no
/// vídeo mantém o tamanho inteiro, com os cantos arredondados fora da janela (onde não aparecem).
/// Cortado, o corte ganhava os cantos arredondados bem na borda, e eles viravam entalhes pretos.
pub(crate) fn local_holes(video: Bounds, holes: &[Hole]) -> Vec<Hole> {
    let (vr, vb) = (video.x + video.width as i32, video.y + video.height as i32);
    holes
        .iter()
        .filter(|h| {
            let b = h.bounds;
            b.x < vr && b.y < vb && b.x + b.width as i32 > video.x && b.y + b.height as i32 > video.y
        })
        .map(|&Hole { bounds: h, radius }| Hole {
            bounds: Bounds {
                x: h.x - video.x,
                y: h.y - video.y,
                width: h.width,
                height: h.height,
            },
            radius,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_posicao_e_callback_ate_a_janela_existir() {
        let mut s = Surface {
            bounds: Bounds {
                x: 10,
                y: 20,
                width: 640,
                height: 360,
            },
            hidden: true,
            ..Surface::default()
        };
        // Sem janela ainda: aplicar não faz nada (nem trava).
        s.apply();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        s.set_on_event(Arc::new(move |e| lock(&seen).push(e)));
        let cb = s.on_event.clone().expect("callback guardado");
        cb(WindowEvent::ToggleFullscreen);
        assert_eq!(*lock(&calls), vec![WindowEvent::ToggleFullscreen]);
        assert!(s.control.is_none());
    }

    #[test]
    fn buracos_ficam_relativos_ao_video_e_so_o_que_cai_sobre_ele() {
        let video = Bounds {
            x: 300,
            y: 50,
            width: 1000,
            height: 600,
        };
        let holes = [
            // Menu que começa na barra lateral e entra 200 px no vídeo (o canto arredondado vai junto).
            Hole {
                bounds: Bounds {
                    x: 250,
                    y: 200,
                    width: 250,
                    height: 180,
                },
                radius: 8,
            },
            // Dica toda fora do vídeo.
            Hole {
                bounds: Bounds {
                    x: 10,
                    y: 10,
                    width: 100,
                    height: 30,
                },
                radius: 0,
            },
        ];
        assert_eq!(
            local_holes(video, &holes),
            // Inteiro, começando antes do vídeo: o canto arredondado da esquerda fica fora dele.
            vec![Hole {
                bounds: Bounds {
                    x: -50,
                    y: 150,
                    width: 250,
                    height: 180,
                },
                radius: 8,
            }]
        );
        assert!(local_holes(video, &[]).is_empty());
    }

    #[test]
    fn lock_continua_funcionando_depois_de_um_panico() {
        let m = Arc::new(Mutex::new(1));
        let poisoned = m.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("pânico de propósito segurando o lock");
        })
        .join();
        assert!(m.is_poisoned());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
    }
}
