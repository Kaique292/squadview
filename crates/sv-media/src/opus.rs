//! Áudio Opus: 48 kHz estéreo, pacotes de 20 ms (implementação em Rust puro, `opus-rs`).

use std::fmt;

use opus_rs::{Application, OpusDecoder, OpusEncoder};

/// Taxa de amostragem do áudio transmitido.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Amostras por canal em cada pacote (20 ms).
pub const FRAME_SAMPLES: usize = SAMPLE_RATE as usize / 50;
/// Bitrate do áudio do jogo: estéreo com folga para música e efeitos.
pub const BITRATE_BPS: i32 = 128_000;
/// Maior pacote Opus possível.
const MAX_PACKET: usize = 1275;

/// Erro do codec (texto vindo do `opus-rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusError(pub &'static str);

impl fmt::Display for OpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "erro no Opus: {}", self.0)
    }
}

impl std::error::Error for OpusError {}

/// Junta amostras soltas (a captura entrega blocos de tamanhos variados) em pacotes de 20 ms.
pub struct AudioEncoder {
    encoder: OpusEncoder,
    pending: Vec<f32>,
    packet: Vec<u8>,
}

impl fmt::Debug for AudioEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioEncoder")
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl AudioEncoder {
    pub fn new() -> Result<Self, OpusError> {
        let mut encoder = OpusEncoder::new(SAMPLE_RATE as i32, CHANNELS, Application::Audio).map_err(OpusError)?;
        encoder.bitrate_bps = BITRATE_BPS;
        encoder.complexity = 8;
        Ok(Self {
            encoder,
            pending: Vec::with_capacity(FRAME_SAMPLES * CHANNELS * 4),
            packet: vec![0; MAX_PACKET],
        })
    }

    /// Recebe amostras intercaladas (L, R, L, R…) e entrega cada pacote de 20 ms pronto.
    pub fn push(&mut self, samples: &[f32], mut on_packet: impl FnMut(&[u8])) -> Result<(), OpusError> {
        self.pending.extend_from_slice(samples);
        let frame = FRAME_SAMPLES * CHANNELS;
        let mut start = 0;
        while self.pending.len() - start >= frame {
            let n = self
                .encoder
                .encode(&self.pending[start..start + frame], FRAME_SAMPLES, &mut self.packet)
                .map_err(OpusError)?;
            on_packet(&self.packet[..n]);
            start += frame;
        }
        self.pending.drain(..start);
        Ok(())
    }
}

/// Decodifica pacotes de 20 ms; um pacote perdido vira 20 ms de áudio disfarçado (PLC).
pub struct AudioDecoder {
    decoder: OpusDecoder,
}

impl fmt::Debug for AudioDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioDecoder").finish_non_exhaustive()
    }
}

impl AudioDecoder {
    pub fn new() -> Result<Self, OpusError> {
        Ok(Self {
            decoder: OpusDecoder::new(SAMPLE_RATE as i32, CHANNELS).map_err(OpusError)?,
        })
    }

    /// Decodifica `packet` (ou disfarça a perda, se `None`) em `out`, que precisa ter
    /// `FRAME_SAMPLES * CHANNELS` posições.
    pub fn decode(&mut self, packet: Option<&[u8]>, out: &mut [f32]) -> Result<(), OpusError> {
        let n = self
            .decoder
            .decode(packet.unwrap_or(&[]), FRAME_SAMPLES, out)
            .map_err(OpusError)?;
        // Se o decoder devolver menos que um pacote inteiro, completa com silêncio.
        out[n * CHANNELS..].fill(0.0);
        Ok(())
    }
}

/// Nível do áudio (pico, 0 a 1), para o medidor da interface.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs())).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 s de um tom de 440 Hz (esquerda) e 660 Hz (direita).
    fn tone() -> Vec<f32> {
        (0..SAMPLE_RATE as usize)
            .flat_map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                [
                    0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin(),
                    0.4 * (2.0 * std::f32::consts::PI * 660.0 * t).sin(),
                ]
            })
            .collect()
    }

    fn roundtrip(input: &[f32], chunk: usize) -> (Vec<Vec<u8>>, Vec<f32>) {
        let mut enc = AudioEncoder::new().unwrap();
        let mut packets = Vec::new();
        for c in input.chunks(chunk) {
            enc.push(c, |p| packets.push(p.to_vec())).unwrap();
        }
        let mut dec = AudioDecoder::new().unwrap();
        let mut out = Vec::new();
        let mut buf = vec![0.0; FRAME_SAMPLES * CHANNELS];
        for p in &packets {
            dec.decode(Some(p), &mut buf).unwrap();
            out.extend_from_slice(&buf);
        }
        (packets, out)
    }

    #[test]
    fn junta_blocos_de_qualquer_tamanho_em_pacotes_de_20ms() {
        // 1 s em blocos de 441 amostras (tamanho comum do WASAPI): 50 pacotes, sobra nada.
        let (packets, out) = roundtrip(&tone(), 441 * 2);
        assert_eq!(packets.len(), 50);
        assert_eq!(out.len(), SAMPLE_RATE as usize * CHANNELS);
    }

    #[test]
    fn bitrate_fica_perto_do_alvo() {
        let (packets, _) = roundtrip(&tone(), 960);
        let bits: usize = packets.iter().map(|p| p.len() * 8).sum();
        assert!((40_000..=180_000).contains(&bits), "{bits} bits em 1 s");
    }

    #[test]
    fn o_som_volta_reconhecivel() {
        let input = tone();
        let (_, out) = roundtrip(&input, 960);
        // O codec atrasa alguns ms: compara a energia (RMS) da segunda metade, já estável.
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let half = input.len() / 2;
        let (a, b) = (rms(&input[half..]), rms(&out[half..]));
        assert!((b / a - 1.0).abs() < 0.2, "energia original {a}, decodificada {b}");
    }

    #[test]
    fn pacote_perdido_vira_audio_e_nao_erro() {
        let mut dec = AudioDecoder::new().unwrap();
        let mut buf = vec![1.0; FRAME_SAMPLES * CHANNELS];
        dec.decode(None, &mut buf).unwrap();
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn pico_do_medidor() {
        assert_eq!(peak(&[0.1, -0.7, 0.3]), 0.7);
        assert_eq!(peak(&[2.0]), 1.0);
        assert_eq!(peak(&[]), 0.0);
    }
}
