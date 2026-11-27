//! A fila do modo reunião (ADR 0032): as falas cortadas na thread do microfone esperam aqui, e uma
//! thread de prioridade baixa as transcreve, uma por vez. O microfone nunca espera o Whisper.
//!
//! A fila guarda no máximo 30 s de áudio. Se o PC não der conta, a fala mais antiga ainda não
//! transcrita vira "[trecho não transcrito]" em vez de a fila crescer sem fim.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use sv_media::transcribe::{Speech, TRANSCRIBE_RATE, Transcriber};

use crate::surface::lock;

/// Áudio máximo esperando a transcrição.
const MAX_QUEUED_SAMPLES: usize = 30 * TRANSCRIBE_RATE;
/// Buffers de fala guardados para o microfone reaproveitar.
const MAX_FREE_BUFFERS: usize = 4;

/// Uma linha pronta da nossa transcrição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptLine {
    /// O que falamos, a partir de `at_ms` (ms desde 1970, UTC).
    Text { at_ms: u64, text: String },
    /// A fala que começou em `at_ms` ficou de fora: o PC não deu conta a tempo.
    Skipped { at_ms: u64 },
    /// O Whisper falhou nessa fala.
    Failed { at_ms: u64, error: String },
}

enum Job {
    Speech(Speech),
    Skipped { at_ms: u64 },
}

#[derive(Default)]
struct State {
    jobs: VecDeque<Job>,
    queued: usize,
    /// Buffers já transcritos, de volta para o microfone.
    free: Vec<Vec<f32>>,
    stop: bool,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    wake: Condvar,
}

/// A fila e a thread que transcreve. Ao ser descartada, a thread ainda transcreve o que já estava
/// na fila e depois para, sem que quem descartou espere por ela.
pub(crate) struct TranscribeQueue {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for TranscribeQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscribeQueue").finish_non_exhaustive()
    }
}

