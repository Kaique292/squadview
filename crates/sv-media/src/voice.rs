//! Voz do chat da sala (ADR 0010): microfone mono → supressão de ruído (RNNoise) → portão de
//! voz → Opus mono VOIP. Do outro lado, o [`VoiceDecoder`] devolve mono a 48 kHz.
//!
//! Tudo aqui é cálculo puro (sem dispositivo), para dar para testar e rodar na thread do
//! microfone. Os buffers são reaproveitados: nada de alocação a cada bloco.

use std::fmt;

use nnnoiseless::DenoiseState;
use opus_rs::{Application, OpusDecoder, OpusEncoder};

use crate::level::{AutoGain, soft_clip, volume_gain};
use crate::opus::{OpusError, SAMPLE_RATE, peak};

/// Amostras em cada pacote de voz (20 ms, mono).
pub const VOICE_FRAME: usize = SAMPLE_RATE as usize / 50;
/// Bitrate da voz: suficiente para fala clara, leve para mandar a 7 amigos.
pub const VOICE_BITRATE_BPS: i32 = 32_000;
/// Quadro do RNNoise (10 ms).
const DENOISE_FRAME: usize = DenoiseState::FRAME_SIZE;
/// Depois que a pessoa para de falar, o portão fica aberto mais um pouco para não cortar o fim
/// das palavras (em pacotes de 20 ms).
const HANGOVER_FRAMES: u32 = 15;
/// Probabilidade de voz (do RNNoise) que abre o portão.
const VAD_THRESHOLD: f32 = 0.5;
/// Sem supressão de ruído, abre o portão acima deste nível (RMS, 0 a 1). ~-46 dBFS.
const LEVEL_THRESHOLD: f32 = 0.005;
const MAX_PACKET: usize = 1275;

/// Opções do processamento do microfone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceOptions {
    /// Supressão de ruído (o "Isolamento de voz" do Discord).
    pub noise_suppression: bool,
    /// Volume do microfone, de 0 a 2 (1 = sem mudança), com a curva de [`volume_gain`].
    pub input_gain: f32,
    /// Controle automático de ganho: deixa a voz no mesmo volume para todos (como no Discord).
    pub auto_gain: bool,
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            noise_suppression: true,
            input_gain: 1.0,
            auto_gain: true,
        }
    }
}

/// Um pacote de voz pronto para mandar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceFrame<'a> {
    pub opus: &'a [u8],
    /// Nível (pico, 0 a 1), para o "está falando" dos outros.
    pub peak: f32,
}

/// Do microfone ao Opus.
pub struct VoiceEncoder {
    encoder: OpusEncoder,
    denoise: Box<DenoiseState<'static>>,
    options: VoiceOptions,
    /// Amostras do microfone esperando completar 10 ms.
    input: Vec<f32>,
    /// Um quadro do RNNoise (entrada e saída, na escala de i16 que ele espera).
    denoise_in: [f32; DENOISE_FRAME],
    denoise_out: [f32; DENOISE_FRAME],
    /// Voz processada esperando completar 20 ms, e a maior probabilidade de voz nesse intervalo.
    frame: Vec<f32>,
    frame_vad: f32,
    packet: Vec<u8>,
    /// Pacotes que o portão ainda fica aberto depois da última fala.
    hangover: u32,
    agc: AutoGain,
}

