//! Áudio do jogo: captura do processo → Opus (quem transmite) e Opus → buffer de jitter → saída
//! (quem assiste).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
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
        let decoder = AudioDecoder::new().map_err(|e| e.to_string())?;
        let mut on_event = on_event;
        let thread = std::thread::Builder::new()
            .name("sv-audio-receiver".into())
            .spawn(move || {
                let mut playout = Playout::new(decoder, || AudioPlayer::start().map_err(|e| e.to_string()));
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(packet) => playout.push(packet),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    playout.tick(
                        thread_flag.load(Ordering::Relaxed),
                        f32::from_bits(thread_volume.load(Ordering::Relaxed)),
                        &mut on_event,
                    );
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
        self.volume.store(clamp_volume(volume).to_bits(), Ordering::Relaxed);
    }
}

/// Volume pedido pela interface, limitado de 0 a 2 (valor inválido volta para 100%).
fn clamp_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, sv_media::MAX_VOLUME)
    } else {
        1.0
    }
}

/// Saída de som de quem assiste: o [`AudioPlayer`] (nos testes, uma saída falsa).
trait Speaker {
    fn set_volume(&self, volume: f32);
    fn push(&self, samples: &[f32]);
}

impl Speaker for AudioPlayer {
    fn set_volume(&self, volume: f32) {
        AudioPlayer::set_volume(self, volume);
    }

    fn push(&self, samples: &[f32]) {
        AudioPlayer::push(self, samples);
    }
}

/// O que a thread de quem assiste faz a cada volta: abre ou fecha a saída conforme "ouvir",
/// aplica o volume e toca o que o buffer de jitter liberar. Os buffers são reaproveitados.
struct Playout<S, Open> {
    jitter: JitterBuffer,
    decoder: AudioDecoder,
    pcm: Vec<f32>,
    player: Option<S>,
    /// Volume já aplicado na saída atual (NaN = nenhum ainda).
    applied: f32,
    /// A saída falhou ao abrir: não tenta de novo (e só avisa uma vez).
    failed: bool,
    open: Open,
}

impl<S: Speaker, Open: FnMut() -> Result<S, String>> Playout<S, Open> {
    fn new(decoder: AudioDecoder, open: Open) -> Self {
        Self {
            jitter: JitterBuffer::new(JITTER_PACKETS),
            decoder,
            pcm: vec![0.0f32; FRAME_SAMPLES * CHANNELS],
            player: None,
            applied: f32::NAN,
            failed: false,
            open,
        }
    }

    fn push(&mut self, packet: AudioPacket) {
        self.jitter.push(packet.seq, packet.data);
    }

