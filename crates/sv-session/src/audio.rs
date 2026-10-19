//! Áudio do jogo: captura do processo → Opus (quem transmite) e Opus → buffer de jitter → saída
//! (quem assiste).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use sv_capture::AudioCapture;
use sv_gpu::clock;
use sv_media::AudioPlayer;
use sv_media::jitter::{JitterBuffer, Next};
use sv_media::opus::{AudioDecoder, AudioEncoder, CHANNELS, FRAME_SAMPLES, peak};

/// Um pacote Opus de 20 ms, pronto para a rede.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPacket {
    pub seq: u32,
    pub data: Vec<u8>,
    /// Pico do som neste pacote (0 a 1), para o medidor.
    pub peak: f32,
    pub timestamp_us: u64,
}

/// Captura e codifica o áudio de um processo. Para ao ser descartado.
#[derive(Debug)]
pub struct AudioSender {
    _capture: AudioCapture,
}

impl AudioSender {
    pub fn start(pid: u32, on_packet: impl FnMut(AudioPacket) + Send + 'static) -> Result<Self, String> {
        let mut encoder = AudioEncoder::new().map_err(|e| e.to_string())?;
        let mut on_packet = on_packet;
        let mut seq = 0u32;
        // Pico do bloco inteiro capturado: os pacotes que saem dele herdam o valor.
        let capture = AudioCapture::start(pid, move |samples| {
            let level = peak(samples);
            let _ = encoder.push(samples, |data| {
                on_packet(AudioPacket {
                    seq,
                    data: data.to_vec(),
                    peak: level,
                    timestamp_us: clock::now_us(),
                });
                seq = seq.wrapping_add(1);
            });
        })
        .map_err(|e| format!("não deu para capturar o áudio desse aplicativo: {e}"))?;
        Ok(Self { _capture: capture })
    }
}

/// Eventos do [`AudioReceiver`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverEvent {
    /// Um pacote não chegou a tempo e foi disfarçado.
    Lost,
    /// Não deu para abrir a saída de áudio.
    PlaybackFailed(String),
}

/// Pacotes atrasados aceitos na fila de entrada (2 s).
const INBOX: usize = 100;
/// Atraso do buffer de jitter: 3 pacotes = 60 ms.
const JITTER_PACKETS: usize = 3;

/// Recebe pacotes, reordena, decodifica e toca (se `listening`). Para ao ser descartado.
pub struct AudioReceiver {
    inbox: Option<SyncSender<AudioPacket>>,
    listening: Arc<AtomicBool>,
    /// Volume (bits de um f32, de 0 a 2), lido pela thread a cada volta.
    volume: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for AudioReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioReceiver")
            .field("listening", &self.listening.load(Ordering::Relaxed))
            .finish()
    }
}

impl AudioReceiver {
    pub fn start(listening: bool, on_event: impl FnMut(ReceiverEvent) + Send + 'static) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<AudioPacket>(INBOX);
        let flag = Arc::new(AtomicBool::new(listening));
        let thread_flag = flag.clone();
        let volume = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let thread_volume = volume.clone();
        let mut decoder = AudioDecoder::new().map_err(|e| e.to_string())?;
        let on_event = Arc::new(Mutex::new(on_event));
        let thread = std::thread::Builder::new()
            .name("sv-audio-receiver".into())
            .spawn(move || {
                let emit = |e| (on_event.lock().unwrap_or_else(|p| p.into_inner()))(e);
                let mut jitter = JitterBuffer::new(JITTER_PACKETS);
                let mut player: Option<AudioPlayer> = None;
                // Volume já aplicado na saída atual (NaN = nenhum ainda).
                let mut applied = f32::NAN;
                let mut playback_failed = false;
                let mut pcm = vec![0.0f32; FRAME_SAMPLES * CHANNELS];
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(packet) => jitter.push(packet.seq, packet.data),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    // Abre ou fecha a saída conforme o usuário liga ou desliga "ouvir".
                    let wants = thread_flag.load(Ordering::Relaxed);
                    if wants && player.is_none() && !playback_failed {
                        match AudioPlayer::start() {
                            Ok(p) => player = Some(p),
                            Err(e) => {
                                playback_failed = true;
                                emit(ReceiverEvent::PlaybackFailed(e.to_string()));
                            }
                        }
                    } else if !wants {
                        player = None;
                        applied = f32::NAN;
                    }
                    let wanted = f32::from_bits(thread_volume.load(Ordering::Relaxed));
                    if let Some(player) = &player
                        && wanted != applied
                    {
                        player.set_volume(wanted);
                        applied = wanted;
                    }
                    while let Some(next) = jitter.pop() {
                        let packet = match &next {
                            Next::Packet(p) => Some(p.as_slice()),
                            Next::Lost => {
                                emit(ReceiverEvent::Lost);
                                None
                            }
                        };
                        if decoder.decode(packet, &mut pcm).is_ok()
                            && let Some(player) = &player
                        {
                            player.push(&pcm);
                        }
                    }
                }
            })
            .expect("não foi possível criar a thread de áudio de quem assiste");
        Ok(Self {
            inbox: Some(tx),
            listening: flag,
            volume,
            thread: Some(thread),
        })
    }

    /// Entrega um pacote recebido. Devolve `false` se a fila estava cheia.
    pub fn submit(&self, packet: AudioPacket) -> bool {
        match self.inbox.as_ref().map(|tx| tx.try_send(packet)) {
            Some(Ok(())) => true,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => false,
        }
    }

    /// Liga ou desliga a saída de áudio (a decodificação continua, para medir).
    pub fn set_listening(&self, listening: bool) {
        self.listening.store(listening, Ordering::Relaxed);
    }

    /// Volume de 0 (mudo) a 2 (200%).
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, sv_media::MAX_VOLUME)
        } else {
            1.0
        };
        self.volume.store(volume.to_bits(), Ordering::Relaxed);
    }
}

impl Drop for AudioReceiver {
    fn drop(&mut self) {
        self.inbox = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(seq: u32) -> AudioPacket {
        let mut enc = AudioEncoder::new().unwrap();
        let mut out = Vec::new();
        enc.push(&vec![0.1; FRAME_SAMPLES * CHANNELS], |p| out = p.to_vec())
            .unwrap();
        AudioPacket {
            seq,
            data: out,
            peak: 0.1,
            timestamp_us: 0,
        }
    }

    #[test]
    fn avisa_quando_um_pacote_se_perde() {
        let (tx, rx) = mpsc::channel();
        let receiver = AudioReceiver::start(false, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        for seq in [0, 1, 3, 4, 5] {
            assert!(receiver.submit(packet(seq)));
        }
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(ReceiverEvent::Lost));
        drop(receiver);
    }

    #[test]
    #[ignore = "precisa de saída de áudio"]
    fn ligar_ouvir_abre_a_saida_sem_erro() {
        let (tx, rx) = mpsc::channel();
        let receiver = AudioReceiver::start(true, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        for seq in 0..10 {
            receiver.submit(packet(seq));
        }
        std::thread::sleep(Duration::from_millis(300));
        receiver.set_listening(false);
        assert!(rx.try_recv().is_err(), "nenhum evento de falha esperado");
    }
}
