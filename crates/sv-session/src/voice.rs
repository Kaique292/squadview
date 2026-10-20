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
use sv_media::{AudioPlayer, MicCapture};

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
    /// Controle automático de ganho do microfone.
    pub auto_gain: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_gain: 1.0,
            output_volume: 1.0,
            noise_suppression: true,
            auto_gain: true,
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

impl Shared {
    fn new(settings: VoiceSettings, on_event: OnEvent) -> Self {
        Self {
            settings: Mutex::new(settings),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            peers: Mutex::new(HashMap::new()),
            reopen_output: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            on_event,
        }
    }

    /// Troca as configurações. Devolve `true` se o microfone mudou (e precisa ser reaberto); se a
    /// saída mudou, o mixer a reabre na próxima volta.
    fn replace_settings(&self, settings: VoiceSettings) -> bool {
        let mut current = lock(&self.settings);
        let mic_changed = current.input_device != settings.input_device;
        if current.output_device != settings.output_device {
            self.reopen_output.store(true, Ordering::Relaxed);
        }
        *current = settings;
        mic_changed
    }

    fn add_peer(&self, member: &str) -> Result<(), String> {
        let decoder = VoiceDecoder::new().map_err(|e| e.to_string())?;
        lock(&self.peers).entry(member.to_owned()).or_insert(Peer {
            jitter: JitterBuffer::new(JITTER_PACKETS),
            decoder,
            volume: 1.0,
            muted: false,
            last_voice: None,
            speaking: false,
        });
        Ok(())
    }

    fn remove_peer(&self, member: &str) {
        let removed = lock(&self.peers).remove(member);
        if removed.is_some_and(|p| p.speaking) {
            (self.on_event)(VoiceEvent::Speaking {
                member: Some(member.to_owned()),
                speaking: false,
            });
        }
    }

    fn submit(&self, member: &str, seq: u32, peak: f32, opus: &[u8], now: Instant) {
        let mut peers = lock(&self.peers);
        if let Some(peer) = peers.get_mut(member) {
            peer.jitter.push(seq, opus.to_vec());
            if peak >= SPEAKING_PEAK {
                peer.last_voice = Some(now);
            }
        }
    }

    fn set_peer_volume(&self, member: &str, volume: f32) {
        if let Some(peer) = lock(&self.peers).get_mut(member) {
            peer.volume = sv_media::volume_gain(volume);
        }
    }

    fn set_peer_muted(&self, member: &str, muted: bool) {
        if let Some(peer) = lock(&self.peers).get_mut(member) {
            peer.muted = muted;
        }
    }