impl fmt::Debug for VoiceEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceEncoder")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl VoiceEncoder {
    pub fn new(options: VoiceOptions) -> Result<Self, OpusError> {
        let mut encoder = OpusEncoder::new(SAMPLE_RATE as i32, 1, Application::Voip).map_err(OpusError)?;
        encoder.bitrate_bps = VOICE_BITRATE_BPS;
        encoder.complexity = 8;
        // FEC: o pacote seguinte leva uma cópia barata do anterior, contra perdas na rede.
        encoder.use_inband_fec = true;
        encoder.packet_loss_perc = 10;
        Ok(Self {
            encoder,
            denoise: DenoiseState::new(),
            options,
            input: Vec::with_capacity(DENOISE_FRAME * 4),
            denoise_in: [0.0; DENOISE_FRAME],
            denoise_out: [0.0; DENOISE_FRAME],
            frame: Vec::with_capacity(VOICE_FRAME),
            frame_vad: 0.0,
            packet: vec![0; MAX_PACKET],
            hangover: 0,
            agc: AutoGain::new(),
        })
    }

    pub fn set_options(&mut self, options: VoiceOptions) {
        self.options = options;
    }

    /// Recebe um bloco do microfone (mono, 48 kHz, de qualquer tamanho). Para cada 20 ms com
    /// voz, chama `on_frame`. `on_level` recebe o nível de cada 20 ms (com ou sem voz), para o
    /// medidor da interface, e se o portão abriu.
    pub fn push(
        &mut self,
        samples: &[f32],
        mut on_level: impl FnMut(f32, bool),
        mut on_frame: impl FnMut(VoiceFrame<'_>),
    ) -> Result<(), OpusError> {
        self.input.extend_from_slice(samples);
        let mut start = 0;
        while self.input.len() - start >= DENOISE_FRAME {
            let chunk = &self.input[start..start + DENOISE_FRAME];
            let gain = volume_gain(self.options.input_gain);
            let vad = if self.options.noise_suppression {
                // O RNNoise trabalha na escala de i16.
                for (dst, s) in self.denoise_in.iter_mut().zip(chunk) {
                    *dst = (s * gain).clamp(-1.0, 1.0) * 32_768.0;
                }
                let vad = self.denoise.process_frame(&mut self.denoise_out, &self.denoise_in);
                self.frame.extend(self.denoise_out.iter().map(|s| s / 32_768.0));
                vad
            } else {
                self.frame.extend(chunk.iter().map(|s| (s * gain).clamp(-1.0, 1.0)));
                let tail = &self.frame[self.frame.len() - DENOISE_FRAME..];
                if rms(tail) >= LEVEL_THRESHOLD { 1.0 } else { 0.0 }
            };
            self.frame_vad = self.frame_vad.max(vad);
            start += DENOISE_FRAME;

            if self.frame.len() >= VOICE_FRAME {
                let level = peak(&self.frame);
                let open = if self.frame_vad >= VAD_THRESHOLD {
                    self.hangover = HANGOVER_FRAMES;
                    true
                } else if self.hangover > 0 {
                    self.hangover -= 1;
                    true
                } else {
                    false
                };
                on_level(level, open);
                if open {
                    // Nível da fala para todos: o AGC sobe o microfone baixo e o limitador segura
                    // o estourado (sem corte seco). Só aprende enquanto há fala de verdade.
                    let voiced = self.frame_vad >= VAD_THRESHOLD;
                    let frame = &mut self.frame[..VOICE_FRAME];
                    if self.options.auto_gain {
                        self.agc.process(frame, voiced);
                    } else {
                        frame.iter_mut().for_each(|s| *s = soft_clip(*s));
                    }
                    let level = peak(frame);
                    let n = self
                        .encoder
                        .encode(&self.frame[..VOICE_FRAME], VOICE_FRAME, &mut self.packet)
                        .map_err(OpusError)?;
                    on_frame(VoiceFrame {
                        opus: &self.packet[..n],
                        peak: level,
                    });
                }
                self.frame.clear();
                self.frame_vad = 0.0;
            }
        }
        self.input.drain(..start);
        Ok(())
    }
}

/// Opus mono → amostras mono a 48 kHz. Pacote perdido vira 20 ms disfarçados (PLC).
pub struct VoiceDecoder {
    decoder: OpusDecoder,
}

impl fmt::Debug for VoiceDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceDecoder").finish_non_exhaustive()
    }
}

