//! Transcrição da voz do modo reunião (ADR 0032): cada um transcreve o próprio microfone, no
//! próprio PC, com o whisper.cpp só na CPU.
//!
//! - [`SpeechSegmenter`]: corta as falas a partir do portão de voz do microfone, já descendo para
//!   16 kHz, sem alocar por pacote.
//! - [`Transcriber`]: o motor, atrás de uma trait para dar para trocar depois.
//! - [`WhisperTranscriber`]: o whisper.cpp, pela crate `whisper-rs`.

use std::fmt;
use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::voice::VOICE_FRAME;

/// Taxa do áudio que o Whisper recebe.
pub const TRANSCRIBE_RATE: usize = 16_000;
/// Do microfone (48 kHz) para o Whisper (16 kHz).
const DOWNSAMPLE: usize = 3;
/// Amostras de 16 kHz em cada pacote de 20 ms.
const FRAME_16K: usize = VOICE_FRAME / DOWNSAMPLE;
/// Pacotes de silêncio que terminam uma fala. O portão já fica aberto 300 ms depois da última
/// palavra (o "hangover" da voz): com mais 300 ms fechado, são os 600 ms de silêncio da ADR.
const END_SILENCE_FRAMES: u32 = 15;
/// Maior fala: aos 15 s, corta mesmo que a pessoa não pare.
pub const MAX_SPEECH_SAMPLES: usize = 15 * TRANSCRIBE_RATE;
/// Fala mais curta que isso (0,5 s) é estalo ou tosse: o Whisper inventaria texto.
const MIN_SPEECH_SAMPLES: usize = TRANSCRIBE_RATE / 2;

/// Uma fala pronta para transcrever.
#[derive(Debug)]
pub struct Speech {
    /// Quando a fala começou (ms desde 1970, UTC).
    pub at_ms: u64,
    /// Mono, 16 kHz.
    pub samples: Vec<f32>,
}

/// Corta as falas do microfone pelo portão de voz. Os buffers voltam por [`Self::recycle`]: depois
/// das primeiras falas, nada é alocado.
#[derive(Debug)]
pub struct SpeechSegmenter {
    current: Vec<f32>,
    at_ms: u64,
    silence: u32,
    spare: Vec<Vec<f32>>,
}

impl Default for SpeechSegmenter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechSegmenter {
    pub fn new() -> Self {
        Self {
            current: Vec::with_capacity(MAX_SPEECH_SAMPLES),
            at_ms: 0,
            silence: 0,
            spare: Vec::new(),
        }
    }

    /// 20 ms com voz (`pcm`: mono, 48 kHz, já sem ruído), às `now_ms`. Devolve a fala se ela
    /// chegou aos 15 s.
    pub fn voice(&mut self, pcm: &[f32], now_ms: u64) -> Option<Speech> {
        if self.current.is_empty() {
            self.at_ms = now_ms;
        }
        self.silence = 0;
        self.current.extend(
            pcm.chunks_exact(DOWNSAMPLE)
                .map(|c| c.iter().sum::<f32>() / DOWNSAMPLE as f32),
        );
        (self.current.len() + FRAME_16K > MAX_SPEECH_SAMPLES).then(|| self.take())?
    }

    /// 20 ms sem voz. Devolve a fala se o silêncio a terminou.
    pub fn silence(&mut self) -> Option<Speech> {
        if self.current.is_empty() {
            return None;
        }
        self.silence += 1;
        (self.silence >= END_SILENCE_FRAMES).then(|| self.take())?
    }

    /// Termina a fala em andamento (o microfone ficou mudo, ou a transcrição foi desligada).
    pub fn finish(&mut self) -> Option<Speech> {
        if self.current.is_empty() { None } else { self.take() }
    }

    /// Devolve o buffer de uma fala já transcrita, para a próxima.
    pub fn recycle(&mut self, mut buffer: Vec<f32>) {
        buffer.clear();
        if self.spare.len() < 4 {
            self.spare.push(buffer);
        }
    }

    fn take(&mut self) -> Option<Speech> {
        self.silence = 0;
        if self.current.len() < MIN_SPEECH_SAMPLES {
            self.current.clear();
            return None;
        }
        let next = self
            .spare
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(MAX_SPEECH_SAMPLES));
        Some(Speech {
            at_ms: self.at_ms,
            samples: std::mem::replace(&mut self.current, next),
        })
    }
}

/// Não deu para carregar o modelo ou transcrever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribeError(pub String);

impl fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TranscribeError {}

/// O motor que vira fala em texto.
pub trait Transcriber: Send {
    /// Transcreve uma fala (mono, 16 kHz). Texto vazio: não havia fala de verdade.
    fn transcribe(&mut self, samples: &[f32]) -> Result<String, TranscribeError>;
}

