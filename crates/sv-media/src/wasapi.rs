//! Partes puras do WASAPI usadas pelo microfone e pela reprodução (testáveis sem dispositivo).

use windows::Win32::Media::Audio::WAVEFORMATEX;

/// `WAVE_FORMAT_IEEE_FLOAT`: amostras f32.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// Formato f32 intercalado com `channels` canais a `sample_rate` Hz. O Windows converte do
/// formato do dispositivo para este (`AUTOCONVERTPCM`).
pub(crate) fn float_format(channels: u16, sample_rate: u32) -> WAVEFORMATEX {
    let block_align = channels * 4;
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: sample_rate * u32::from(block_align),
        nBlockAlign: block_align,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

/// Duração do buffer do WASAPI em unidades de 100 ns.
pub(crate) const fn buffer_duration(ms: i64) -> i64 {
    ms * 10_000
}

/// Copia um pacote do WASAPI para `buffer` (reaproveitado: sem alocar depois que a capacidade
/// assenta). `None` é um pacote marcado como silêncio: vira `len` zeros.
pub(crate) fn copy_packet(buffer: &mut Vec<f32>, data: Option<&[f32]>, len: usize) {
    buffer.clear();
    match data {
        Some(samples) => buffer.extend_from_slice(samples),
        None => buffer.resize(len, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_float_estereo_e_mono_a_48_khz() {
        // WAVEFORMATEX é packed: copia os campos antes de comparar.
        let fields = |f: WAVEFORMATEX| {
            let WAVEFORMATEX {
                wFormatTag,
                nChannels,
                nSamplesPerSec,
                nAvgBytesPerSec,
                nBlockAlign,
                wBitsPerSample,
                cbSize,
            } = f;
            (
                wFormatTag,
                nChannels,
                nSamplesPerSec,
                nAvgBytesPerSec,
                nBlockAlign,
                wBitsPerSample,
                cbSize,
            )
        };
        assert_eq!(
            fields(float_format(2, 48_000)),
            (WAVE_FORMAT_IEEE_FLOAT, 2, 48_000, 48_000 * 8, 8, 32, 0)
        );
        assert_eq!(
            fields(float_format(1, 48_000)),
            (WAVE_FORMAT_IEEE_FLOAT, 1, 48_000, 192_000, 4, 32, 0)
        );
    }

    #[test]
    fn duracao_do_buffer_em_100_ns() {
        assert_eq!(buffer_duration(20), 200_000);
        assert_eq!(buffer_duration(30), 300_000);
    }

    #[test]
    fn pacote_com_som_e_copiado_e_silencio_vira_zeros() {
        let mut buffer = Vec::with_capacity(16);
        copy_packet(&mut buffer, Some(&[0.5, -0.5, 0.25]), 3);
        assert_eq!(buffer, [0.5, -0.5, 0.25]);
        copy_packet(&mut buffer, None, 4);
        assert_eq!(buffer, [0.0; 4], "o pacote anterior não pode vazar");
        let capacity = buffer.capacity();
        copy_packet(&mut buffer, Some(&[1.0; 8]), 8);
        assert_eq!(buffer.capacity(), capacity, "reaproveita o buffer");
    }
}
