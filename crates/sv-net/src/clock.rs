//! Estimativa da diferença entre o relógio de quem transmite e o nosso (estilo NTP).
//!
//! A cada ping: `offset = remoto - (envio + chegada) / 2`. Vale a medição de menor ida e volta
//! (a menos afetada por fila na rede) entre as últimas.

/// Quantas medições guardar.
const WINDOW: usize = 16;

#[derive(Debug, Default)]
pub struct ClockEstimator {
    samples: Vec<(u64, i64)>,
}

impl ClockEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um pong: `sent_us` e `received_us` no nosso relógio, `remote_us` no do outro.
    pub fn record(&mut self, sent_us: u64, remote_us: u64, received_us: u64) {
        if received_us < sent_us {
            return;
        }
        let rtt = received_us - sent_us;
        let midpoint = sent_us as i128 + i128::from(rtt / 2);
        let offset = (remote_us as i128 - midpoint) as i64;
        if self.samples.len() == WINDOW {
            self.samples.remove(0);
        }
        self.samples.push((rtt, offset));
    }

    /// Quanto somar a um horário do outro relógio para trazê-lo ao nosso (µs).
    pub fn to_local_offset(&self) -> Option<i64> {
        self.samples
            .iter()
            .min_by_key(|(rtt, _)| *rtt)
            .map(|(_, offset)| -offset)
    }

    /// Menor ida e volta medida (µs).
    pub fn rtt_us(&self) -> Option<u64> {
        self.samples.iter().map(|(rtt, _)| *rtt).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acha_a_diferenca_de_relogio_com_rede_simetrica() {
        let mut c = ClockEstimator::new();
        // O outro relógio está 5 s à frente; ida e volta de 40 ms.
        c.record(1_000_000, 6_020_000, 1_040_000);
        assert_eq!(c.to_local_offset(), Some(-5_000_000));
        assert_eq!(c.rtt_us(), Some(40_000));
    }

    #[test]
    fn prefere_a_medicao_com_menor_ida_e_volta() {
        let mut c = ClockEstimator::new();
        c.record(0, 5_100_000, 300_000); // fila na rede: estimativa ruim
        c.record(1_000_000, 6_010_000, 1_020_000); // limpa
        assert_eq!(c.to_local_offset(), Some(-5_000_000));
    }

    #[test]
    fn ignora_medicao_impossivel_e_guarda_so_as_recentes() {
        let mut c = ClockEstimator::new();
        c.record(10, 0, 5);
        assert_eq!(c.to_local_offset(), None);
        for i in 0..40 {
            c.record(i * 1000, i * 1000 + 50, i * 1000 + 100 + i);
        }
        assert_eq!(c.samples.len(), WINDOW);
    }
}
