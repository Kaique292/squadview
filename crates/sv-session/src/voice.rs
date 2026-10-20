//! Motor do chat de voz da sala (ADR 0010): microfone → pacotes para os amigos, e pacotes dos
//! amigos → mixer → uma saída de áudio. A rede fica com a [`crate::Room`]; aqui só o áudio.
//!
//! Threads: a do microfone (captura, ruído, Opus) e a do mixer (a cada ~10 ms completa a fila da
//! saída com 20 ms misturados). A rede só enfileira pacotes, sem esperar ninguém.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sv_media::jitter::{JitterBuffer, Next};
use sv_media::voice::{VOICE_FRAME, VoiceDecoder, VoiceEncoder, VoiceOptions};
use sv_media::{AudioPlayer, MAX_VOLUME, MicCapture};

use crate::surface::lock;

/// Pacotes guardados antes de tocar a voz de alguém (60 ms): absorve a variação da rede.
const JITTER_PACKETS: usize = 3;
/// Quanto áudio manter na fila da saída. Abaixo disso o mixer manda mais 20 ms.
const OUTPUT_TARGET_MS: usize = 60;
/// Por quanto tempo alguém continua "falando" depois do último pacote com som.
const SPEAKING_HOLD: Duration = Duration::from_millis(300);
/// Pico mínimo para contar como fala (abaixo é respiração ou resto de ruído).
const SPEAKING_PEAK: f32 = 0.02;

/// Escolhas do usuário para a voz (ficam salvas na interface e chegam aqui).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSettings {
    /// Microfone (id do Windows) ou `None` para o padrão de chamadas.
    pub input_device: Option<String>,
    /// Saída (id do Windows) ou `None` para a padrão de chamadas.
    pub output_device: Option<String>,
    /// Volume do microfone, de 0 a 2.
    pub input_gain: f32,
    /// Volume geral da saída, de 0 a 2.
    pub output_volume: f32,
    pub noise_suppression: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_gain: 1.0,
            output_volume: 1.0,
            noise_suppression: true,
        }
    }
}

/// O que a interface precisa saber da voz.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceEvent {
    /// Alguém começou ou parou de falar (`None` = nós).
    Speaking { member: Option<String>, speaking: bool },
    /// Nível do nosso microfone (0 a 1), ~5 vezes por segundo, para o medidor das configurações.
    InputLevel(f32),
    /// Não deu para abrir o microfone (a voz segue só ouvindo).
    MicFailed(String),
    /// Não deu para abrir a saída de áudio.
    OutputFailed(String),
}

type OnEvent = Arc<dyn Fn(VoiceEvent) + Send + Sync>;
/// Recebe cada pacote de voz nosso: (seq, pico, opus).
pub type OnPacket = Arc<dyn Fn(u32, f32, &[u8]) + Send + Sync>;

/// A voz de um amigo.
struct Peer {
    jitter: JitterBuffer,
    decoder: VoiceDecoder,
    /// Volume só para nós, de 0 a 2.
    volume: f32,
    /// Silenciado só para nós.
    muted: bool,
    /// Último pacote com som, para o "está falando".
    last_voice: Option<Instant>,
    speaking: bool,
}

struct Shared {
    settings: Mutex<VoiceSettings>,
    muted: AtomicBool,
    deafened: AtomicBool,
    peers: Mutex<HashMap<String, Peer>>,
    /// A saída precisa ser reaberta (outro dispositivo).
    reopen_output: AtomicBool,
    stop: AtomicBool,
    on_event: OnEvent,
}

/// O chat de voz em andamento. Para tudo ao ser descartado.
pub struct VoiceEngine {
    shared: Arc<Shared>,
    encoder: Arc<Mutex<VoiceEncoder>>,
    on_packet: OnPacket,
    seq: Arc<AtomicU32>,
    mic: Mutex<Option<MicCapture>>,
    mixer: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for VoiceEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceEngine").finish_non_exhaustive()
    }
}