    fn tick(&mut self, wants: bool, volume: f32, emit: &mut impl FnMut(ReceiverEvent)) {
        // Abre ou fecha a saída conforme o usuário liga ou desliga "ouvir".
        if wants && self.player.is_none() && !self.failed {
            match (self.open)() {
                Ok(p) => self.player = Some(p),
                Err(e) => {
                    self.failed = true;
                    emit(ReceiverEvent::PlaybackFailed(e));
                }
            }
        } else if !wants {
            self.player = None;
            self.applied = f32::NAN;
        }
        if let Some(player) = &self.player
            && volume != self.applied
        {
            player.set_volume(volume);
            self.applied = volume;
        }
        while let Some(next) = self.jitter.pop() {
            let packet = match &next {
                Next::Packet(p) => Some(p.as_slice()),
                Next::Lost => {
                    emit(ReceiverEvent::Lost);
                    None
                }
            };
            if self.decoder.decode(packet, &mut self.pcm).is_ok()
                && let Some(player) = &self.player
            {
                player.push(&self.pcm);
            }
        }
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

    /// O que a saída falsa recebeu.
    #[derive(Default)]
    struct Heard {
        opened: u32,
        closed: u32,
        volumes: Vec<f32>,
        pushed: usize,
    }

    struct FakeSpeaker(Arc<std::sync::Mutex<Heard>>);

    impl Speaker for FakeSpeaker {
        fn set_volume(&self, volume: f32) {
            self.0.lock().unwrap().volumes.push(volume);
        }
        fn push(&self, samples: &[f32]) {
            assert_eq!(samples.len(), FRAME_SAMPLES * CHANNELS);
            self.0.lock().unwrap().pushed += 1;
        }
    }

    impl Drop for FakeSpeaker {
        fn drop(&mut self) {
            self.0.lock().unwrap().closed += 1;
        }
    }

    type FakeOpen = Box<dyn FnMut() -> Result<FakeSpeaker, String>>;

    fn playout(heard: &Arc<std::sync::Mutex<Heard>>) -> Playout<FakeSpeaker, FakeOpen> {
        let heard = heard.clone();
        Playout::new(
            AudioDecoder::new().unwrap(),
            Box::new(move || {
                heard.lock().unwrap().opened += 1;
                Ok(FakeSpeaker(heard.clone()))
            }),
        )
    }

    #[test]
    fn ouvir_abre_a_saida_toca_o_que_chega_e_aplica_o_volume() {
        let heard = Arc::new(std::sync::Mutex::new(Heard::default()));
        let mut p = playout(&heard);
        let mut got = Vec::new();
        let mut emit = |e| got.push(e);
        p.tick(true, 1.5, &mut emit);
        for seq in 0..6 {
            p.push(packet(seq));
        }
        p.tick(true, 1.5, &mut emit);
        p.tick(true, 1.5, &mut emit);
        let h = heard.lock().unwrap();
        assert_eq!(h.opened, 1, "a saída abre uma vez só");
        assert_eq!(h.volumes, vec![1.5], "o mesmo volume não é reaplicado");
        assert!(h.pushed >= 3, "tocou {} pacotes", h.pushed);
        drop(h);
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn mudar_o_volume_aplica_na_saida_aberta() {
        let heard = Arc::new(std::sync::Mutex::new(Heard::default()));
        let mut p = playout(&heard);
        let mut emit = |_| {};
        p.tick(true, 1.0, &mut emit);
        p.tick(true, 0.25, &mut emit);
        p.tick(true, 0.25, &mut emit);
        assert_eq!(heard.lock().unwrap().volumes, vec![1.0, 0.25]);
    }

    #[test]
    fn desligar_ouvir_fecha_a_saida_e_religar_reaplica_o_volume() {
        let heard = Arc::new(std::sync::Mutex::new(Heard::default()));
        let mut p = playout(&heard);
        let mut emit = |_| {};
        p.tick(true, 0.8, &mut emit);
        p.tick(false, 0.8, &mut emit);
        {
            let h = heard.lock().unwrap();
            assert_eq!((h.opened, h.closed), (1, 1));
        }
        // Sem ouvir, os pacotes continuam sendo decodificados (para medir), mas nada toca.
        for seq in 0..6 {
            p.push(packet(seq));
        }
        p.tick(false, 0.8, &mut emit);
        assert_eq!(heard.lock().unwrap().pushed, 0);
        p.tick(true, 0.8, &mut emit);
        let h = heard.lock().unwrap();
        assert_eq!(h.opened, 2);
        assert_eq!(h.volumes, vec![0.8, 0.8], "a saída nova recebe o volume de novo");
    }

    #[test]
    fn saida_que_nao_abre_avisa_uma_vez_e_nao_insiste() {
        let tries = Arc::new(std::sync::Mutex::new(0u32));
        let counter = tries.clone();
        let open: Box<dyn FnMut() -> Result<FakeSpeaker, String>> = Box::new(move || {
            *counter.lock().unwrap() += 1;
            Err("sem placa de som".into())
        });
        let mut p = Playout::new(AudioDecoder::new().unwrap(), open);
        let mut got = Vec::new();
        let mut emit = |e| got.push(e);
        for _ in 0..3 {
            p.tick(true, 1.0, &mut emit);
        }
        assert_eq!(*tries.lock().unwrap(), 1);
        assert_eq!(got, vec![ReceiverEvent::PlaybackFailed("sem placa de som".into())]);
    }

    #[test]
    fn pacote_perdido_e_disfarcado_e_avisado() {
        let heard = Arc::new(std::sync::Mutex::new(Heard::default()));
        let mut p = playout(&heard);
        let mut got = Vec::new();
        let mut emit = |e| got.push(e);
        for seq in [0, 1, 3, 4, 5, 6] {
            p.push(packet(seq));
        }
        p.tick(true, 1.0, &mut emit);
        assert_eq!(got, vec![ReceiverEvent::Lost]);
        // O buraco também vira som (o decoder disfarça), então a saída recebe todos.
        assert!(heard.lock().unwrap().pushed >= 4);
    }

    #[test]
    fn volume_fica_entre_0_e_2_e_invalido_volta_para_100() {
        assert_eq!(clamp_volume(0.5), 0.5);
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(5.0), sv_media::MAX_VOLUME);
        assert_eq!(clamp_volume(f32::NAN), 1.0);
        assert_eq!(clamp_volume(f32::INFINITY), 1.0);
    }

    #[test]
    fn receptor_guarda_volume_e_ouvir_pedidos_pela_interface() {
        let receiver = AudioReceiver::start(false, |_| {}).unwrap();
        receiver.set_volume(3.0);
        assert_eq!(
            f32::from_bits(receiver.volume.load(Ordering::Relaxed)),
            sv_media::MAX_VOLUME
        );
        receiver.set_volume(f32::NAN);
        assert_eq!(f32::from_bits(receiver.volume.load(Ordering::Relaxed)), 1.0);
        // Só a flag: a thread abriria a saída de verdade se "ouvir" ficasse ligado.
        receiver.set_listening(false);
        assert!(format!("{receiver:?}").contains("listening: false"));
        assert!(receiver.submit(packet(0)));
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
