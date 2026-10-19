//! Decodificação por GPU (D3D11VA), renderização e áudio Opus.
//!
//! Fase 1: decode do stream recebido direto em textura D3D11, exibida sem passar pela CPU.

/// Taxa de amostragem do áudio transmitido (Opus trabalha nativamente em 48 kHz).
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_em_48khz() {
        assert_eq!(AUDIO_SAMPLE_RATE, 48_000);
    }
}