    fn peers(&self) -> Vec<String> {
        lock(&self.peers).keys().cloned().collect()
    }
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
        let mut engine = Self::idle(settings, on_packet, Arc::new(on_event))?;
        let mixer_shared = engine.shared.clone();
        let mixer = std::thread::Builder::new()
            .name("sv-voice-mixer".into())
            .spawn(move || run_mixer(&mixer_shared))
            .map_err(|e| e.to_string())?;
        engine.mixer = Some(mixer);
        engine.open_mic();
        Ok(engine)
    }

    /// O motor sem microfone nem mixer (o [`Self::start`] liga os dois).
    fn idle(settings: VoiceSettings, on_packet: OnPacket, on_event: OnEvent) -> Result<Self, String> {
        let options = VoiceOptions {
            noise_suppression: settings.noise_suppression,
            input_gain: settings.input_gain,
            auto_gain: settings.auto_gain,
        };
        let encoder = Arc::new(Mutex::new(VoiceEncoder::new(options).map_err(|e| e.to_string())?));
        Ok(Self {
            shared: Arc::new(Shared::new(settings, on_event)),
            encoder,
            on_packet,
            seq: Arc::new(AtomicU32::new(0)),
            mic: Mutex::new(None),
            mixer: None,
        })
    }

    /// Um motor que não abre dispositivo nenhum, para testar quem usa a voz sem microfone.
    #[cfg(test)]
    pub(crate) fn without_devices(
        settings: VoiceSettings,
        on_packet: OnPacket,
        on_event: impl Fn(VoiceEvent) + Send + Sync + 'static,
    ) -> Self {
        Self::idle(settings, on_packet, Arc::new(on_event)).expect("encoder de voz")
    }

    /// Um microfone de mentira: cada bloco entregue passa pelo mesmo caminho da captura real.
    #[cfg(test)]
    pub(crate) fn fake_mic(&self) -> impl FnMut(&[f32]) + use<> {
        let mut pipeline = self.mic_pipeline();
        move |samples| pipeline.process(samples)
    }

    /// Pacotes de `member` esperando no buffer de jitter (`None`: ele não está na voz).
    #[cfg(test)]
    pub(crate) fn buffered(&self, member: &str) -> Option<usize> {
        lock(&self.shared.peers).get(member).map(|p| p.jitter.len())
    }

    /// O que roda a cada bloco do microfone (mudo, portão de ruído, pacotes, medidor).
    fn mic_pipeline(&self) -> MicPipeline {
        MicPipeline {
            shared: self.shared.clone(),
            encoder: self.encoder.clone(),
            on_packet: self.on_packet.clone(),
            seq: self.seq.clone(),
            speaking: false,
            level_peak: 0.0,
            level_frames: 0,
        }
    }

    /// (Re)abre o microfone com as configurações atuais.
    fn open_mic(&self) {
        lock(&self.mic).take();
        let device = lock(&self.shared.settings).input_device.clone();
        let mut pipeline = self.mic_pipeline();
        match MicCapture::start(device, move |samples| pipeline.process(samples)) {
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
        lock(&self.encoder).set_options(VoiceOptions {
            noise_suppression: settings.noise_suppression,
            input_gain: settings.input_gain,
            auto_gain: settings.auto_gain,
        });
        if self.shared.replace_settings(settings) {
            self.open_mic();
        }
    }

    /// Começa a tocar a voz de `member` (a conexão de voz com ele abriu).
    pub fn add_peer(&self, member: &str) -> Result<(), String> {
        self.shared.add_peer(member)
    }

    /// Para de tocar a voz de `member` (saiu ou a conexão caiu).
    pub fn remove_peer(&self, member: &str) {
        self.shared.remove_peer(member);
    }

    /// Um pacote de voz chegou de `member`.
    pub fn submit(&self, member: &str, seq: u32, peak: f32, opus: &[u8]) {
        self.shared.submit(member, seq, peak, opus, Instant::now());
    }

    /// Volume de `member` só para nós, de 0 a 2.
    pub fn set_peer_volume(&self, member: &str, volume: f32) {
        self.shared.set_peer_volume(member, volume);
    }

    /// Silencia `member` só para nós.
    pub fn set_peer_muted(&self, member: &str, muted: bool) {
        self.shared.set_peer_muted(member, muted);
    }

    /// Com quem a voz está ligada.
    pub fn peers(&self) -> Vec<String> {
        self.shared.peers()
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

/// O caminho do microfone, na thread dele: mudo → nada; senão ruído, portão e Opus, avisando
/// quando começamos ou paramos de falar e o nível a cada 200 ms.
struct MicPipeline {
    shared: Arc<Shared>,
    encoder: Arc<Mutex<VoiceEncoder>>,
    on_packet: OnPacket,
    seq: Arc<AtomicU32>,
    speaking: bool,
    level_peak: f32,
    level_frames: u32,
}

impl MicPipeline {
    fn process(&mut self, samples: &[f32]) {
        let Self {
            shared,
            encoder,
            on_packet,
            seq,
            speaking,
            level_peak,
            level_frames,
        } = self;
        // Mudo (ou surdo): não processa nem manda nada, como no Discord.
        if shared.muted.load(Ordering::Relaxed) || shared.deafened.load(Ordering::Relaxed) {
            if *speaking {
                *speaking = false;
                (shared.on_event)(VoiceEvent::Speaking {
                    member: None,
                    speaking: false,
                });
            }
            return;
        }
        let mut encoder = lock(encoder);
        let _ = encoder.push(
            samples,
            |level, open| {
                if open != *speaking {
                    *speaking = open;
                    (shared.on_event)(VoiceEvent::Speaking {
                        member: None,
                        speaking: open,
                    });
                }
                *level_peak = level_peak.max(level);
                *level_frames += 1;
                // A cada 200 ms (10 pacotes de 20 ms), o nível para o medidor.
                if *level_frames >= 10 {
                    (shared.on_event)(VoiceEvent::InputLevel(*level_peak));
                    *level_peak = 0.0;
                    *level_frames = 0;
                }
            },
            |frame| {
                let n = seq.fetch_add(1, Ordering::Relaxed);
                on_packet(n, frame.peak, frame.opus);
            },
        );
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

/// Mistura os próximos 20 ms em `stereo` (intercalado), já com o volume geral (surdo: silêncio),
/// e avisa quem começou ou parou de falar.
fn render_output(shared: &Shared, mix: &mut [f32], decoded: &mut [f32], stereo: &mut [f32], now: Instant) {
    let changes = mix_frame(&mut lock(&shared.peers), mix, decoded, now);
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
        sv_media::volume_gain(lock(&shared.settings).output_volume)
    };
    for (pair, m) in stereo.chunks_exact_mut(2).zip(mix.iter()) {
        let s = sv_media::soft_clip(m * volume);
        pair[0] = s;
        pair[1] = s;
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
        while frames < 4
            && let Some(p) = player.as_ref().filter(|p| p.buffered_ms() < OUTPUT_TARGET_MS)
        {
            render_output(shared, &mut mix, &mut decoded, &mut stereo, Instant::now());
            p.push(&stereo);
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
            auto_gain: false,
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

    /// Eventos de voz guardados para o teste conferir.
    type Events = Arc<Mutex<Vec<VoiceEvent>>>;

    fn recorder() -> (Events, OnEvent) {
        let events: Events = Arc::default();
        let sink = events.clone();
        (events, Arc::new(move |e| lock(&sink).push(e)))
    }

    fn sem_supressao() -> VoiceSettings {
        VoiceSettings {
            noise_suppression: false,
            ..VoiceSettings::default()
        }
    }

    /// Um tom de 220 Hz, como se alguém falasse no microfone.
    fn tone(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    /// Motor sem dispositivos que guarda os pacotes (seq, pico) e os eventos.
    /// Pacotes que o microfone mandou: (seq, pico).
    type Sent = Arc<Mutex<Vec<(u32, f32)>>>;

    fn engine() -> (VoiceEngine, Sent, Events) {
        let packets: Sent = Arc::default();
        let sink = packets.clone();
        let events: Events = Arc::default();
        let event_sink = events.clone();
        let engine = VoiceEngine::without_devices(
            sem_supressao(),
            Arc::new(move |seq, peak, opus| {
                assert!(!opus.is_empty());
                lock(&sink).push((seq, peak));
            }),
            move |e| lock(&event_sink).push(e),
        );
        (engine, packets, events)
    }

    /// Entrega 1 s de tom ao microfone em blocos de 10 ms, como a captura faz.
    fn speak(pipeline: &mut MicPipeline, ms: usize) {
        let audio = tone(48 * ms);
        for block in audio.chunks(480) {
            pipeline.process(block);
        }
    }

    #[test]
    fn microfone_manda_pacotes_numerados_e_avisa_que_estamos_falando() {
        let (engine, packets, events) = engine();
        let mut mic = engine.mic_pipeline();
        speak(&mut mic, 1000);
        let packets = lock(&packets).clone();
        assert!(packets.len() >= 40, "{} pacotes em 1 s", packets.len());
        let seqs: Vec<u32> = packets.iter().map(|p| p.0).collect();
        assert_eq!(seqs, (0..packets.len() as u32).collect::<Vec<_>>());
        let events = lock(&events).clone();
        assert!(events.contains(&VoiceEvent::Speaking {
            member: None,
            speaking: true
        }));
        let levels = events.iter().filter(|e| matches!(e, VoiceEvent::InputLevel(_))).count();
        assert!((4..=5).contains(&levels), "{levels} medições do nível em 1 s");
    }

    #[test]
    fn mudo_nao_manda_nada_e_para_de_aparecer_falando() {
        let (engine, packets, events) = engine();
        let mut mic = engine.mic_pipeline();
        speak(&mut mic, 300);
        engine.set_muted(true);
        let sent = lock(&packets).len();
        lock(&events).clear();
        speak(&mut mic, 300);
        assert_eq!(lock(&packets).len(), sent, "mudo não manda pacote");
        assert_eq!(
            *lock(&events),
            vec![VoiceEvent::Speaking {
                member: None,
                speaking: false
            }],
            "avisa uma vez só que parou de falar"
        );
        engine.set_muted(false);
        speak(&mut mic, 300);
        assert!(lock(&packets).len() > sent, "volta a mandar ao desmutar");
    }

    #[test]
    fn surdo_tambem_cala_o_microfone() {
        let (engine, packets, _) = engine();
        let mut mic = engine.mic_pipeline();
        engine.set_deafened(true);
        speak(&mut mic, 300);
        assert!(lock(&packets).is_empty());
        engine.set_deafened(false);
        speak(&mut mic, 300);
        assert!(!lock(&packets).is_empty());
    }

    #[test]
    fn amigos_entram_e_saem_da_voz_sem_duplicar() {
        let (engine, _, events) = engine();
        engine.add_peer("ana").unwrap();
        engine.add_peer("ana").unwrap();
        engine.add_peer("beto").unwrap();
        let mut peers = engine.peers();
        peers.sort();
        assert_eq!(peers, ["ana", "beto"]);
        // Quem não estava falando sai sem aviso de "parou de falar".
        engine.remove_peer("beto");
        engine.remove_peer("ninguem");
        assert_eq!(engine.peers(), ["ana"]);
        assert!(lock(&events).is_empty());
        assert!(format!("{engine:?}").starts_with("VoiceEngine"));
    }

    #[test]
    fn quem_sai_falando_deixa_de_aparecer_falando() {
        let (events, on_event) = recorder();
        let shared = Shared::new(VoiceSettings::default(), on_event);
        shared.add_peer("ana").unwrap();
        lock(&shared.peers).get_mut("ana").unwrap().speaking = true;
        shared.remove_peer("ana");
        assert_eq!(
            *lock(&events),
            vec![VoiceEvent::Speaking {
                member: Some("ana".into()),
                speaking: false
            }]
        );
    }

    #[test]
    fn volume_e_mudo_por_pessoa_so_valem_para_quem_esta_na_voz() {
        let (_, on_event) = recorder();
        let shared = Shared::new(VoiceSettings::default(), on_event);
        shared.add_peer("ana").unwrap();
        // Acima de 200% fica em 200%, que pela curva perceptiva é ×4 (+12 dB).
        shared.set_peer_volume("ana", 5.0);
        assert_eq!(shared.peers.lock().unwrap()["ana"].volume, 4.0);
        shared.set_peer_volume("ana", -1.0);
        assert_eq!(shared.peers.lock().unwrap()["ana"].volume, 0.0);
        shared.set_peer_muted("ana", true);
        assert!(shared.peers.lock().unwrap()["ana"].muted);
        // Quem não está na voz: nada acontece (nem entra na lista).
        shared.set_peer_volume("beto", 1.0);
        shared.set_peer_muted("beto", true);
        shared.submit("beto", 0, 1.0, &[1, 2, 3], Instant::now());
        assert_eq!(shared.peers(), ["ana"]);
    }

    #[test]
    fn so_pacote_com_som_conta_como_fala() {
        let (_, on_event) = recorder();
        let shared = Shared::new(VoiceSettings::default(), on_event);
        shared.add_peer("ana").unwrap();
        let now = Instant::now();
        shared.submit("ana", 0, SPEAKING_PEAK / 2.0, &[1], now);
        assert!(lock(&shared.peers)["ana"].last_voice.is_none(), "respiração não conta");
        shared.submit("ana", 1, SPEAKING_PEAK, &[1], now);
        assert_eq!(lock(&shared.peers)["ana"].last_voice, Some(now));
        assert_eq!(lock(&shared.peers)["ana"].jitter.len(), 2);
    }

    #[test]
    fn trocar_dispositivo_reabre_so_o_que_mudou() {
        let (_, on_event) = recorder();
        let shared = Shared::new(VoiceSettings::default(), on_event);
        let mut next = VoiceSettings {
            input_gain: 1.5,
            output_volume: 0.5,
            ..VoiceSettings::default()
        };
        assert!(
            !shared.replace_settings(next.clone()),
            "só volume: não reabre o microfone"
        );
        assert!(!shared.reopen_output.load(Ordering::Relaxed), "nem a saída");
        assert_eq!(lock(&shared.settings).output_volume, 0.5);

        next.output_device = Some("fone".into());
        assert!(!shared.replace_settings(next.clone()));
        assert!(
            shared.reopen_output.swap(false, Ordering::Relaxed),
            "saída nova: o mixer reabre"
        );

        next.input_device = Some("microfone usb".into());
        assert!(shared.replace_settings(next.clone()), "microfone novo: reabre");
        assert!(!shared.reopen_output.load(Ordering::Relaxed));
    }

    #[test]
    fn trocar_ganho_e_supressao_sem_trocar_microfone_nao_abre_dispositivo() {
        let (engine, packets, events) = engine();
        engine.set_settings(VoiceSettings {
            input_gain: 0.0,
            ..sem_supressao()
        });
        // Ganho zero: o microfone fica em silêncio, o portão não abre e nada sai.
        let mut mic = engine.mic_pipeline();
        speak(&mut mic, 300);
        assert!(lock(&packets).iter().all(|(_, peak)| *peak == 0.0));
        assert!(!lock(&events).iter().any(|e| matches!(e, VoiceEvent::MicFailed(_))));
    }

    fn render(shared: &Shared) -> Vec<f32> {
        let mut mix = vec![0.0; VOICE_FRAME];
        let mut decoded = vec![0.0; VOICE_FRAME];
        let mut stereo = vec![0.0; VOICE_FRAME * 2];
        let mut top = vec![0.0f32; 2];
        for _ in 0..9 {
            render_output(shared, &mut mix, &mut decoded, &mut stereo, Instant::now());
            let peak = sv_media::opus::peak(&stereo);
            if peak > top[0] {
                top[0] = peak;
                // Os dois canais saem iguais.
                top[1] = stereo.chunks_exact(2).map(|p| (p[0] - p[1]).abs()).fold(0.0, f32::max);
            }
        }
        top
    }

    fn shared_with_voice(settings: VoiceSettings) -> (Shared, Events) {
        let (events, on_event) = recorder();
        let shared = Shared::new(settings, on_event);
        shared.add_peer("ana").unwrap();
        let mut peers = lock(&shared.peers);
        let ana = peers.get_mut("ana").unwrap();
        feed(ana, &packets(10));
        drop(peers);
        (shared, events)
    }

    #[test]
    fn saida_toca_a_voz_em_estereo_e_avisa_quem_fala() {
        let (shared, events) = shared_with_voice(VoiceSettings::default());
        let [peak, diff] = render(&shared)[..] else {
            unreachable!()
        };
        assert!(peak > 0.1, "pico {peak}");
        assert_eq!(diff, 0.0);
        assert_eq!(
            lock(&events)[0],
            VoiceEvent::Speaking {
                member: Some("ana".into()),
                speaking: true
            }
        );
    }

    #[test]
    fn surdo_nao_ouve_ninguem() {
        let (shared, _) = shared_with_voice(VoiceSettings::default());
        shared.deafened.store(true, Ordering::Relaxed);
        assert_eq!(render(&shared)[0], 0.0);
    }

    #[test]
    fn volume_geral_baixa_a_saida_e_nunca_passa_do_limite() {
        let normal = render(&shared_with_voice(VoiceSettings::default()).0)[0];
        let low = render(
            &shared_with_voice(VoiceSettings {
                output_volume: 0.25,
                ..VoiceSettings::default()
            })
            .0,
        )[0];
        assert!(low > 0.0 && low < normal * 0.5, "normal {normal}, baixo {low}");
        let loud = render(
            &shared_with_voice(VoiceSettings {
                output_volume: 50.0,
                ..VoiceSettings::default()
            })
            .0,
        )[0];
        assert!(loud <= 1.0, "a saída é limitada a ±1 ({loud})");
    }

    #[test]
    fn pacote_perdido_no_meio_da_fala_e_disfarcado() {
        let mut peers = HashMap::from([("ana".to_string(), peer())]);
        let p = peers.get_mut("ana").unwrap();
        for (i, pk) in packets(6).into_iter().enumerate() {
            if i != 2 {
                p.jitter.push(i as u32, pk);
            }
        }
        p.last_voice = Some(Instant::now());
        // O buraco vira som disfarçado (PLC), sem silêncio nem erro.
        assert!(mix_level(&mut peers, 5) > 0.05);
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