impl VoiceEngine {
    /// Liga o microfone e o mixer. Falha no microfone não impede de ouvir os outros.
    pub fn start(
        settings: VoiceSettings,
        on_packet: OnPacket,
        on_event: impl Fn(VoiceEvent) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let options = VoiceOptions {
            noise_suppression: settings.noise_suppression,
            input_gain: settings.input_gain,
        };
        let encoder = Arc::new(Mutex::new(VoiceEncoder::new(options).map_err(|e| e.to_string())?));
        let shared = Arc::new(Shared {
            settings: Mutex::new(settings),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            peers: Mutex::new(HashMap::new()),
            reopen_output: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            on_event: Arc::new(on_event),
        });
        let mixer_shared = shared.clone();
        let mixer = std::thread::Builder::new()
            .name("sv-voice-mixer".into())
            .spawn(move || run_mixer(&mixer_shared))
            .map_err(|e| e.to_string())?;
        let engine = Self {
            shared,
            encoder,
            on_packet,
            seq: Arc::new(AtomicU32::new(0)),
            mic: Mutex::new(None),
            mixer: Some(mixer),
        };
        engine.open_mic();
        Ok(engine)
    }

    /// (Re)abre o microfone com as configurações atuais.
    fn open_mic(&self) {
        lock(&self.mic).take();
        let device = lock(&self.shared.settings).input_device.clone();
        let shared = self.shared.clone();
        let encoder = self.encoder.clone();
        let on_packet = self.on_packet.clone();
        let seq = self.seq.clone();
        let mut speaking = false;
        let mut level_peak = 0.0f32;
        let mut level_frames = 0u32;
        let result = MicCapture::start(device, move |samples| {
            // Mudo (ou surdo): não processa nem manda nada, como no Discord.
            if shared.muted.load(Ordering::Relaxed) || shared.deafened.load(Ordering::Relaxed) {
                if speaking {
                    speaking = false;
                    (shared.on_event)(VoiceEvent::Speaking {
                        member: None,
                        speaking: false,
                    });
                }
                return;
            }
            let mut encoder = lock(&encoder);
            let _ = encoder.push(
                samples,
                |level, open| {
                    if open != speaking {
                        speaking = open;
                        (shared.on_event)(VoiceEvent::Speaking {
                            member: None,
                            speaking: open,
                        });
                    }
                    level_peak = level_peak.max(level);
                    level_frames += 1;
                    // A cada 200 ms (10 pacotes de 20 ms), o nível para o medidor.
                    if level_frames >= 10 {
                        (shared.on_event)(VoiceEvent::InputLevel(level_peak));
                        level_peak = 0.0;
                        level_frames = 0;
                    }
                },
                |frame| {
                    let n = seq.fetch_add(1, Ordering::Relaxed);
                    on_packet(n, frame.peak, frame.opus);
                },
            );
        });
        match result {
            Ok(mic) => *lock(&self.mic) = Some(mic),
            Err(e) => (self.shared.on_event)(VoiceEvent::MicFailed(e.to_string())),
        }
    }

    /// Silencia o nosso microfone (para todos).
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    /// Ensurdece: não ouvimos ninguém e o microfone também fica mudo.
    pub fn set_deafened(&self, deafened: bool) {
        self.shared.deafened.store(deafened, Ordering::Relaxed);
    }

    /// Troca as configurações; reabre o microfone ou a saída se o dispositivo mudou.
    pub fn set_settings(&self, settings: VoiceSettings) {
        let previous = std::mem::replace(&mut *lock(&self.shared.settings), settings.clone());
        lock(&self.encoder).set_options(VoiceOptions {
            noise_suppression: settings.noise_suppression,
            input_gain: settings.input_gain,
        });
        if previous.input_device != settings.input_device {
            self.open_mic();
        }
        if previous.output_device != settings.output_device {
            self.shared.reopen_output.store(true, Ordering::Relaxed);
        }
    }

    /// Começa a tocar a voz de `member` (a conexão de voz com ele abriu).
    pub fn add_peer(&self, member: &str) -> Result<(), String> {
        let decoder = VoiceDecoder::new().map_err(|e| e.to_string())?;
        lock(&self.shared.peers).entry(member.to_owned()).or_insert(Peer {
            jitter: JitterBuffer::new(JITTER_PACKETS),
            decoder,
            volume: 1.0,
            muted: false,
            last_voice: None,
            speaking: false,
        });
        Ok(())
    }

    /// Para de tocar a voz de `member` (saiu ou a conexão caiu).
    pub fn remove_peer(&self, member: &str) {
        let removed = lock(&self.shared.peers).remove(member);
        if removed.is_some_and(|p| p.speaking) {
            (self.shared.on_event)(VoiceEvent::Speaking {
                member: Some(member.to_owned()),
                speaking: false,
            });
        }
    }

