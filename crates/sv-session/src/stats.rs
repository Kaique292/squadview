//! Estatísticas do stream (fps, bitrate, tempo de encode, latência), calculadas por janela de tempo.

use serde::Serialize;

/// O que a interface mostra, atualizado a cada [`StatsMeter::WINDOW_US`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStats {
    /// Quadros que a fonte (o jogo) entregou por segundo, antes do limite de fps.
    /// Para no refresh do monitor: abaixo dele, o jogo está rodando abaixo do refresh.
    pub source_fps: f32,
    /// Quadros codificados por segundo.
    pub fps: f32,
    pub bitrate_kbps: u32,
    /// Média do tempo de encode, em ms.
    pub encode_ms: f32,
    /// Média da latência da captura até a tela, em ms (no preview local, é exata; na rede, estimada).
    pub latency_ms: f32,
    /// Pior latência da janela (p95), em ms.
    pub latency_p95_ms: f32,
    /// Frames descartados porque o encoder ainda estava ocupado.
    pub dropped: u32,
    pub width: u32,
    pub height: u32,
    /// Ex.: "NVIDIA H.264 Encoder MFT".
    pub encoder: String,
    /// O áudio do jogo está sendo capturado.
    pub audio_active: bool,
    /// Por que o áudio não está sendo capturado, se der erro.
    pub audio_error: Option<String>,
    pub audio_kbps: u32,
    /// Maior pico do áudio na janela (0 a 1), para o medidor.
    pub audio_level: f32,
    /// Pacotes de áudio que não chegaram a tempo (disfarçados).
    pub audio_lost: u32,
    /// Ida e volta até quem transmite, em ms (só para quem assiste; 0 na prévia).
    pub rtt_ms: f32,
    /// Altura da camada baixa do simulcast, quando ela está ligada para alguém (0: desligada).
    pub low_layer_height: u32,
    /// Por que a camada baixa não abriu, se der erro (quem assiste fica na principal).
    pub low_layer_error: Option<String>,
}

/// Acumula medições e fecha uma [`StreamStats`] por janela de tempo.
#[derive(Debug, Default)]
pub struct StatsMeter {
    window_start_us: Option<u64>,
    source_frames: u32,
    frames: u32,
    bytes: u64,
    encode_us_sum: u64,
    encode_samples: u32,
    latencies_us: Vec<u32>,
    dropped: u32,
    audio_bytes: u64,
    audio_level: f32,
    audio_lost: u32,
}

impl StatsMeter {
    /// Duração de cada janela: 500 ms.
    pub const WINDOW_US: u64 = 500_000;

    pub fn new() -> Self {
        Self::default()
    }

    /// A fonte entregou um quadro novo (antes do limite de fps).
    pub fn record_source_frame(&mut self) {
        self.source_frames += 1;
    }

    /// Um frame codificado saiu do encoder.
    pub fn record_encoded(&mut self, bytes: usize, encode_us: u32) {
        self.frames += 1;
        self.bytes += bytes as u64;
        if encode_us > 0 {
            self.encode_us_sum += u64::from(encode_us);
            self.encode_samples += 1;
        }
    }

    /// Um frame apareceu na tela, `latency_us` depois de capturado.
    pub fn record_presented(&mut self, latency_us: u32) {
        if self.latencies_us.len() < 1024 {
            self.latencies_us.push(latency_us);
        }
    }

    /// Um pacote de áudio foi codificado.
    pub fn record_audio(&mut self, bytes: usize, peak: f32) {
        self.audio_bytes += bytes as u64;
        self.audio_level = self.audio_level.max(peak);
    }

    /// Um pacote de áudio se perdeu.
    pub fn record_audio_lost(&mut self) {
        self.audio_lost += 1;
    }

    /// Um frame foi descartado antes do encoder.
    pub fn record_dropped(&mut self) {
        self.dropped += 1;
    }

