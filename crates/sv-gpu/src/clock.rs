//! Relógio do pipeline de mídia, na mesma base do `SystemRelativeTime` da captura (QPC).

use std::sync::OnceLock;

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// Agora, em microssegundos do relógio de alta precisão do Windows.
/// É a mesma base dos timestamps de [`crate::GpuFrame`] vindos da captura.
pub fn now_us() -> u64 {
    static FREQ: OnceLock<i64> = OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: ponteiro de saída válido; a função nunca falha no Windows XP ou mais novo.
        let _ = unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1)
    });
    let mut counter = 0i64;
    // SAFETY: ponteiro de saída válido.
    let _ = unsafe { QueryPerformanceCounter(&mut counter) };
    (counter as i128 * 1_000_000 / freq as i128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relogio_anda_para_frente() {
        let a = now_us();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = now_us();
        assert!(b >= a + 4_000, "andou só {} us", b - a);
    }
}