    /// Um pacote de voz chegou de `member`.
    pub fn submit(&self, member: &str, seq: u32, peak: f32, opus: &[u8]) {
        let mut peers = lock(&self.shared.peers);
        if let Some(peer) = peers.get_mut(member) {
            peer.jitter.push(seq, opus.to_vec());
            if peak >= SPEAKING_PEAK {
                peer.last_voice = Some(Instant::now());
            }
        }
    }

    /// Volume de `member` só para nós, de 0 a 2.
    pub fn set_peer_volume(&self, member: &str, volume: f32) {
        if let Some(peer) = lock(&self.shared.peers).get_mut(member) {
            peer.volume = volume.clamp(0.0, MAX_VOLUME);
        }
    }

    /// Silencia `member` só para nós.
    pub fn set_peer_muted(&self, member: &str, muted: bool) {
        if let Some(peer) = lock(&self.shared.peers).get_mut(member) {
            peer.muted = muted;
        }
    }

    /// Com quem a voz está ligada.
    pub fn peers(&self) -> Vec<String> {
        lock(&self.shared.peers).keys().cloned().collect()
    }
}

impl Drop for VoiceEngine {
    fn drop(&mut self) {
        lock(&self.mic).take();
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(mixer) = self.mixer.take() {
            let _ = mixer.join();
        }
    }
}

/// Mistura 20 ms de todo mundo em `mix` (mono). Devolve quem mudou de "falando".
fn mix_frame(
    peers: &mut HashMap<String, Peer>,
    mix: &mut [f32],
    decoded: &mut [f32],
    now: Instant,
) -> Vec<(String, bool)> {
    mix.fill(0.0);
    let mut changes = Vec::new();
    for (id, peer) in peers.iter_mut() {
        let packet = match peer.jitter.pop() {
            Some(Next::Packet(p)) => Some(p),
            // Perdido no meio da fala: o decoder disfarça (PLC).
            Some(Next::Lost) => None,
            // Calado (ou enchendo o buffer): nada a tocar.
            None => {
                update_speaking(id, peer, now, &mut changes);
                continue;
            }
        };
        if peer.decoder.decode(packet.as_deref(), decoded).is_ok() && !peer.muted {
            for (m, d) in mix.iter_mut().zip(decoded.iter()) {
                *m += d * peer.volume;
            }
        }
        update_speaking(id, peer, now, &mut changes);
    }
    changes
}

fn update_speaking(id: &str, peer: &mut Peer, now: Instant, changes: &mut Vec<(String, bool)>) {
    // Silenciado para nós não aparece falando, como no Discord.
    let speaking = !peer.muted && peer.last_voice.is_some_and(|t| now.duration_since(t) < SPEAKING_HOLD);
    if speaking != peer.speaking {
        peer.speaking = speaking;
        changes.push((id.to_owned(), speaking));
    }
}

