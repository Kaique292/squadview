//! A superfície de vídeo de uma sessão (prévia ou stream de um amigo): guarda o que a interface
//! pediu (posição, cantos, picture in picture) até o vídeo existir, e aplica nele.

use std::sync::Mutex;

use sv_media::{Bounds, WindowControl};

/// O que a interface pediu, guardado até a superfície existir (ela abre no primeiro quadro).
#[derive(Default)]
pub(crate) struct Surface {
    pub control: Option<WindowControl>,
    pub bounds: Bounds,
    /// Raio dos cantos do quadro (pixels físicos), para o vídeo por baixo da interface (ADR 0021).
    pub radius: u32,
    /// A janela do picture in picture onde o vídeo está agora (`None`: a do app) e a área dele lá.
    /// A área do app (`bounds`) fica guardada para a volta.
    pub host: Option<(isize, Bounds)>,
}

impl Surface {
    pub fn apply(&self) {
        if let Some(c) = &self.control {
            c.set_corner_radius(self.radius);
            c.set_host(self.host.map(|(hwnd, _)| hwnd));
            c.set_bounds(self.host.map_or(self.bounds, |(_, area)| area));
        }
    }

    /// O vídeo abriu: aplica o que a interface já pediu.
    pub fn attach(&mut self, control: WindowControl) {
        self.control = Some(control);
        self.apply();
    }
}

/// Atalho para travar um Mutex mesmo se outra thread entrou em pânico segurando ele.
pub(crate) fn lock<T: ?Sized>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn sem_o_video_ainda_guarda_o_pedido() {
        let mut s = Surface {
            radius: 8,
            ..Surface::default()
        };
        // Sem vídeo ainda: aplicar não faz nada (nem trava).
        s.apply();
        s.bounds.width = 640;
        assert!(s.control.is_none());
    }

    #[test]
    fn por_baixo_da_interface_o_picture_in_picture_usa_a_area_da_janelinha_e_volta_ao_lugar() {
        use sv_gpu::Size;
        use sv_media::{VideoWindow, WindowTarget};
        let app = VideoWindow::open(
            WindowTarget::Standalone {
                title: "app (surface)".into(),
            },
            Size::new(1, 1),
        )
        .unwrap();
        let pip = VideoWindow::open(
            WindowTarget::Standalone {
                title: "pip (surface)".into(),
            },
            Size::new(1, 1),
        )
        .unwrap();
        let app_hwnd = app.control().hwnd();
        let Ok(video) = VideoWindow::open(WindowTarget::Composed { parent: app_hwnd }, Size::new(1920, 1080)) else {
            eprintln!("pulado: sem DirectComposition aqui");
            return;
        };
        let layer = video.control().layer().unwrap().clone();
        layer.set_video(Size::new(1920, 1080));
        let quadro = Bounds {
            x: 10,
            y: 20,
            width: 640,
            height: 360,
        };
        let mut s = Surface {
            bounds: quadro,
            radius: 8,
            ..Surface::default()
        };
        s.attach(video.control());
        assert_eq!(layer.placed(), Some((app_hwnd, quadro)));

        // No picture in picture: a área da janelinha.
        let janelinha = Bounds {
            x: 0,
            y: 0,
            width: 480,
            height: 270,
        };
        s.host = Some((pip.control().hwnd(), janelinha));
        s.apply();
        assert_eq!(layer.placed(), Some((pip.control().hwnd(), janelinha)));
        // Voltou: o último lugar do app.
        s.host = None;
        s.apply();
        assert_eq!(layer.placed(), Some((app_hwnd, quadro)));
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