    /// Fecha a janela se ela já durou [`Self::WINDOW_US`]. `base` traz os campos fixos
    /// (tamanho, nome do encoder), que são copiados para o resultado.
    pub fn poll(&mut self, now_us: u64, base: &StreamStats) -> Option<StreamStats> {
        let start = *self.window_start_us.get_or_insert(now_us);
        let elapsed = now_us.saturating_sub(start);
        if elapsed < Self::WINDOW_US {
            return None;
        }
        let secs = elapsed as f64 / 1e6;
        self.latencies_us.sort_unstable();
        let latency_avg = if self.latencies_us.is_empty() {
            0.0
        } else {
            self.latencies_us.iter().map(|&l| f64::from(l)).sum::<f64>() / self.latencies_us.len() as f64
        };
        let p95 = self
            .latencies_us
            .get((self.latencies_us.len() * 95 / 100).min(self.latencies_us.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);

        let stats = StreamStats {
            source_fps: (f64::from(self.source_frames) / secs) as f32,
            fps: (f64::from(self.frames) / secs) as f32,
            bitrate_kbps: (self.bytes as f64 * 8.0 / 1000.0 / secs) as u32,
            encode_ms: if self.encode_samples == 0 {
                0.0
            } else {
                (self.encode_us_sum as f64 / f64::from(self.encode_samples) / 1000.0) as f32
            },
            latency_ms: (latency_avg / 1000.0) as f32,
            latency_p95_ms: p95 as f32 / 1000.0,
            dropped: self.dropped,
            audio_kbps: (self.audio_bytes as f64 * 8.0 / 1000.0 / secs) as u32,
            audio_level: self.audio_level,
            audio_lost: self.audio_lost,
            ..base.clone()
        };
        *self = Self {
            window_start_us: Some(now_us),
            ..Self::default()
        };
        Some(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> StreamStats {
        StreamStats {
            width: 1920,
            height: 1080,
            encoder: "Teste".into(),
            ..Default::default()
        }
    }

    #[test]
    fn nao_fecha_antes_de_meio_segundo() {
        let mut m = StatsMeter::new();
        assert_eq!(m.poll(0, &base()), None);
        m.record_encoded(1000, 2000);
        assert_eq!(m.poll(400_000, &base()), None);
    }

    #[test]
    fn calcula_fps_bitrate_encode_e_latencia() {
        let mut m = StatsMeter::new();
        m.poll(0, &base());
        // 30 frames de 20 KB em 0,5 s = 60 fps e 9,6 Mbps.
        // Dois frames lentos em 30 (6,7%) aparecem no p95; a média fica em (28·20 + 2·50) / 30 = 22 ms.
        for i in 0..30 {
            m.record_encoded(20_000, 2_000);
            m.record_presented(if i >= 28 { 50_000 } else { 20_000 });
        }
        for _ in 0..37 {
            m.record_source_frame();
        }
        m.record_dropped();
        let s = m.poll(500_000, &base()).unwrap();
        assert_eq!(s.source_fps, 74.0);
        assert_eq!(s.fps, 60.0);
        assert_eq!(s.bitrate_kbps, 9_600);
        assert_eq!(s.encode_ms, 2.0);
        assert!((s.latency_ms - 22.0).abs() < 0.01);
        assert_eq!(s.latency_p95_ms, 50.0);
        assert_eq!(s.dropped, 1);
        assert_eq!((s.width, s.height, s.encoder.as_str()), (1920, 1080, "Teste"));
    }

    #[test]
    fn mede_bitrate_nivel_e_perdas_do_audio() {
        let mut m = StatsMeter::new();
        m.poll(0, &base());
        // 25 pacotes de 400 bytes em 0,5 s = 160 kbps.
        for i in 0..25 {
            m.record_audio(400, if i == 3 { 0.8 } else { 0.2 });
        }
        m.record_audio_lost();
        let s = m
            .poll(
                500_000,
                &StreamStats {
                    audio_active: true,
                    ..base()
                },
            )
            .unwrap();
        assert_eq!(
            (s.audio_kbps, s.audio_level, s.audio_lost, s.audio_active),
            (160, 0.8, 1, true)
        );
    }

    #[test]
    fn recomeca_do_zero_a_cada_janela() {
        let mut m = StatsMeter::new();
        m.poll(0, &base());
        m.record_encoded(1000, 1000);
        m.poll(500_000, &base()).unwrap();
        let s = m.poll(1_000_000, &base()).unwrap();
        assert_eq!((s.fps, s.bitrate_kbps, s.dropped), (0.0, 0, 0));
    }

    #[test]
    fn sem_medicoes_de_tempo_fica_em_zero() {
        let mut m = StatsMeter::new();
        m.poll(0, &base());
        m.record_encoded(1000, 0);
        let s = m.poll(500_000, &base()).unwrap();
        assert_eq!((s.encode_ms, s.latency_ms, s.latency_p95_ms), (0.0, 0.0, 0.0));
    }
}