fn run_mixer(shared: &Shared) {
    let mut player: Option<AudioPlayer> = None;
    let mut failed = false;
    let mut mix = vec![0.0f32; VOICE_FRAME];
    let mut decoded = vec![0.0f32; VOICE_FRAME];
    let mut stereo = vec![0.0f32; VOICE_FRAME * 2];
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.reopen_output.swap(false, Ordering::Relaxed) {
            player = None;
            failed = false;
        }
        if player.is_none() && !failed {
            let device = lock(&shared.settings).output_device.clone();
            match AudioPlayer::start_on(device) {
                Ok(p) => player = Some(p),
                Err(e) => {
                    failed = true;
                    (shared.on_event)(VoiceEvent::OutputFailed(e.to_string()));
                }
            }
        }
        // Completa a fila da saída até ~60 ms, 20 ms por vez.
        let mut frames = 0;
        while frames < 4 && player.as_ref().is_some_and(|p| p.buffered_ms() < OUTPUT_TARGET_MS) {
            let changes = mix_frame(&mut lock(&shared.peers), &mut mix, &mut decoded, Instant::now());
            for (member, speaking) in changes {
                (shared.on_event)(VoiceEvent::Speaking {
                    member: Some(member),
                    speaking,
                });
            }
            let deafened = shared.deafened.load(Ordering::Relaxed);
            let volume = if deafened {
                0.0
            } else {
                lock(&shared.settings).output_volume.clamp(0.0, MAX_VOLUME)
            };
            for (pair, m) in stereo.chunks_exact_mut(2).zip(&mix) {
                let s = (m * volume).clamp(-1.0, 1.0);
                pair[0] = s;
                pair[1] = s;
            }
            if let Some(p) = &player {
                p.push(&stereo);
            }
            frames += 1;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sv_media::voice::VoiceEncoder;

    fn peer() -> Peer {
        Peer {
            jitter: JitterBuffer::new(1),
            decoder: VoiceDecoder::new().unwrap(),
            volume: 1.0,
            muted: false,
            last_voice: None,
            speaking: false,
        }
    }

    /// Pacotes de voz de um tom (sem supressão de ruído, para o portão abrir).
    fn packets(n: usize) -> Vec<Vec<u8>> {
        let mut enc = VoiceEncoder::new(VoiceOptions {
            noise_suppression: false,
            input_gain: 1.0,
        })
        .unwrap();
        let tone: Vec<f32> = (0..VOICE_FRAME * n)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut out = Vec::new();
        enc.push(&tone, |_, _| {}, |f| out.push(f.opus.to_vec())).unwrap();
        out
    }

    fn feed(p: &mut Peer, packets: &[Vec<u8>]) {
        for (i, pk) in packets.iter().enumerate() {
            p.jitter.push(i as u32, pk.clone());
        }
        p.last_voice = Some(Instant::now());
    }

    fn mix_level(peers: &mut HashMap<String, Peer>, frames: usize) -> f32 {
        let mut mix = vec![0.0; VOICE_FRAME];
        let mut decoded = vec![0.0; VOICE_FRAME];
        let mut top = 0.0f32;
        for _ in 0..frames {
            mix_frame(peers, &mut mix, &mut decoded, Instant::now());
            top = top.max(sv_media::opus::peak(&mix));
        }
        top
    }

    #[test]
    fn mistura_a_voz_de_quem_fala_e_avisa_que_esta_falando() {
        let mut peers = HashMap::from([("ana".to_string(), peer())]);
        feed(peers.get_mut("ana").unwrap(), &packets(10));
        let mut mix = vec![0.0; VOICE_FRAME];
        let mut decoded = vec![0.0; VOICE_FRAME];
        let changes = mix_frame(&mut peers, &mut mix, &mut decoded, Instant::now());
        assert_eq!(changes, vec![("ana".to_string(), true)]);
        assert!(mix_level(&mut peers, 8) > 0.1);
    }

    #[test]
    fn silenciar_so_para_mim_tira_do_mix_e_do_falando() {
        let mut peers = HashMap::from([("ana".to_string(), peer())]);
        let p = peers.get_mut("ana").unwrap();
        p.muted = true;
        feed(p, &packets(10));
        assert_eq!(mix_level(&mut peers, 9), 0.0);
        assert!(!peers["ana"].speaking);
    }

    #[test]
    fn volume_por_pessoa_muda_o_quanto_ela_soa() {
        let level = |volume: f32| {
            let mut peers = HashMap::from([("ana".to_string(), peer())]);
            let p = peers.get_mut("ana").unwrap();
            p.volume = volume;
            feed(p, &packets(10));
            mix_level(&mut peers, 9)
        };
        let normal = level(1.0);
        let half = level(0.5);
        assert!(half > 0.0 && half < normal * 0.7, "normal {normal}, metade {half}");
    }

    #[test]
    fn calado_nao_toca_nada_e_para_de_aparecer_falando() {
        let mut peers = HashMap::from([("ana".to_string(), peer())]);
        let p = peers.get_mut("ana").unwrap();
        p.speaking = true;
        p.last_voice = Some(Instant::now() - SPEAKING_HOLD * 2);
        let mut mix = vec![1.0; VOICE_FRAME];
        let mut decoded = vec![0.0; VOICE_FRAME];
        let changes = mix_frame(&mut peers, &mut mix, &mut decoded, Instant::now());
        assert!(mix.iter().all(|s| *s == 0.0));
        assert_eq!(changes, vec![("ana".to_string(), false)]);
    }

    #[test]
    #[ignore = "precisa de microfone e saída de áudio"]
    fn liga_e_desliga_com_os_dispositivos_padrao() {
        let engine = VoiceEngine::start(VoiceSettings::default(), Arc::new(|_, _, _| {}), |_| {}).unwrap();
        engine.add_peer("ana").unwrap();
        engine.set_muted(true);
        std::thread::sleep(Duration::from_millis(300));
        drop(engine);
    }
}