impl VoiceDecoder {
    pub fn new() -> Result<Self, OpusError> {
        Ok(Self {
            decoder: OpusDecoder::new(SAMPLE_RATE as i32, 1).map_err(OpusError)?,
        })
    }

    /// Decodifica `packet` (ou disfarça a perda, se `None`) em `out`, com [`VOICE_FRAME`] posições.
    pub fn decode(&mut self, packet: Option<&[u8]>, out: &mut [f32]) -> Result<(), OpusError> {
        let n = self
            .decoder
            .decode(packet.unwrap_or(&[]), VOICE_FRAME, out)
            .map_err(OpusError)?;
        let len = out.len();
        out[n.min(len)..].fill(0.0);
        Ok(())
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

    fn tone(seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(SAMPLE_RATE as f32 * seconds) as usize)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    fn encode(options: VoiceOptions, input: &[f32], chunk: usize) -> (Vec<Vec<u8>>, Vec<bool>) {
        let mut enc = VoiceEncoder::new(options).unwrap();
        let mut packets = Vec::new();
        let mut gates = Vec::new();
        for c in input.chunks(chunk) {
            enc.push(c, |_, open| gates.push(open), |f| packets.push(f.opus.to_vec()))
                .unwrap();
        }
        (packets, gates)
    }

    const PLAIN: VoiceOptions = VoiceOptions {
        noise_suppression: false,
        input_gain: 1.0,
        auto_gain: false,
    };

    #[test]
    fn silencio_nao_manda_nada() {
        let (packets, gates) = encode(PLAIN, &vec![0.0; SAMPLE_RATE as usize], 441);
        assert!(packets.is_empty());
        // Mesmo calado, o medidor recebe o nível a cada 20 ms.
        assert_eq!(gates.len(), 50);
    }

    #[test]
    fn com_som_manda_um_pacote_a_cada_20_ms_de_qualquer_tamanho_de_bloco() {
        let (packets, _) = encode(PLAIN, &tone(1.0, 0.3), 441);
        assert_eq!(packets.len(), 50);
        assert!(packets.iter().all(|p| !p.is_empty() && p.len() < 400));
    }

    #[test]
    fn segura_o_portao_um_pouco_depois_de_parar_de_falar() {
        let mut input = tone(0.2, 0.3);
        input.extend(vec![0.0; SAMPLE_RATE as usize]);
        let (packets, _) = encode(PLAIN, &input, 480);
        // 10 pacotes de fala + 15 de "rabo" para não cortar o fim da palavra.
        assert_eq!(packets.len(), 10 + HANGOVER_FRAMES as usize);
    }

    #[test]
    fn ganho_zero_e_o_mesmo_que_ficar_calado() {
        let (packets, _) = encode(
            VoiceOptions {
                noise_suppression: false,
                input_gain: 0.0,
                auto_gain: true,
            },
            &tone(0.5, 0.3),
            480,
        );
        assert!(packets.is_empty());
    }

    #[test]
    fn supressao_de_ruido_corta_chiado_constante() {
        // Ruído branco fraco: o RNNoise não acha voz e o portão fica fechado (quase sempre).
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..SAMPLE_RATE as usize * 2)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.02
            })
            .collect();
        let (packets, _) = encode(VoiceOptions::default(), &noise, 480);
        assert!(packets.len() < 20, "{} pacotes de ruído", packets.len());
    }

    #[test]
    fn decodifica_o_que_codificou_e_disfarca_perda() {
        let (packets, _) = encode(PLAIN, &tone(0.5, 0.3), 480);
        let mut dec = VoiceDecoder::new().unwrap();
        let mut out = vec![0.0; VOICE_FRAME];
        for p in &packets {
            dec.decode(Some(p), &mut out).unwrap();
        }
        assert!(peak(&out) > 0.1);
        dec.decode(None, &mut out).unwrap();
        assert_eq!(out.len(), VOICE_FRAME);
    }
}