/// O whisper.cpp foi compilado para processadores com AVX2, FMA e F16C (todo processador de jogo
/// dos últimos 10 anos). Sem eles, o app não liga a transcrição em vez de travar.
pub fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("f16c")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Threads de cálculo do Whisper: no máximo 2, para sobrar processador para o jogo.
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(2) as i32)
}

/// O whisper.cpp na CPU.
pub struct WhisperTranscriber {
    state: WhisperState,
    language: Option<String>,
}

impl fmt::Debug for WhisperTranscriber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WhisperTranscriber")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl WhisperTranscriber {
    /// Carrega o modelo (`ggml-*.bin`). `language`: código do idioma (`"pt"`), ou `None` para
    /// detectar sozinho.
    pub fn load(model: &Path, language: Option<&str>) -> Result<Self, TranscribeError> {
        if !cpu_supported() {
            return Err(TranscribeError(
                "este processador não tem as instruções AVX2 que a transcrição usa".into(),
            ));
        }
        // Sem isso, o whisper.cpp escreve o carregamento inteiro no console.
        whisper_rs::install_logging_hooks();
        let mut params = WhisperContextParameters::default();
        // A placa de vídeo fica com o jogo e o encode (ADR 0032).
        params.use_gpu(false);
        let ctx = WhisperContext::new_with_params(model, params)
            .map_err(|e| TranscribeError(format!("não deu para abrir o modelo: {e}")))?;
        let state = ctx
            .create_state()
            .map_err(|e| TranscribeError(format!("não deu para preparar o modelo: {e}")))?;
        Ok(Self {
            state,
            language: language.map(str::to_owned),
        })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&mut self, samples: &[f32]) -> Result<String, TranscribeError> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(threads());
        params.set_language(Some(self.language.as_deref().unwrap_or("auto")));
        // Cada fala é independente: sem o texto anterior, um erro não se repete na próxima.
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_no_timestamps(true);
        params.set_audio_ctx(audio_ctx(samples.len()));
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        self.state
            .full(params, samples)
            .map_err(|e| TranscribeError(format!("a transcrição falhou: {e}")))?;
        let mut text = String::new();
        for segment in self.state.as_iter() {
            if let Ok(s) = segment.to_str_lossy() {
                text.push_str(&s);
            }
        }
        Ok(clean_text(&text))
    }
}

/// Posições do encoder do Whisper: 1500 para os 30 s da janela dele (50 por segundo).
const FULL_AUDIO_CTX: usize = 1500;
/// Folga além do tamanho da fala: abaixo disso o Whisper começa a errar o fim da frase.
const AUDIO_CTX_MARGIN: usize = 64;

/// Quanto da janela de 30 s o encoder processa. Sem isso, uma fala de 2 s custa o mesmo que uma
/// de 30 s; com isso, o custo acompanha o tamanho da fala.
fn audio_ctx(samples: usize) -> i32 {
    let ctx = samples.div_ceil(TRANSCRIBE_RATE / 50) + AUDIO_CTX_MARGIN;
    ctx.min(FULL_AUDIO_CTX) as i32
}

/// Frases que o Whisper inventa em silêncio ou ruído (vêm das legendas com que ele foi treinado).
const HALLUCINATIONS: [&str; 6] = [
    "amara.org",
    "legendas pela comunidade",
    "obrigado por assistir",
    "inscreva-se no canal",
    "legenda adriana zanotto",
    "subtítulos realizados",
];

