/// Deixa passar no máximo `fps` frames por segundo.
///
/// O Windows entrega frames na taxa do monitor (144 Hz, 240 Hz…) sempre que a tela muda;
/// o encoder só precisa da taxa escolhida pelo usuário. Uma pequena tolerância evita
/// descartar frames que chegam um pouco antes da hora por variação do relógio.
#[derive(Debug, Clone)]
pub struct FrameThrottle {
    interval_us: u64,
    next_due_us: Option<u64>,
}

impl FrameThrottle {
    pub fn new(fps: u32) -> Self {
        Self {
            interval_us: 1_000_000 / u64::from(fps.max(1)),
            next_due_us: None,
        }
    }

    /// Decide se o frame capturado em `timestamp_us` deve ser usado.
    pub fn accept(&mut self, timestamp_us: u64) -> bool {
        let tolerance = self.interval_us / 8;
        match self.next_due_us {
            Some(due) if timestamp_us + tolerance < due => false,
            Some(due) => {
                // Se atrasou mais de um intervalo (tela parada), recomeça a contagem a partir de agora.
                let next = due + self.interval_us;
                self.next_due_us = Some(if timestamp_us >= next {
                    timestamp_us + self.interval_us
                } else {
                    next
                });
                true
            }
            None => {
                self.next_due_us = Some(timestamp_us + self.interval_us);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simula um monitor de `hz` por 1 segundo e conta quantos frames passam.
    fn frames_aceitos(fps: u32, hz: u64) -> usize {
        let mut t = FrameThrottle::new(fps);
        (0..hz).filter(|i| t.accept(i * 1_000_000 / hz)).count()
    }

    #[test]
    fn monitor_de_144hz_vira_60fps() {
        let n = frames_aceitos(60, 144);
        assert!((58..=61).contains(&n), "aceitou {n}");
    }

    #[test]
    fn monitor_de_75hz_vira_60fps() {
        let n = frames_aceitos(60, 75);
        assert!((58..=61).contains(&n), "aceitou {n}");
    }

    #[test]
    fn monitor_de_60hz_em_60fps_nao_perde_frames() {
        assert_eq!(frames_aceitos(60, 60), 60);
    }

    #[test]
    fn monitor_de_240hz_vira_30fps() {
        let n = frames_aceitos(30, 240);
        assert!((29..=31).contains(&n), "aceitou {n}");
    }

    #[test]
    fn depois_de_tela_parada_aceita_o_proximo_frame_na_hora() {
        let mut t = FrameThrottle::new(60);
        assert!(t.accept(0));
        // 2 segundos sem mudança na tela.
        assert!(t.accept(2_000_000));
        // O seguinte precisa respeitar o intervalo a partir de agora, e não de antes.
        assert!(!t.accept(2_005_000));
        assert!(t.accept(2_016_667));
    }

    #[test]
    fn fps_zero_nao_divide_por_zero() {
        let mut t = FrameThrottle::new(0);
        assert!(t.accept(0));
    }
}
