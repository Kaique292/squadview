//! Uso da GPU e da CPU, lido dos contadores de desempenho do Windows (os mesmos do Gerenciador de Tarefas).

use serde::Serialize;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

/// Uma leitura de uso do sistema, em porcentagem (0–100).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLoad {
    /// Motor 3D da GPU (o que o jogo usa).
    pub gpu_3d: f32,
    /// Encoder de vídeo da GPU (o que o SquadView usa para transmitir).
    pub gpu_encode: f32,
    /// Decoder de vídeo da GPU (o que o SquadView usa para assistir).
    pub gpu_decode: f32,
    pub cpu: f32,
}

/// Lê o uso do sistema. A primeira leitura sai zerada (os contadores precisam de duas amostras).
pub struct SystemMonitor {
    query: PDH_HQUERY,
    gpu_3d: Option<PDH_HCOUNTER>,
    gpu_encode: Option<PDH_HCOUNTER>,
    gpu_decode: Option<PDH_HCOUNTER>,
    cpu: Option<PDH_HCOUNTER>,
}

impl std::fmt::Debug for SystemMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemMonitor").finish_non_exhaustive()
    }
}

// SAFETY: os handles do PDH são usados só por quem possui o SystemMonitor (via &mut self).
unsafe impl Send for SystemMonitor {}

impl SystemMonitor {
    /// Abre os contadores. Algum que não exista nesta máquina fica de fora (lido como 0).
    pub fn new() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: ponteiro de saída válido; consulta local (sem fonte de log).
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let add = |path: PCWSTR| {
            let mut counter = PDH_HCOUNTER::default();
            // SAFETY: consulta aberta acima; caminho é uma string estática.
            (unsafe { PdhAddEnglishCounterW(query, path, 0, &mut counter) } == 0).then_some(counter)
        };
        let monitor = Self {
            query,
            gpu_3d: add(w!(r"\GPU Engine(*engtype_3D)\Utilization Percentage")),
            gpu_encode: add(w!(r"\GPU Engine(*engtype_VideoEncode)\Utilization Percentage")),
            gpu_decode: add(w!(r"\GPU Engine(*engtype_VideoDecode)\Utilization Percentage")),
            cpu: add(w!(r"\Processor(_Total)\% Processor Time")),
        };
        // SAFETY: primeira coleta para os contadores de taxa terem base.
        unsafe { PdhCollectQueryData(query) };
        Some(monitor)
    }

    /// Lê o uso desde a leitura anterior.
    pub fn sample(&mut self) -> SystemLoad {
        // SAFETY: consulta válida enquanto self existir.
        unsafe { PdhCollectQueryData(self.query) };
        SystemLoad {
            gpu_3d: self.gpu_3d.map_or(0.0, read_sum),
            gpu_encode: self.gpu_encode.map_or(0.0, read_sum),
            gpu_decode: self.gpu_decode.map_or(0.0, read_sum),
            cpu: self.cpu.map_or(0.0, read_single),
        }
    }
}

impl Drop for SystemMonitor {
    fn drop(&mut self) {
        // SAFETY: fecha a consulta aberta em new().
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Os contadores de GPU têm uma instância por processo e motor; o uso total é a soma.
fn read_sum(counter: PDH_HCOUNTER) -> f32 {
    let mut size = 0u32;
    let mut count = 0u32;
    // SAFETY: primeira chamada só pergunta o tamanho; a segunda usa um buffer desse tamanho,
    // alinhado para os itens (os nomes ficam no fim do mesmo buffer).
    unsafe {
        if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
            return 0.0;
        }
        let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut buffer: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity((size as usize).div_ceil(item));
        let items = buffer.as_mut_ptr();
        if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
            return 0.0;
        }
        let values = (0..count as usize).map(|i| {
            let v = &(*items.add(i)).FmtValue;
            if v.CStatus == 0 { v.Anonymous.doubleValue } else { 0.0 }
        });
        sum_capped(values)
    }
}

fn read_single(counter: PDH_HCOUNTER) -> f32 {
    let mut value = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: contador válido; ponteiro de saída vivo.
    unsafe {
        if PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut value) != 0 || value.CStatus != 0 {
            return 0.0;
        }
        value.Anonymous.doubleValue.clamp(0.0, 100.0) as f32
    }
}

/// Soma o uso de vários processos, limitado a 100%.
pub(crate) fn sum_capped(values: impl Iterator<Item = f64>) -> f32 {
    // `+ 0.0`: a soma vazia de f64 dá -0.0, que apareceria como "-0" no registro.
    (values.filter(|v| v.is_finite() && *v > 0.0).sum::<f64>() + 0.0).min(100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma_os_processos_sem_passar_de_100() {
        assert_eq!(sum_capped([30.0, 25.5].into_iter()), 55.5);
        assert_eq!(sum_capped([80.0, 70.0].into_iter()), 100.0);
    }

    #[test]
    fn ignora_valores_invalidos() {
        assert_eq!(sum_capped([f64::NAN, -5.0, 10.0].into_iter()), 10.0);
        assert!(sum_capped(std::iter::empty()).is_sign_positive());
    }

    #[test]
    #[ignore = "precisa dos contadores de desempenho do Windows"]
    fn le_o_uso_do_sistema() {
        let mut m = SystemMonitor::new().expect("contadores");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let load = m.sample();
        eprintln!("{load:?}");
        assert!((0.0..=100.0).contains(&load.cpu));
    }
}
