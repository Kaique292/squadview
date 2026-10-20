//! Nível do áudio: a curva dos controles de volume, o limitador e o controle automático de ganho
//! do microfone. Tudo por amostra, sem alocação (roda nas threads de áudio).

use crate::playback::MAX_VOLUME;

/// Ganho de um controle de volume da interface (0 a 2, ou seja, 0% a 200%).
///
/// O ouvido percebe o volume em escala logarítmica: multiplicar o sinal por 2 (+6 dB) mal se nota.
/// Com a curva ao quadrado, 200% vira ×4 (+12 dB), 150% ×2,25 (+7 dB) e 50% ×0,25 (−12 dB), e a
/// barra soa como a do Discord: o meio é o volume normal e as pontas mudam de verdade.
pub fn volume_gain(volume: f32) -> f32 {
    let v = volume.clamp(0.0, MAX_VOLUME);
    v * v
}

/// Começo do limitador: abaixo disso a amostra passa intacta.
const KNEE: f32 = 0.9;

/// Limitador suave: abaixo de 0,9 não mexe; acima, aproxima de 1 sem passar, em vez de cortar
/// seco (que distorce quando alguém sobe o volume).
pub fn soft_clip(sample: f32) -> f32 {
    let magnitude = sample.abs();
    if magnitude <= KNEE {
        return sample;
    }
    let room = 1.0 - KNEE;
    let squeezed = KNEE + room * ((magnitude - KNEE) / room).tanh();
    squeezed.copysign(sample)
}

/// Nível alvo da fala (RMS): cerca de −20 dBFS, perto do que o Discord entrega.
const TARGET_RMS: f32 = 0.1;
/// Quanto o controle pode subir (microfone baixo) e descer (microfone estourado), em dB.
const MAX_GAIN_DB: f32 = 20.0;
const MIN_GAIN_DB: f32 = -10.0;
/// Velocidade a cada 20 ms: desce rápido (não estoura no grito) e sobe devagar (não "respira").
const ATTACK_DB: f32 = 3.0;
const RELEASE_DB: f32 = 0.4;
/// Abaixo disso é silêncio de verdade: não serve para medir a voz.
const SILENCE_RMS: f32 = 1e-4;

/// Controle automático de ganho do microfone (o "AGC" do Discord): deixa a voz de todo mundo
/// no mesmo volume, com microfone baixo ou alto. Só aprende com a fala (portão aberto), então não
/// sobe o ruído de fundo nos silêncios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoGain {
    gain_db: f32,
}

impl Default for AutoGain {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoGain {
    pub const fn new() -> Self {
        Self { gain_db: 0.0 }
    }

    /// Ganho atual, em dB.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Ajusta e aplica o ganho num quadro de 20 ms (com limitador). `voiced`: o quadro tem fala.
    pub fn process(&mut self, frame: &mut [f32], voiced: bool) {
        if voiced {
            let rms = rms(frame);
            if rms > SILENCE_RMS {
                let wanted = (20.0 * (TARGET_RMS / rms).log10()).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                let step = wanted - self.gain_db;
                self.gain_db += step.clamp(-ATTACK_DB, RELEASE_DB);
            }
        }
        let gain = 10f32.powf(self.gain_db / 20.0);
        for s in frame.iter_mut() {
            *s = soft_clip(*s * gain);
        }
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amplitude * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn curva_do_volume_e_perceptiva() {
        assert_eq!(volume_gain(1.0), 1.0);
        assert_eq!(volume_gain(2.0), 4.0);
        assert_eq!(volume_gain(0.5), 0.25);
        assert_eq!(volume_gain(0.0), 0.0);
        // Fora da faixa: preso entre 0% e 200%.
        assert_eq!(volume_gain(5.0), 4.0);
        assert_eq!(volume_gain(-1.0), 0.0);
    }

    #[test]
    fn limitador_nao_mexe_no_normal_e_nunca_passa_de_1() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert_eq!(soft_clip(-0.9), -0.9);
        for x in [0.95, 1.0, 2.0, 10.0, -3.0] {
            let y = soft_clip(x);
            assert!(y.abs() <= 1.0 && y.abs() > KNEE, "{x} → {y}");
            assert_eq!(y.signum(), x.signum());
        }
        // Continua subindo (sem degrau): mais alto entra, mais alto sai.
        assert!(soft_clip(0.95) > soft_clip(0.91));
    }

    #[test]
    fn microfone_baixo_sobe_ate_o_nivel_da_fala() {
        let mut agc = AutoGain::new();
        // Fala a −40 dBFS (headset baixo): em ~1 s o ganho chega perto do máximo de +20 dB.
        for _ in 0..50 {
            let mut frame = tone(0.014, 960);
            agc.process(&mut frame, true);
        }
        assert!(agc.gain_db() > 15.0, "{}", agc.gain_db());
        let mut frame = tone(0.014, 960);
        agc.process(&mut frame, true);
        assert!(rms(&frame) > 0.07, "{}", rms(&frame));
    }

    #[test]
    fn microfone_estourado_desce_rapido() {
        let mut agc = AutoGain::new();
        for _ in 0..5 {
            let mut frame = tone(0.9, 960);
            agc.process(&mut frame, true);
        }
        assert!(agc.gain_db() < -5.0, "{}", agc.gain_db());
    }

    #[test]
    fn silencio_e_ruido_sem_fala_nao_mudam_o_ganho() {
        let mut agc = AutoGain::new();
        for _ in 0..100 {
            let mut frame = tone(0.003, 960);
            agc.process(&mut frame, false);
        }
        assert_eq!(agc.gain_db(), 0.0);
        let mut zero = vec![0.0; 960];
        agc.process(&mut zero, true);
        assert_eq!(agc.gain_db(), 0.0);
    }
}