/// Tira espaços, marcações entre colchetes (`[música]`) e as frases inventadas.
pub fn clean_text(text: &str) -> String {
    let text = text.trim();
    let lower = text.to_lowercase();
    if HALLUCINATIONS.iter().any(|h| lower.contains(h)) {
        return String::new();
    }
    let bracketed = (text.starts_with('[') && text.ends_with(']')) || (text.starts_with('(') && text.ends_with(')'));
    if bracketed || !text.chars().any(char::is_alphanumeric) {
        return String::new();
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000_000;

    fn voice(seg: &mut SpeechSegmenter, frames: usize) -> Option<Speech> {
        let pcm = [0.3f32; VOICE_FRAME];
        let mut out = None;
        for i in 0..frames {
            if let Some(s) = seg.voice(&pcm, NOW + i as u64 * 20) {
                out = Some(s);
            }
        }
        out
    }

    fn silence(seg: &mut SpeechSegmenter, frames: usize) -> Option<Speech> {
        (0..frames).find_map(|_| seg.silence())
    }

    #[test]
    fn fala_termina_depois_do_silencio_e_desce_para_16_khz() {
        let mut seg = SpeechSegmenter::new();
        assert!(voice(&mut seg, 50).is_none(), "1 s de fala ainda não terminou");
        assert!(silence(&mut seg, 14).is_none(), "280 ms de pausa não termina a fala");
        assert!(voice(&mut seg, 25).is_none(), "voltou a falar: a pausa zera");
        let speech = silence(&mut seg, 15).expect("300 ms fechado (mais o hangover) termina");
        assert_eq!(speech.at_ms, NOW, "a hora é a do começo da fala");
        assert_eq!(speech.samples.len(), 75 * FRAME_16K);
        assert!(speech.samples.iter().all(|s| (s - 0.3).abs() < 1e-6));
        assert!(silence(&mut seg, 100).is_none(), "sem fala, nada sai");
    }

    #[test]
    fn fala_longa_e_cortada_aos_15_segundos() {
        let mut seg = SpeechSegmenter::new();
        let first = voice(&mut seg, 750).expect("15 s cortam a fala");
        assert_eq!(first.samples.len(), MAX_SPEECH_SAMPLES);
        assert!(voice(&mut seg, 50).is_none());
        let rest = seg.finish().expect("o resto vira outra fala");
        assert_eq!(rest.samples.len(), 50 * FRAME_16K);
    }

    #[test]
    fn estalo_curto_nao_vira_fala() {
        let mut seg = SpeechSegmenter::new();
        voice(&mut seg, 10);
        assert!(silence(&mut seg, 15).is_none(), "200 ms de som é estalo");
        voice(&mut seg, 10);
        assert!(seg.finish().is_none());
    }

    #[test]
    fn buffers_devolvidos_sao_reaproveitados() {
        let mut seg = SpeechSegmenter::new();
        voice(&mut seg, 30);
        let speech = seg.finish().unwrap();
        let ptr = speech.samples.as_ptr();
        seg.recycle(speech.samples);
        voice(&mut seg, 30);
        seg.finish().unwrap();
        // O buffer devolvido virou o "atual" da fala seguinte: sem alocar de novo.
        voice(&mut seg, 1);
        assert_eq!(seg.current.as_ptr(), ptr);
    }

    #[test]
    fn texto_inventado_em_silencio_e_descartado() {
        assert_eq!(clean_text("  Bora pro mid agora.  "), "Bora pro mid agora.");
        assert_eq!(clean_text("Legendas pela comunidade Amara.org"), "");
        assert_eq!(clean_text("Obrigado por assistir!"), "");
        assert_eq!(clean_text("[Música]"), "");
        assert_eq!(clean_text("(risos)"), "");
        assert_eq!(clean_text(" ... "), "");
        assert_eq!(clean_text(""), "");
    }

    #[test]
    fn janela_do_encoder_acompanha_o_tamanho_da_fala() {
        // 2 s: 100 posições mais a folga; 15 s: 750 mais a folga; nunca passa das 1500 dos 30 s.
        assert_eq!(audio_ctx(2 * TRANSCRIBE_RATE), 164);
        assert_eq!(audio_ctx(MAX_SPEECH_SAMPLES), 814);
        assert_eq!(audio_ctx(40 * TRANSCRIBE_RATE), 1500);
    }

    /// Lê um WAV de 16 bits, mono, 16 kHz (o que o Whisper recebe).
    fn read_wav(path: &str) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("WAV");
        let data = bytes.windows(4).position(|w| w == b"data").expect("bloco data") + 8;
        bytes[data..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0)
            .collect()
    }

    /// Medição do modo reunião (ADR 0032): transcreve um WAV com o modelo de verdade e mostra o
    /// texto e o fator de tempo real (tempo de transcrição ÷ duração da fala). Rode em release:
    /// `SV_WHISPER_MODEL=<modelo.bin> SV_WHISPER_WAV=<fala.wav>
    /// cargo test -p sv-media --release --lib -- --ignored mede_a_transcricao --nocapture`
    #[test]
    #[ignore = "precisa do modelo do Whisper e de um WAV (SV_WHISPER_MODEL e SV_WHISPER_WAV)"]
    fn mede_a_transcricao() {
        let model = std::env::var("SV_WHISPER_MODEL").expect("SV_WHISPER_MODEL");
        let wav = read_wav(&std::env::var("SV_WHISPER_WAV").expect("SV_WHISPER_WAV"));
        let seconds = wav.len() as f32 / TRANSCRIBE_RATE as f32;
        let started = std::time::Instant::now();
        let mut whisper = WhisperTranscriber::load(Path::new(&model), Some("pt")).expect("modelo");
        let load = started.elapsed();
        // Em falas de até 15 s, como o corte faz na sala.
        let mut total = std::time::Duration::ZERO;
        for (i, speech) in wav.chunks(MAX_SPEECH_SAMPLES).enumerate() {
            let started = std::time::Instant::now();
            let text = whisper.transcribe(speech).expect("transcrição");
            let took = started.elapsed();
            total += took;
            let len = speech.len() as f32 / TRANSCRIBE_RATE as f32;
            println!("fala {i} ({len:.1} s) em {:.2} s: {text}", took.as_secs_f32());
        }
        println!(
            "modelo carregado em {:.2} s; {seconds:.1} s de fala em {:.2} s: fator de tempo real {:.2}",
            load.as_secs_f32(),
            total.as_secs_f32(),
            total.as_secs_f32() / seconds
        );
    }
}