impl TranscribeQueue {
    /// Liga a thread com o `transcriber`. Cada linha pronta vai para `on_line`.
    pub(crate) fn start(
        transcriber: Box<dyn Transcriber>,
        on_line: impl Fn(TranscriptLine) + Send + 'static,
    ) -> Result<Self, String> {
        let inner = Arc::new(Inner::default());
        let worker = inner.clone();
        std::thread::Builder::new()
            .name("sv-transcribe".into())
            .spawn(move || {
                lower_priority();
                run(&worker, transcriber, on_line);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    /// Uma fala nova. Se passar dos 30 s na fila, as mais antigas viram trechos não transcritos.
    pub(crate) fn push(&self, speech: Speech) {
        let mut state = lock(&self.inner.state);
        state.queued += speech.samples.len();
        state.jobs.push_back(Job::Speech(speech));
        let State { jobs, queued, free, .. } = &mut *state;
        for job in jobs.iter_mut() {
            if *queued <= MAX_QUEUED_SAMPLES {
                break;
            }
            if let Job::Speech(old) = job {
                let at_ms = old.at_ms;
                *queued -= old.samples.len();
                let Job::Speech(old) = std::mem::replace(job, Job::Skipped { at_ms }) else {
                    unreachable!()
                };
                keep_buffer(free, old.samples);
            }
        }
        drop(state);
        self.inner.wake.notify_one();
    }

    /// Um buffer já transcrito, se houver, sem esperar (a thread do microfone não pode travar).
    pub(crate) fn try_take_buffer(&self) -> Option<Vec<f32>> {
        self.inner.state.try_lock().ok()?.free.pop()
    }
}

impl Drop for TranscribeQueue {
    fn drop(&mut self) {
        lock(&self.inner.state).stop = true;
        self.inner.wake.notify_one();
    }
}

fn keep_buffer(free: &mut Vec<Vec<f32>>, mut buffer: Vec<f32>) {
    if free.len() < MAX_FREE_BUFFERS {
        buffer.clear();
        free.push(buffer);
    }
}

fn run(inner: &Inner, mut transcriber: Box<dyn Transcriber>, on_line: impl Fn(TranscriptLine)) {
    loop {
        let job = {
            let mut state = lock(&inner.state);
            loop {
                if let Some(job) = state.jobs.pop_front() {
                    if let Job::Speech(speech) = &job {
                        state.queued -= speech.samples.len();
                    }
                    break job;
                }
                if state.stop {
                    return;
                }
                state = inner.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        };
        match job {
            Job::Skipped { at_ms } => on_line(TranscriptLine::Skipped { at_ms }),
            Job::Speech(speech) => {
                let at_ms = speech.at_ms;
                match transcriber.transcribe(&speech.samples) {
                    Ok(text) if text.is_empty() => {}
                    Ok(text) => on_line(TranscriptLine::Text { at_ms, text }),
                    Err(e) => on_line(TranscriptLine::Failed {
                        at_ms,
                        error: e.to_string(),
                    }),
                }
                keep_buffer(&mut lock(&inner.state).free, speech.samples);
            }
        }
    }
}

/// A transcrição disputa a CPU com o jogo: o jogo ganha.
fn lower_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: muda só a prioridade da própria thread.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use sv_media::transcribe::TranscribeError;

    use super::*;

    /// Um Whisper de mentira: devolve quantos segundos ouviu, e pode ficar preso até ser solto.
    struct Fake {
        gate: Option<mpsc::Receiver<()>>,
    }

    impl Transcriber for Fake {
        fn transcribe(&mut self, samples: &[f32]) -> Result<String, TranscribeError> {
            if let Some(gate) = &self.gate {
                let _ = gate.recv_timeout(Duration::from_secs(5));
            }
            if samples.first() == Some(&-1.0) {
                return Err(TranscribeError("modelo quebrado".into()));
            }
            if samples.first() == Some(&0.5) {
                return Ok(String::new());
            }
            Ok(format!("{} s", samples.len() / TRANSCRIBE_RATE))
        }
    }

    fn speech(at_ms: u64, seconds: usize) -> Speech {
        Speech {
            at_ms,
            samples: vec![0.0; seconds * TRANSCRIBE_RATE],
        }
    }

    fn queue(gate: Option<mpsc::Receiver<()>>) -> (TranscribeQueue, mpsc::Receiver<TranscriptLine>) {
        let (tx, rx) = mpsc::channel();
        let queue = TranscribeQueue::start(Box::new(Fake { gate }), move |line| {
            let _ = tx.send(line);
        })
        .expect("thread");
        (queue, rx)
    }

    fn next(rx: &mpsc::Receiver<TranscriptLine>) -> TranscriptLine {
        rx.recv_timeout(Duration::from_secs(5)).expect("linha da transcrição")
    }

    #[test]
    fn falas_saem_transcritas_na_ordem() {
        let (queue, rx) = queue(None);
        queue.push(speech(1_000, 2));
        queue.push(speech(5_000, 3));
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 1_000,
                text: "2 s".into()
            }
        );
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 5_000,
                text: "3 s".into()
            }
        );
    }

    #[test]
    fn com_o_pc_atrasado_a_fala_mais_antiga_vira_trecho_nao_transcrito() {
        let (release, gate) = mpsc::channel();
        let (queue, rx) = queue(Some(gate));
        // A primeira fala prende o Whisper; as outras somam 36 s na fila, 6 s além do limite.
        queue.push(speech(0, 1));
        std::thread::sleep(Duration::from_millis(100));
        queue.push(speech(1_000, 12));
        queue.push(speech(2_000, 12));
        queue.push(speech(3_000, 12));
        for _ in 0..3 {
            release.send(()).unwrap();
        }
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 0,
                text: "1 s".into()
            }
        );
        assert_eq!(next(&rx), TranscriptLine::Skipped { at_ms: 1_000 });
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 2_000,
                text: "12 s".into()
            }
        );
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 3_000,
                text: "12 s".into()
            }
        );
    }

    #[test]
    fn erro_do_whisper_vira_linha_de_falha_e_a_fila_segue() {
        let (queue, rx) = queue(None);
        queue.push(Speech {
            at_ms: 7,
            samples: vec![-1.0; TRANSCRIBE_RATE],
        });
        queue.push(speech(9, 1));
        assert_eq!(
            next(&rx),
            TranscriptLine::Failed {
                at_ms: 7,
                error: "modelo quebrado".into()
            }
        );
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 9,
                text: "1 s".into()
            }
        );
    }

    #[test]
    fn fala_sem_texto_nao_vira_linha() {
        let (queue, rx) = queue(None);
        queue.push(Speech {
            at_ms: 1,
            samples: vec![0.5; TRANSCRIBE_RATE],
        });
        queue.push(speech(2, 1));
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 2,
                text: "1 s".into()
            }
        );
    }

    #[test]
    fn buffers_transcritos_voltam_para_o_microfone() {
        let (queue, rx) = queue(None);
        queue.push(speech(1, 1));
        next(&rx);
        // A thread devolve o buffer logo depois de mandar a linha.
        let mut buffer = None;
        for _ in 0..50 {
            buffer = queue.try_take_buffer();
            if buffer.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let buffer = buffer.expect("buffer de volta");
        assert!(buffer.is_empty());
        assert!(buffer.capacity() >= TRANSCRIBE_RATE);
    }

    #[test]
    fn descartar_a_fila_ainda_transcreve_o_que_ja_estava_nela() {
        let (release, gate) = mpsc::channel();
        let (queue, rx) = queue(Some(gate));
        queue.push(speech(1, 1));
        queue.push(speech(2, 2));
        drop(queue);
        release.send(()).unwrap();
        release.send(()).unwrap();
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 1,
                text: "1 s".into()
            }
        );
        assert_eq!(
            next(&rx),
            TranscriptLine::Text {
                at_ms: 2,
                text: "2 s".into()
            }
        );
    }

    #[test]
    fn descartar_a_fila_para_a_thread() {
        let (queue, rx) = queue(None);
        drop(queue);
        // Com a thread parada, o canal fecha (o `on_line` foi descartado junto com ela).
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }
}
