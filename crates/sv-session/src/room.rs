//! Sala: junta a sinalização (quem está, quem está ao vivo) e a rede P2P (quem assiste quem).
//!
//! Fluxo para assistir alguém (ver `docs/protocolo-malha.md`):
//! 1. quem quer assistir manda `WatchRequest` com seus endereços e sua fingerprint;
//! 2. quem transmite autoriza essa fingerprint, "fura" o NAT na direção dele e responde
//!    `WatchAccept` com os próprios endereços, fingerprint e o formato do vídeo;
//! 3. quem quer assistir conecta (QUIC) e começa a receber.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use quinn::Connection;
use sv_encode::EncodedFrame;
use sv_net::protocol::{FrameHeader, Signal, StreamDescription};
use sv_net::signaling::error_message;
use sv_net::{
    Broadcaster, DiscordUser, JoinRequester, Member, Peer, RoomClient, RoomEvent as WireEvent, SpeakingEffect,
    VideoFrame, stun,
};
use tokio::runtime::Runtime;

use crate::surface::lock;
use crate::voice_mesh::VoiceMesh;
use crate::{AudioPacket, KeyframeHandle, RelaySource, StreamInfo, StreamTap, VoiceEngine, VoiceEvent, VoiceSettings};

/// O que a interface precisa saber da sala.
pub enum RoomUpdate {
    /// Lista completa de membros (nós primeiro).
    Members(Vec<Member>),
    /// Nome da sala (`None`: sem nome; a interface mostra o código).
    RoomName(Option<String>),
    /// Algo da voz: quem está falando, nível do microfone, falha de dispositivo.
    Voice(VoiceEvent),
    /// Com quem a voz está ligada (mudou: alguém conectou ou caiu).
    VoicePeers(Vec<String>),
    /// Com quem a voz passa por outro membro (ADR 0020): membro → quem repassa.
    VoiceBridges(BTreeMap<String, String>),
    /// Não deu para ligar a voz com `member_id` (sem conexão direta).
    VoiceFailed { member_id: String, reason: String },
    /// A conexão com quem pedimos para assistir abriu, pelo caminho `path`.
    WatchReady {
        member: Member,
        conn: Connection,
        desc: StreamDescription,
        path: WatchPath,
    },
    /// O repassador TURN desta sala ficou pronto (`Ok`, com o nosso endereço nele) ou falhou.
    Relay(Result<std::net::SocketAddr, String>),
    /// Não deu para assistir (recusado ou sem conexão direta).
    WatchFailed { member_id: String, reason: String },
    /// Um amigo de alguém da sala pediu para entrar.
    JoinRequest { id: String, from: JoinRequester },
    /// O pedido de entrada foi respondido ou venceu.
    JoinRequestClosed(String),
    /// Erro da sala (cheia, versão diferente...).
    Error(String),
    /// A conexão com a sinalização caiu (`reason`, para o registro); tentando voltar para a sala.
    Reconnecting { reason: String },
    /// Voltamos para a sala depois de uma queda. `resumed`: com o mesmo id, sem ninguém ver a
    /// gente sair (ADR 0022); `false` se a sala não guardou a vaga e entramos como membro novo.
    Reconnected { resumed: bool },
    /// Perdemos a conexão com a sinalização e não deu para voltar.
    Disconnected,
}

/// Por onde chega o stream que assistimos (ADR 0014).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchPath {
    /// Direto de quem transmite (IPv4 ou IPv6).
    Direct,
    /// Pelo repassador TURN de quem transmite.
    Turn,
    /// Repassado por outro membro da sala que já assiste (o id dele).
    Member(String),
}

impl WatchPath {
    /// Para o registro de desempenho.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Direct => "direto",
            Self::Turn => "repassador",
            Self::Member(_) => "membro",
        }
    }
}

/// Pedido de repasse em andamento: quem ajuda agora e quem já tentamos.
struct PendingRelay {
    streamer: String,
    helper: String,
    tried: Vec<String>,
    /// Quem ajuda já respondeu (estamos conectando): o prazo de resposta não vale mais.
    answered: bool,
    /// Por que o direto falhou (é o que aparece se ninguém puder repassar).
    reason: String,
}

/// Pedido de repasse da voz em andamento (ADR 0020): quem ajuda agora e quem já tentamos.
struct PendingVoiceRelay {
    helper: String,
    tried: Vec<String>,
    /// Por que a voz direta falhou (é o que aparece se ninguém puder repassar).
    reason: String,
}

/// Quanto esperar a resposta de quem pode repassar antes de tentar o próximo.
const RELAY_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);
/// Quanto esperar quem pode repassar a voz antes de tentar o próximo.
const VOICE_RELAY_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
/// A voz repassada caiu: espera um pouco antes de pedir outra ponte (quem ajudava pode ter só
/// reconectado).
const VOICE_RELAY_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
/// Quantas pessoas cada membro repassa no máximo (é o upload dele que paga).
const MAX_RELAYED: usize = 3;

impl std::fmt::Debug for RoomUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Members(m) => f.debug_tuple("Members").field(m).finish(),
            Self::RoomName(n) => f.debug_tuple("RoomName").field(n).finish(),
            Self::Voice(e) => f.debug_tuple("Voice").field(e).finish(),
            Self::VoicePeers(p) => f.debug_tuple("VoicePeers").field(p).finish(),
            Self::VoiceBridges(b) => f.debug_tuple("VoiceBridges").field(b).finish(),
            Self::VoiceFailed { member_id, reason } => f
                .debug_struct("VoiceFailed")
                .field("member_id", member_id)
                .field("reason", reason)
                .finish(),
            Self::WatchReady { member, path, .. } => f
                .debug_struct("WatchReady")
                .field("member", member)
                .field("path", path)
                .finish(),
            Self::Relay(r) => f.debug_tuple("Relay").field(r).finish(),
            Self::WatchFailed { member_id, reason } => f
                .debug_struct("WatchFailed")
                .field("member_id", member_id)
                .field("reason", reason)
                .finish(),
            Self::JoinRequest { id, from } => f
                .debug_struct("JoinRequest")
                .field("id", id)
                .field("from", from)
                .finish(),
            Self::JoinRequestClosed(id) => f.debug_tuple("JoinRequestClosed").field(id).finish(),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
            Self::Reconnecting { reason } => f.debug_struct("Reconnecting").field("reason", reason).finish(),
            Self::Reconnected { resumed } => f.debug_struct("Reconnected").field("resumed", resumed).finish(),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}

type OnUpdate = Arc<dyn Fn(RoomUpdate) + Send + Sync>;

/// Quanto esperar a resposta de quem transmite a um pedido para assistir.
const WATCH_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);
/// Quanto um pedido para assistir espera o primeiro quadro do encoder, quando chega logo depois de
/// começarmos a transmitir (menos que o `WATCH_ANSWER_TIMEOUT` de quem pediu, para ele receber o
/// motivo em vez de "não respondeu").
const FIRST_FRAME_WAIT: std::time::Duration = std::time::Duration::from_secs(8);
/// Intervalos entre as tentativas de voltar para a sala depois de uma queda (~2 min no total). A
/// primeira é na hora: a maioria das quedas é só a conexão com a sala, com a internet funcionando,
/// e a sala guarda a nossa vaga por pouco tempo (ADR 0022).
const RECONNECT_DELAYS_S: [u64; 9] = [0, 1, 2, 4, 8, 15, 30, 30, 30];

struct Inner {
    /// Trocado quando a sala reconecta depois de uma queda.
    client: Mutex<RoomClient>,
    signaling_url: String,
    /// Sessão do SquadView, para entrar de novo como conferido ao reconectar.
    session: Mutex<Option<String>>,
    /// Token do último `welcome`: volta para a mesma vaga se a sinalização cair (ADR 0022).
    resume: Mutex<Option<String>>,
    code: String,
    peer: Peer,
    me: Mutex<Member>,
    room_name: Mutex<Option<String>>,
    members: Mutex<BTreeMap<String, Member>>,
    broadcaster: Broadcaster,
    /// Formato do que estamos transmitindo (None: não estamos ao vivo ou o encoder ainda não começou).
    stream: Mutex<Option<StreamDescription>>,
    keyframes: Mutex<Option<KeyframeHandle>>,
    /// Quem pedimos para assistir, esperando resposta (várias lives ao mesmo tempo, ADR 0019).
    pending_watch: Mutex<std::collections::HashSet<String>>,
    /// Sem conexão com quem transmite: pedindo para outro membro repassar (por streamer).
    pending_relay: Mutex<HashMap<String, PendingRelay>>,
    /// Os streams que assistimos, por streamer, para repassar a quem pedir (quem não alcança
    /// quem transmite).
    relay_sources: Mutex<HashMap<String, RelaySource>>,
    /// Fingerprint de quem vai conectar para receber repassado por nós → de qual streamer.
    relay_viewers: Mutex<HashMap<String, String>>,
    /// A voz com estes membros não abriu: pedindo a alguém para repassar (ADR 0020).
    pending_voice_relay: Mutex<HashMap<String, PendingVoiceRelay>>,
    /// Pedidos para assistir que chegaram com o "AO VIVO" já anunciado, mas antes do primeiro
    /// quadro do encoder: são respondidos quando ele sai (ou recusados depois de `FIRST_FRAME_WAIT`).
    first_frame_waiters: Mutex<Vec<WatchWaiter>>,
    voice: Arc<VoiceMesh>,
    runtime: tokio::runtime::Handle,
    on_update: OnUpdate,
}

/// Uma sala em que entramos. Sai da sala ao ser descartada.
pub struct Room {
    runtime: Arc<Runtime>,
    inner: Arc<Inner>,
    code: String,
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room").field("code", &self.code).finish_non_exhaustive()
    }
}

/// Quem entra na sala.
#[derive(Debug, Clone, Default)]
pub struct JoinAs {
    pub name: String,
    /// Conta do Discord vinculada (para a foto).
    pub discord: Option<DiscordUser>,
    /// Sessão do SquadView (ADR 0011): a sala confere e passa a saber quem somos de verdade.
    pub session: Option<String>,
    /// Pode criar a sala (criando uma, ou numa sala fixa de servidor). `false` (entrando por um
    /// código ou convite): se a sala não existe, a entrada falha com "Código inválido".
    pub create: bool,
}

impl JoinAs {
    /// Só o nome (sem Discord), como nos testes: cria a sala se ela ainda não existe.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            create: true,
            ..Self::default()
        }
    }
}

impl Room {
    /// Entra na sala `code` em `signaling_url`. Bloqueia até a sala responder (ou falhar).
    pub fn join(
        signaling_url: &str,
        code: &str,
        me: JoinAs,
        room_name: Option<&str>,
        voice: Option<VoiceSettings>,
        on_update: impl Fn(RoomUpdate) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let JoinAs {
            name,
            discord,
            session,
            create,
        } = me;
        let name = name.as_str();
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("sv-net")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?,
        );
        let on_update: OnUpdate = Arc::new(on_update);
        let voice_update = on_update.clone();
        let voice = Arc::new(VoiceMesh::start(voice, move |e| voice_update(RoomUpdate::Voice(e))));
        let (inner, events) = runtime.block_on(async {
            let peer = Peer::bind(stun::DEFAULT_SERVERS)
                .await
                .map_err(|e| format!("rede indisponível: {e}"))?;
            let (client, mut events) = RoomClient::join(
                signaling_url,
                code,
                name,
                discord.as_ref(),
                session.as_deref(),
                room_name,
                None,
                create,
            )
            .await?;
            // A primeira resposta diz se entramos.
            let me = match events.recv().await {
                Some(WireEvent::Joined {
                    you,
                    members,
                    room,
                    resume,
                }) => (you, members, room, resume),
                Some(WireEvent::Error(code)) => return Err(error_message(&code).to_owned()),
                _ => return Err("o serviço de salas não respondeu".to_owned()),
            };
            let (you, members, room, resume) = me;
            let inner = Arc::new(Inner {
                client: Mutex::new(client),
                signaling_url: signaling_url.to_owned(),
                session: Mutex::new(session.clone()),
                resume: Mutex::new(resume),
                code: code.to_owned(),
                peer,
                me: Mutex::new(Member {
                    id: you,
                    name: name.to_owned(),
                    live: false,
                    muted: false,
                    deafened: false,
                    discord,
                    verified: false,
                    watching: None,
                    watching_all: Vec::new(),
                    effect: None,
                }),
                room_name: Mutex::new(room),
                members: Mutex::new(members.into_iter().map(|m| (m.id.clone(), m)).collect()),
                broadcaster: Broadcaster::new(),
                stream: Mutex::new(None),
                keyframes: Mutex::new(None),
                pending_watch: Mutex::default(),
                pending_relay: Mutex::default(),
                relay_sources: Mutex::default(),
                relay_viewers: Mutex::default(),
                pending_voice_relay: Mutex::new(HashMap::new()),
                first_frame_waiters: Mutex::default(),
                voice,
                runtime: tokio::runtime::Handle::current(),
                on_update: on_update.clone(),
            });
            Ok::<_, String>((inner, events))
        })?;

        inner.publish_members();
        (inner.on_update)(RoomUpdate::RoomName(lock(&inner.room_name).clone()));
        // O repasse da voz mudou: atualiza quem está na voz e, se caiu, pede outra ponte (sem
        // prender a sala viva).
        let weak = Arc::downgrade(&inner);
        inner.voice.on_relay_change(move |lost| {
            if let Some(inner) = weak.upgrade() {
                inner.voice_relay_changed(lost);
            }
        });
        inner.offer_voice_to_all();
        runtime.spawn(run_session(inner.clone(), events));
        runtime.spawn(accept_viewers(inner.clone()));
        Ok(Self {
            runtime,
            inner,
            code: code.to_owned(),
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// Nome da sala, se ela tiver um.
    pub fn room_name(&self) -> Option<String> {
        lock(&self.inner.room_name).clone()
    }

    /// Renomeia a sala para todos. A confirmação volta como [`RoomUpdate::RoomName`].
    pub fn rename_room(&self, name: &str) {
        lock(&self.inner.client).set_room_name(name);
    }

    /// Entramos com o Discord já dentro da sala: a sala confere a sessão (e lembra dela para as
    /// reconexões).
    pub fn verify(&self, session: &str) {
        *lock(&self.inner.session) = Some(session.to_owned());
        lock(&self.inner.client).verify(session);
    }

    /// Aceita ou recusa o pedido de entrada `id` (o primeiro membro que responder decide).
    pub fn answer_join(&self, id: &str, accept: bool) {
        lock(&self.inner.client).answer_join(id, accept);
    }

    /// Troca o nosso apelido na sala.
    pub fn set_name(&self, name: &str) {
        lock(&self.inner.me).name = name.to_owned();
        lock(&self.inner.client).set_name(name);
        self.inner.publish_members();
    }

    /// Troca o nome e a conta do Discord (a pessoa vinculou ou desvinculou já na sala).
    pub fn set_profile(&self, name: &str, discord: Option<DiscordUser>) {
        lock(&self.inner.client).set_profile(name, discord.as_ref());
        let mut me = lock(&self.inner.me);
        me.name = name.to_owned();
        me.discord = discord;
        drop(me);
        self.inner.publish_members();
    }

    /// Runtime da rede: [`crate::RemoteView::start`] precisa rodar dentro dele.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// O que a prévia deve entregar para a sala mandar aos amigos.
    pub fn tap(&self) -> Arc<dyn StreamTap> {
        Arc::new(BroadcastTap {
            inner: self.inner.clone(),
        })
    }

    /// Começamos a transmitir: a sala fica sabendo e passamos a aceitar quem pedir para assistir.
    pub fn stream_started(&self, keyframes: KeyframeHandle) {
        *lock(&self.inner.keyframes) = Some(keyframes);
        lock(&self.inner.me).live = true;
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Paramos de transmitir: desconecta quem assistia (e recusa quem esperava o primeiro quadro).
    pub fn stream_stopped(&self) {
        *lock(&self.inner.keyframes) = None;
        *lock(&self.inner.stream) = None;
        self.inner.flush_waiters();
        self.inner.broadcaster.close();
        lock(&self.inner.me).live = false;
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Pede para assistir `member_id`. A resposta chega como [`RoomUpdate::WatchReady`] ou
    /// [`RoomUpdate::WatchFailed`].
    pub fn watch(&self, member_id: &str) {
        lock(&self.inner.pending_watch).insert(member_id.to_owned());
        lock(&self.inner.pending_relay).remove(member_id);
        // Sem resposta a tempo (quem transmite saiu, caiu ou tem um app antigo): desiste e avisa.
        let inner = self.inner.clone();
        let id = member_id.to_owned();
        self.runtime.spawn(async move {
            tokio::time::sleep(WATCH_ANSWER_TIMEOUT).await;
            let expired = lock(&inner.pending_watch).remove(&id);
            if expired {
                (inner.on_update)(RoomUpdate::WatchFailed {
                    member_id: id,
                    reason: "quem transmite não respondeu. Tente de novo em instantes.".into(),
                });
            }
        });
        let signal = Signal::WatchRequest {
            candidates: self.inner.peer.candidates().to_vec(),
            fingerprint: self.inner.peer.fingerprint().to_owned(),
        };
        lock(&self.inner.client).signal(member_id, &signal);
    }

    /// Como o nosso roteador mapeia portas (para o registro e para explicar falhas de conexão).
    pub fn nat(&self) -> sv_net::NatMapping {
        self.inner.peer.nat()
    }

    /// O motor de voz (`None`: a sala entrou sem voz ou o áudio falhou).
    pub fn voice(&self) -> Option<&VoiceEngine> {
        self.inner.voice.engine()
    }

    /// Silencia o nosso microfone para todos (e avisa a sala, para aparecer o ícone).
    pub fn set_muted(&self, muted: bool) {
        if let Some(v) = self.voice() {
            v.set_muted(muted);
        }
        lock(&self.inner.me).muted = muted;
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Ensurdece: não ouvimos ninguém e o microfone também fica mudo.
    pub fn set_deafened(&self, deafened: bool) {
        if let Some(v) = self.voice() {
            v.set_deafened(deafened);
        }
        lock(&self.inner.me).deafened = deafened;
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Troca o nosso efeito de fala (`None`: o anel padrão): a sala inteira passa a vê-lo.
    pub fn set_effect(&self, effect: Option<SpeakingEffect>) {
        let mut me = lock(&self.inner.me);
        if me.effect == effect {
            return;
        }
        me.effect = effect;
        drop(me);
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Quem estamos assistindo agora (todas as lives, ADR 0019): a sala fica sabendo, para quem
    /// transmite ver quantos e quem assistem (inclusive quem recebe repassado por outro membro).
    pub fn set_watching(&self, member_ids: &[String]) {
        let mut me = lock(&self.inner.me);
        if me.watching_all == member_ids {
            return;
        }
        me.watching = member_ids.first().cloned();
        me.watching_all = member_ids.to_vec();
        drop(me);
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Começamos a assistir `source.streamer`: a sala pode repassar esse stream a quem não alcança
    /// quem transmite (ADR 0014).
    pub fn add_relay_source(&self, source: RelaySource) {
        lock(&self.inner.relay_sources).insert(source.streamer.clone(), source);
    }

    /// Paramos de assistir `streamer`: quem ia receber esse stream por nós não recebe mais.
    pub fn remove_relay_source(&self, streamer: &str) {
        lock(&self.inner.relay_sources).remove(streamer);
        lock(&self.inner.relay_viewers).retain(|_, s| s != streamer);
    }

    /// Quantos amigos estão assistindo a gente.
    pub fn viewer_count(&self) -> usize {
        self.inner.broadcaster.viewer_count()
    }
}

/// Quanto esperar o `leave` sair ao deixar a sala.
const LEAVE_FLUSH: std::time::Duration = std::time::Duration::from_millis(500);

impl Drop for Room {
    fn drop(&mut self) {
        // Antes de o runtime parar: sem o `leave`, a sala guardaria a nossa vaga por 60 s, como se
        // a gente tivesse caído (ADR 0022).
        lock(&self.inner.client).leave(LEAVE_FLUSH);
        self.inner.broadcaster.close();
        self.inner.voice.close();
        self.inner.peer.close();
    }
}

/// `member` assiste `streamer`: pela lista nova (ADR 0019) ou pelo campo antigo (apps até o 0.8.0).
fn watches(member: &Member, streamer: &str) -> bool {
    member.watching_all.iter().any(|id| id == streamer) || member.watching.as_deref() == Some(streamer)
}

impl Inner {
    /// Responde um pedido para assistir. Com o "AO VIVO" já anunciado e o encoder ainda sem o
    /// primeiro quadro (uma janela parada pode demorar a mandar um), o pedido espera esse quadro em
    /// vez de ser recusado: antes, quem clicava nesse intervalo via "Não está transmitindo agora."
    /// com o selo de ao vivo aceso.
    fn answer_watch(self: &Arc<Self>, request: WatchWaiter) {
        let live = lock(&self.keyframes).is_some();
        let desc = *lock(&self.stream);
        let reply = match (live, desc) {
            (true, Some(stream)) => {
                self.peer.allowed().allow(&request.fingerprint);
                self.peer.punch(&request.candidates);
                // Se o direto não fechar, a pessoa vem pelo nosso endereço no repassador.
                self.peer.permit_relay(&request.candidates);
                Signal::WatchAccept {
                    candidates: self.peer.candidates().to_vec(),
                    fingerprint: self.peer.fingerprint().to_owned(),
                    stream,
                    relay: self.peer.relay_addr(),
                }
            }
            (true, None) => {
                let key = (request.from.clone(), request.fingerprint.clone());
                lock(&self.first_frame_waiters).push(request);
                let weak = Arc::downgrade(self);
                self.runtime.spawn(async move {
                    tokio::time::sleep(FIRST_FRAME_WAIT).await;
                    if let Some(inner) = weak.upgrade() {
                        inner.give_up_waiter(&key.0, &key.1);
                    }
                });
                return;
            }
            (false, _) => Signal::WatchDeny {
                reason: NOT_LIVE.into(),
            },
        };
        lock(&self.client).signal(&request.from, &reply);
    }

    /// O primeiro quadro não saiu a tempo: recusa o pedido que ainda esperava.
    fn give_up_waiter(&self, from: &str, fingerprint: &str) {
        let mut waiters = lock(&self.first_frame_waiters);
        let Some(i) = waiters
            .iter()
            .position(|w| w.from == from && w.fingerprint == fingerprint)
        else {
            return;
        };
        let waiter = waiters.remove(i);
        drop(waiters);
        lock(&self.client).signal(
            &waiter.from,
            &Signal::WatchDeny {
                reason: STILL_STARTING.into(),
            },
        );
    }

    /// O primeiro quadro saiu (ou a transmissão parou): responde quem esperava.
    fn flush_waiters(self: &Arc<Self>) {
        // Chamado da thread do encoder (o primeiro quadro) ou da interface: aceitar usa o runtime.
        let _runtime = self.runtime.enter();
        let waiters = std::mem::take(&mut *lock(&self.first_frame_waiters));
        for waiter in waiters {
            self.answer_watch(waiter);
        }
    }

    /// Manda para a sala o nosso estado (ao vivo, mudo, surdo, quem assistimos e o efeito de fala).
    fn send_status(&self) {
        let me = lock(&self.me).clone();
        lock(&self.client).set_status(me.live, me.muted, me.deafened, &me.watching_all, me.effect.as_ref());
    }

    /// Chama para a voz quem a gente deve chamar (ver `VoiceMesh::should_offer`).
    fn offer_voice_to(&self, member: &str) {
        let me = lock(&self.me).id.clone();
        if self.voice.should_offer(&me, member) {
            let offer = Signal::VoiceOffer {
                candidates: self.peer.candidates().to_vec(),
                fingerprint: self.peer.fingerprint().to_owned(),
            };
            lock(&self.client).signal(member, &offer);
        }
    }

    fn offer_voice_to_all(&self) {
        self.voice.set_me(&lock(&self.me).id);
        let members: Vec<String> = lock(&self.members).keys().cloned().collect();
        for m in members {
            self.offer_voice_to(&m);
        }
    }

    fn publish_voice_peers(&self) {
        (self.on_update)(RoomUpdate::VoicePeers(self.voice.connected()));
        (self.on_update)(RoomUpdate::VoiceBridges(self.voice.bridges().into_iter().collect()));
    }

    fn publish_members(&self) {
        let me = lock(&self.me).clone();
        let mut list = vec![me];
        list.extend(lock(&self.members).values().cloned());
        (self.on_update)(RoomUpdate::Members(list));
    }

    fn member(&self, id: &str) -> Option<Member> {
        lock(&self.members).get(id).cloned()
    }

    /// O membro, ou um provisório se ele acabou de sair da lista.
    fn member_or_placeholder(&self, id: &str) -> Member {
        self.member(id).unwrap_or(Member {
            id: id.to_owned(),
            name: "?".into(),
            live: true,
            muted: false,
            deafened: false,
            discord: None,
            verified: false,
            watching: None,
            watching_all: Vec::new(),
            effect: None,
        })
    }

    /// Não deu para conectar em quem transmite: pede a um membro que já assiste para repassar
    /// (ADR 0014), um de cada vez. Sem ninguém (ou todos recusaram), avisa com `reason`.
    fn try_relay(self: &Arc<Self>, streamer: String, reason: String) {
        let me = lock(&self.me).id.clone();
        let mut pending = lock(&self.pending_relay);
        let mut tried = pending.remove(&streamer).map(|p| p.tried).unwrap_or_default();
        let helper = lock(&self.members)
            .values()
            .filter(|m| m.id != me && m.id != streamer && !tried.contains(&m.id))
            .find(|m| watches(m, &streamer))
            .map(|m| m.id.clone());
        let Some(helper) = helper else {
            drop(pending);
            (self.on_update)(RoomUpdate::WatchFailed {
                member_id: streamer,
                reason,
            });
            return;
        };
        tried.push(helper.clone());
        pending.insert(
            streamer.clone(),
            PendingRelay {
                streamer: streamer.clone(),
                helper: helper.clone(),
                tried,
                answered: false,
                reason,
            },
        );
        drop(pending);
        let request = Signal::RelayRequest {
            streamer,
            candidates: self.peer.candidates().to_vec(),
            fingerprint: self.peer.fingerprint().to_owned(),
        };
        lock(&self.client).signal(&helper, &request);
        // Sem resposta (app antigo, saiu): o próximo.
        let inner = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(RELAY_ANSWER_TIMEOUT).await;
            let next = lock(&inner.pending_relay)
                .values()
                .find(|p| p.helper == helper && !p.answered)
                .map(|p| (p.streamer.clone(), p.reason.clone()));
            if let Some((streamer, reason)) = next {
                inner.try_relay(streamer, reason);
            }
        });
    }

    /// A voz com `target` não abriu: pede a um membro que fala com os dois para repassar (ADR
    /// 0020), do menor ping para o maior. Sem ninguém, a falha chega à interface com `reason`.
    fn try_voice_relay(self: &Arc<Self>, target: String, reason: String) {
        let mut pending = lock(&self.pending_voice_relay);
        let mut tried = pending.remove(&target).map(|p| p.tried).unwrap_or_default();
        let helper = self.voice.helpers_for(&target).into_iter().find(|h| !tried.contains(h));
        let Some(helper) = helper else {
            drop(pending);
            (self.on_update)(RoomUpdate::VoiceFailed {
                member_id: target,
                reason,
            });
            return;
        };
        tried.push(helper.clone());
        pending.insert(
            target.clone(),
            PendingVoiceRelay {
                helper: helper.clone(),
                tried,
                reason,
            },
        );
        drop(pending);
        let request = Signal::VoiceRelayRequest { to: target.clone() };
        lock(&self.client).signal(&helper, &request);
        // Sem resposta (app antigo, saiu): o próximo.
        let inner = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(VOICE_RELAY_ANSWER_TIMEOUT).await;
            inner.next_voice_helper(&target, &helper);
        });
    }

    /// `helper` não vai repassar a voz com `target` (recusou ou não respondeu): tenta o próximo.
    fn next_voice_helper(self: &Arc<Self>, target: &str, helper: &str) {
        let reason = lock(&self.pending_voice_relay)
            .get(target)
            .filter(|p| p.helper == helper)
            .map(|p| p.reason.clone());
        if let Some(reason) = reason {
            self.try_voice_relay(target.to_owned(), reason);
        }
    }

    /// Como quem ajuda: abre a conexão de repasse com `member` pelo mesmo caminho da voz.
    fn open_voice_relay(self: &Arc<Self>, member: String) {
        if !self.voice.needs_relay_conn(&member) {
            return;
        }
        let Some((addr, fingerprint)) = self.voice.reach(&member) else {
            self.voice.relay_failed(&member);
            return;
        };
        let inner = self.clone();
        tokio::spawn(async move {
            match inner
                .peer
                .connect_for(&[addr], None, &fingerprint, sv_net::Purpose::VoiceRelay)
                .await
            {
                Ok(conn) => inner.voice.register_relay(&inner.runtime, member, conn),
                Err(_) => inner.voice.relay_failed(&member),
            }
        });
    }

    /// O repasse da voz mudou; a voz com cada `(membro, quem repassava)` de `lost` caiu. Quem
    /// tinha pedido a ponte (o id menor, que chama na voz) pede outra a outro membro (o mesmo
    /// ficaria caindo, como com uma ponta num app antigo), se a pessoa continua na sala e a voz
    /// com ela não voltou de outro jeito.
    fn voice_relay_changed(self: &Arc<Self>, lost: Vec<(String, String)>) {
        self.publish_voice_peers();
        let me = lock(&self.me).id.clone();
        for (peer, helper) in lost {
            if me >= peer {
                continue;
            }
            let inner = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(VOICE_RELAY_RETRY_AFTER).await;
                let in_room = lock(&inner.members).contains_key(&peer);
                if in_room && !inner.voice.connected().contains(&peer) {
                    let reason = "a ponte da voz caiu e ninguém mais pode repassar".to_owned();
                    lock(&inner.pending_voice_relay).insert(
                        peer.clone(),
                        PendingVoiceRelay {
                            helper: String::new(),
                            tried: vec![helper],
                            reason: reason.clone(),
                        },
                    );
                    inner.try_voice_relay(peer, reason);
                }
            });
        }
    }

    fn on_signal(self: &Arc<Self>, from: String, signal: Signal) {
        match signal {
            Signal::WatchRequest {
                candidates,
                fingerprint,
            } => self.answer_watch(WatchWaiter {
                from,
                candidates,
                fingerprint,
            }),
            Signal::WatchAccept {
                candidates,
                fingerprint,
                stream,
                relay,
            } => {
                let direct = lock(&self.pending_watch).contains(&from);
                // Ou é quem aceitou repassar um stream que pedimos.
                let helped = {
                    let mut pending = lock(&self.pending_relay);
                    pending.values_mut().find(|p| p.helper == from && !p.answered).map(|p| {
                        p.answered = true;
                        p.streamer.clone()
                    })
                };
                let streamer = match (direct, helped) {
                    (true, _) => from.clone(),
                    (false, Some(streamer)) => streamer,
                    (false, None) => return,
                };
                let inner = self.clone();
                tokio::spawn(async move {
                    let result = inner.peer.connect(&candidates, relay, &fingerprint).await;
                    if direct {
                        lock(&inner.pending_watch).remove(&streamer);
                    }
                    match result {
                        Ok(conn) => {
                            let path = if !direct {
                                lock(&inner.pending_relay).remove(&streamer);
                                WatchPath::Member(from)
                            } else if relay.is_some_and(|r| r == conn.remote_address()) {
                                WatchPath::Turn
                            } else {
                                WatchPath::Direct
                            };
                            (inner.on_update)(RoomUpdate::WatchReady {
                                member: inner.member_or_placeholder(&streamer),
                                conn,
                                desc: stream,
                                path,
                            });
                        }
                        // Sem caminho até quem transmite (nem pelo repassador): pede a quem já
                        // assiste para repassar; se era um ajudante, tenta o próximo.
                        Err(e) => {
                            let reason = if direct {
                                connect_failure(&e, inner.peer.nat(), relay.is_some())
                            } else {
                                lock(&inner.pending_relay)
                                    .get(&streamer)
                                    .map_or_else(|| e.to_string(), |p| p.reason.clone())
                            };
                            inner.try_relay(streamer, reason);
                        }
                    }
                });
            }
            Signal::RelayRequest {
                streamer,
                candidates,
                fingerprint,
            } => {
                let source = lock(&self.relay_sources)
                    .get(&streamer)
                    .cloned()
                    .filter(|s| s.broadcaster.viewer_count() < MAX_RELAYED);
                let reply = match source {
                    Some(source) => {
                        self.peer.allowed().allow(&fingerprint);
                        lock(&self.relay_viewers).insert(fingerprint, streamer);
                        self.peer.punch(&candidates);
                        self.peer.permit_relay(&candidates);
                        Signal::WatchAccept {
                            candidates: self.peer.candidates().to_vec(),
                            fingerprint: self.peer.fingerprint().to_owned(),
                            stream: source.desc,
                            relay: self.peer.relay_addr(),
                        }
                    }
                    None => Signal::WatchDeny {
                        reason: "Não dá para repassar agora.".into(),
                    },
                };
                lock(&self.client).signal(&from, &reply);
            }
            Signal::VoiceOffer {
                candidates,
                fingerprint,
            } => {
                if self.voice.engine().is_none() {
                    return;
                }
                self.peer.allowed().allow(&fingerprint);
                self.voice.expect(&from, &fingerprint);
                self.peer.punch(&candidates);
                self.peer.permit_relay(&candidates);
                let answer = Signal::VoiceAnswer {
                    candidates: self.peer.candidates().to_vec(),
                    fingerprint: self.peer.fingerprint().to_owned(),
                    relay: self.peer.relay_addr(),
                };
                lock(&self.client).signal(&from, &answer);
            }
            Signal::VoiceAnswer {
                candidates,
                fingerprint,
                relay,
            } => {
                if !self.voice.answered(&from) {
                    return;
                }
                let inner = self.clone();
                tokio::spawn(async move {
                    match inner
                        .peer
                        .connect_for(&candidates, relay, &fingerprint, sv_net::Purpose::Voice)
                        .await
                    {
                        Ok(conn) => {
                            // Pode abrir a conexão de repasse com a gente depois (ADR 0020).
                            inner.peer.allowed().allow(&fingerprint);
                            inner.voice.register(&inner.runtime, from, &fingerprint, conn);
                            inner.publish_voice_peers();
                        }
                        // Nem direto nem pelo TURN: pede a um membro que fala com os dois.
                        Err(e) => {
                            let reason = connect_failure(&e, inner.peer.nat(), relay.is_some());
                            inner.try_voice_relay(from, reason);
                        }
                    }
                });
            }
            // Somos quem ajuda: `from` não alcança `to` e pede para repassarmos a voz entre eles.
            Signal::VoiceRelayRequest { to } => {
                let known = lock(&self.members).contains_key(&to);
                let reply = match self.voice.add_relay_pair(&from, &to) {
                    Ok(()) if known => Signal::VoiceRelayAccept { to: to.clone() },
                    Ok(()) => Signal::VoiceRelayDeny {
                        to: to.clone(),
                        reason: "essa pessoa não está na sala".into(),
                    },
                    Err(reason) => Signal::VoiceRelayDeny { to: to.clone(), reason },
                };
                let accepted = matches!(reply, Signal::VoiceRelayAccept { .. });
                lock(&self.client).signal(&from, &reply);
                if accepted {
                    self.open_voice_relay(from);
                    self.open_voice_relay(to);
                }
            }
            // Quem pedimos aceitou: sorteia a chave do par e manda direto para a outra ponta.
            Signal::VoiceRelayAccept { to } => {
                let ours = lock(&self.pending_voice_relay)
                    .get(&to)
                    .is_some_and(|p| p.helper == from);
                if !ours {
                    return;
                }
                let Some(key) = sv_net::protocol::RelayKey::random() else {
                    self.next_voice_helper(&to, &from);
                    return;
                };
                if !self.voice.route(&to, &from, sv_net::protocol::RelayKey::new(&key)) {
                    self.next_voice_helper(&to, &from);
                    return;
                }
                lock(&self.pending_voice_relay).remove(&to);
                let share = Signal::VoiceRelayKey {
                    via: from,
                    key: sv_net::protocol::RelayKey::to_hex(&key),
                };
                lock(&self.client).signal(&to, &share);
                self.publish_voice_peers();
            }
            Signal::VoiceRelayDeny { to, .. } => self.next_voice_helper(&to, &from),
            // A outra ponta pediu a ponte: a voz dela vai chegar por `via`, assinada com `key`.
            Signal::VoiceRelayKey { via, key } => {
                let Some(key) = sv_net::protocol::RelayKey::from_hex(&key) else {
                    return;
                };
                if lock(&self.members).contains_key(&from) && self.voice.talks_to(&via) {
                    self.voice.route(&from, &via, key);
                    self.publish_voice_peers();
                }
            }
            Signal::WatchDeny { reason } => {
                if lock(&self.pending_watch).remove(&from) {
                    (self.on_update)(RoomUpdate::WatchFailed {
                        member_id: from,
                        reason,
                    });
                    return;
                }
                // Quem ia repassar não pode: o próximo.
                let next = lock(&self.pending_relay)
                    .values()
                    .find(|p| p.helper == from)
                    .map(|p| (p.streamer.clone(), p.reason.clone()));
                if let Some((streamer, reason)) = next {
                    self.try_relay(streamer, reason);
                }
            }
        }
    }
}

/// Explica por que não deu para conectar. Se o nosso NAT é simétrico, a causa é quase certa.
/// `relay_tried`: o outro lado mandou um endereço no repassador TURN e nem por ele fechou. Sem
/// repassador configurado, a mensagem não fala de um repassador que não existe.
fn connect_failure(error: &sv_net::ConnectError, nat: sv_net::NatMapping, relay_tried: bool) -> String {
    match (error, nat) {
        (sv_net::ConnectError::Unreachable, sv_net::NatMapping::Symmetric) if !relay_tried => {
            "não deu para conectar: a sua internet troca de porta a cada conexão (NAT simétrico, comum \
             em CGNAT de operadora e em 4G) e a do outro lado também não deixou a conexão direta \
             fechar. Ligar o IPv6 no roteador costuma resolver; ou tente por outra rede, como o celular."
                .into()
        }
        (sv_net::ConnectError::Unreachable, sv_net::NatMapping::Symmetric) => {
            "não deu para conectar: a sua internet troca de porta a cada conexão (NAT simétrico, comum \
             em CGNAT de operadora e em 4G) e o repassador da sala não respondeu. Tente de novo em \
             instantes, ou por outra rede, como o roteador de casa ou o celular."
                .into()
        }
        _ => error.to_string(),
    }
}

/// Trata os eventos da sala até a conexão cair. Devolve o motivo da queda.
async fn handle_events(inner: Arc<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<WireEvent>) -> String {
    while let Some(event) = events.recv().await {
        match event {
            WireEvent::Joined { .. } => {}
            WireEvent::RoomRenamed(name) => {
                *lock(&inner.room_name) = Some(name.clone());
                (inner.on_update)(RoomUpdate::RoomName(Some(name)));
            }
            WireEvent::MemberJoined(m) => {
                let id = m.id.clone();
                lock(&inner.members).insert(id.clone(), m);
                inner.publish_members();
                inner.offer_voice_to(&id);
            }
            WireEvent::MemberUpdated(m) => {
                lock(&inner.members).insert(m.id.clone(), m);
                inner.publish_members();
            }
            WireEvent::MemberLeft(id) => {
                lock(&inner.members).remove(&id);
                lock(&inner.pending_voice_relay).remove(&id);
                inner.voice.unregister(&id);
                inner.publish_members();
                inner.publish_voice_peers();
            }
            WireEvent::Signal { from, signal } => inner.on_signal(from, signal),
            WireEvent::JoinRequest { id, from } => (inner.on_update)(RoomUpdate::JoinRequest { id, from }),
            WireEvent::JoinRequestClosed(id) => (inner.on_update)(RoomUpdate::JoinRequestClosed(id)),
            WireEvent::Turn(credentials) => {
                // Uma vez por sala: ao reconectar a sinalização, o Worker manda de novo, mas trocar
                // o repassador derrubaria quem já está conectado por ele.
                if inner.peer.relay_addr().is_none() {
                    tokio::spawn(enable_relay(inner.clone(), credentials));
                }
            }
            WireEvent::Error(code) => (inner.on_update)(RoomUpdate::Error(error_message(&code).to_owned())),
            WireEvent::Disconnected(reason) => return reason,
        }
    }
    "o cliente da sala parou".into()
}

/// Pede o endereço no repassador TURN (ADR 0014). Só serve a quem não conecta direto: quem pede
/// para assistir ou chama para a voz tenta primeiro o caminho direto.
async fn enable_relay(inner: Arc<Inner>, credentials: sv_net::signaling::TurnCredentials) {
    // Resolver o nome do repassador consulta o DNS: fora das tarefas da rede.
    let server = tokio::task::spawn_blocking(move || credentials.server())
        .await
        .ok()
        .flatten();
    let result = match server {
        Some(server) => inner.peer.enable_relay(server).await.map_err(|e| e.to_string()),
        None => Err("nenhum endereço UDP do repassador".into()),
    };
    (inner.on_update)(RoomUpdate::Relay(result));
}

/// Ouve a sala; se a sinalização cair, volta para ela e continua ouvindo.
async fn run_session(inner: Arc<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<WireEvent>) {
    loop {
        let reason = handle_events(inner.clone(), events).await;
        match reconnect(&inner, reason).await {
            Some(next) => events = next,
            None => return,
        }
    }
}

/// A sinalização caiu: entra de novo na mesma sala, com o mesmo nome e, se a sala ainda guarda a
/// vaga, com o mesmo id (ADR 0022): para os outros, a gente nunca saiu. As conexões QUIC (voz, quem
/// assiste a gente e quem a gente assiste) não dependem dela e continuam. `None` se desistiu.
async fn reconnect(inner: &Arc<Inner>, reason: String) -> Option<tokio::sync::mpsc::UnboundedReceiver<WireEvent>> {
    (inner.on_update)(RoomUpdate::Reconnecting { reason });
    for delay in RECONNECT_DELAYS_S {
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        let (name, discord, live) = {
            let me = lock(&inner.me);
            (me.name.clone(), me.discord.clone(), me.live)
        };
        let room_name = lock(&inner.room_name).clone();
        // Copia antes do `await`: um lock não pode atravessar a espera.
        let session = lock(&inner.session).clone();
        let resume = lock(&inner.resume).clone().map(|token| sv_net::Resume {
            id: lock(&inner.me).id.clone(),
            token,
        });
        let Ok((client, mut events)) = RoomClient::join(
            &inner.signaling_url,
            &inner.code,
            &name,
            discord.as_ref(),
            session.as_deref(),
            room_name.as_deref(),
            resume.as_ref(),
            // Já estávamos na sala: se ela esvaziou (reinício), voltamos e ela continua existindo.
            true,
        )
        .await
        else {
            continue;
        };
        match events.recv().await {
            Some(WireEvent::Joined {
                you,
                members,
                room,
                resume: token,
            }) => {
                let resumed = resume.as_ref().is_some_and(|r| r.id == you);
                *lock(&inner.client) = client;
                *lock(&inner.resume) = token;
                lock(&inner.me).id = you;
                *lock(&inner.members) = members.into_iter().map(|m| (m.id.clone(), m)).collect();
                if room.is_some() {
                    *lock(&inner.room_name) = room;
                }
                let _ = live;
                inner.send_status();
                inner.publish_members();
                inner.offer_voice_to_all();
                (inner.on_update)(RoomUpdate::RoomName(lock(&inner.room_name).clone()));
                (inner.on_update)(RoomUpdate::Reconnected { resumed });
                return Some(events);
            }
            // Sala cheia ou versão diferente: não adianta insistir.
            Some(WireEvent::Error(code)) if code != "rate-limited" => break,
            _ => continue,
        }
    }
    (inner.on_update)(RoomUpdate::Disconnected);
    None
}

async fn accept_viewers(inner: Arc<Inner>) {
    while let Some(accepted) = inner.peer.accept().await {
        if accepted.purpose == sv_net::Purpose::Voice {
            // Voz: quem é vem da fingerprint que a pessoa mandou no voice-offer.
            let fingerprint = accepted.fingerprint.unwrap_or_default();
            match inner.voice.member_for(&fingerprint) {
                Some(member) => {
                    inner
                        .voice
                        .register(&inner.runtime, member, &fingerprint, accepted.conn);
                    inner.publish_voice_peers();
                }
                None => accepted.conn.close(quinn::VarInt::from_u32(2), b"desconhecido"),
            }
            continue;
        }
        if accepted.purpose == sv_net::Purpose::VoiceRelay {
            // Repasse de voz (ADR 0020): só de quem já está na voz com a gente.
            let member = accepted
                .fingerprint
                .as_deref()
                .and_then(|f| inner.voice.voice_member_for(f));
            match member {
                Some(member) => inner.voice.register_relay(&inner.runtime, member, accepted.conn),
                None => accepted.conn.close(quinn::VarInt::from_u32(2), b"desconhecido"),
            }
            continue;
        }
        let conn = accepted.conn;
        // Quem pediu repasse recebe o stream que estamos assistindo, não o nosso.
        let relayed = accepted
            .fingerprint
            .as_ref()
            .and_then(|f| lock(&inner.relay_viewers).remove(f));
        if let Some(streamer) = relayed {
            let source = lock(&inner.relay_sources).get(&streamer).cloned();
            match source {
                Some(source) => {
                    let keyframes = source.keyframes.clone();
                    source.broadcaster.add(conn, Arc::new(move || keyframes.request()));
                }
                None => conn.close(quinn::VarInt::from_u32(1), b"parou de assistir"),
            }
            continue;
        }
        let keyframes = lock(&inner.keyframes).clone();
        let Some(keyframes) = keyframes else {
            conn.close(quinn::VarInt::from_u32(1), b"nao esta transmitindo");
            continue;
        };
        inner.broadcaster.add(conn, Arc::new(move || keyframes.request()));
    }
}

/// Entrega o que a prévia produz para os amigos conectados.
/// Recusa de quem não está ao vivo.
const NOT_LIVE: &str = "Não está transmitindo agora.";
/// Recusa de quem está ao vivo, mas cujo encoder ainda não mandou o primeiro quadro.
const STILL_STARTING: &str = "A transmissão ainda está começando. Tente de novo em instantes.";

/// Um pedido para assistir, guardado até o primeiro quadro do encoder.
struct WatchWaiter {
    from: String,
    candidates: Vec<std::net::SocketAddr>,
    fingerprint: String,
}

struct BroadcastTap {
    inner: Arc<Inner>,
}

impl StreamTap for BroadcastTap {
    fn started(&self, info: &StreamInfo) {
        *lock(&self.inner.stream) = Some(StreamDescription {
            width: info.size.width,
            height: info.size.height,
            fps: info.fps,
            audio: true,
        });
        // Quem pediu para assistir antes do primeiro quadro entra agora.
        self.inner.flush_waiters();
    }

    fn video(&self, seq: u32, frame: &EncodedFrame) {
        if self.inner.broadcaster.viewer_count() == 0 {
            return;
        }
        self.inner.broadcaster.send_video(&VideoFrame {
            header: FrameHeader {
                seq,
                timestamp_us: frame.timestamp_us,
                keyframe: frame.keyframe,
                encode_us: frame.encode_us,
            },
            data: Arc::from(frame.data.as_slice()),
        });
    }

    fn audio(&self, packet: &AudioPacket) {
        if self.inner.broadcaster.viewer_count() > 0 {
            self.inner
                .broadcaster
                .send_audio(packet.seq, packet.timestamp_us, packet.peak, &packet.data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nat_simetrico_explica_a_causa_sem_espacos_sobrando() {
        let msg = connect_failure(&sv_net::ConnectError::Unreachable, sv_net::NatMapping::Symmetric, true);
        assert!(msg.contains("NAT simétrico, comum em CGNAT"), "{msg}");
        assert!(msg.contains("o repassador da sala não respondeu"), "{msg}");
        assert!(msg.contains("roteador de casa ou o celular"), "{msg}");
        assert!(!msg.contains("  "), "espaços duplicados na mensagem: {msg:?}");
    }

    #[test]
    fn sem_repassador_a_mensagem_nao_fala_de_repassador() {
        // Sala sem TURN configurado (o caso do Lobo e do Nino): antes, a mensagem dizia que "o
        // repassador da sala não respondeu", sem existir repassador.
        let msg = connect_failure(&sv_net::ConnectError::Unreachable, sv_net::NatMapping::Symmetric, false);
        assert!(msg.contains("NAT simétrico, comum em CGNAT"), "{msg}");
        assert!(!msg.contains("repassador"), "{msg}");
        assert!(msg.contains("IPv6"), "{msg}");
        assert!(!msg.contains("  "), "espaços duplicados na mensagem: {msg:?}");
    }

    #[test]
    fn outras_falhas_de_conexao_usam_a_mensagem_da_rede() {
        let unreachable = sv_net::ConnectError::Unreachable;
        for nat in [sv_net::NatMapping::Unknown, sv_net::NatMapping::EndpointIndependent] {
            assert_eq!(connect_failure(&unreachable, nat, false), unreachable.to_string());
        }
        let other = sv_net::ConnectError::Other("certificado".into());
        assert_eq!(
            connect_failure(&other, sv_net::NatMapping::Symmetric, true),
            "erro na conexão: certificado"
        );
    }

    #[test]
    fn entrar_so_com_o_nome_nao_leva_discord_nem_sessao() {
        let me = JoinAs::named("Ana");
        assert_eq!(me.name, "Ana");
        assert!(me.discord.is_none() && me.session.is_none());
    }

    #[test]
    fn atualizacoes_da_sala_aparecem_legiveis_no_registro() {
        let member = Member {
            id: "a1".into(),
            name: "Ana".into(),
            live: true,
            muted: false,
            deafened: false,
            discord: None,
            verified: false,
            watching: None,
            watching_all: Vec::new(),
            effect: None,
        };
        let cases: Vec<(RoomUpdate, &str)> = vec![
            (RoomUpdate::Members(vec![member.clone()]), "Members(["),
            (
                RoomUpdate::RoomName(Some("Galera".into())),
                "RoomName(Some(\"Galera\"))",
            ),
            (RoomUpdate::Voice(VoiceEvent::InputLevel(0.5)), "Voice(InputLevel(0.5))"),
            (RoomUpdate::VoicePeers(vec!["b2".into()]), "VoicePeers([\"b2\"])"),
            (
                RoomUpdate::VoiceFailed {
                    member_id: "b2".into(),
                    reason: "x".into(),
                },
                "VoiceFailed { member_id: \"b2\", reason: \"x\" }",
            ),
            (
                RoomUpdate::WatchFailed {
                    member_id: "b2".into(),
                    reason: "y".into(),
                },
                "WatchFailed { member_id: \"b2\", reason: \"y\" }",
            ),
            (
                RoomUpdate::JoinRequest {
                    id: "r1".into(),
                    from: JoinRequester {
                        id: "d1".into(),
                        name: "Caio".into(),
                        avatar: None,
                    },
                },
                "JoinRequest { id: \"r1\", from: JoinRequester {",
            ),
            (RoomUpdate::JoinRequestClosed("r1".into()), "JoinRequestClosed(\"r1\")"),
            (RoomUpdate::Error("cheia".into()), "Error(\"cheia\")"),
            (
                RoomUpdate::Reconnecting { reason: "caiu".into() },
                "Reconnecting { reason: \"caiu\" }",
            ),
            (
                RoomUpdate::Reconnected { resumed: true },
                "Reconnected { resumed: true }",
            ),
            (RoomUpdate::Disconnected, "Disconnected"),
        ];
        for (update, expected) in cases {
            let text = format!("{update:?}");
            assert!(text.starts_with(expected), "{text} não começa com {expected}");
        }
    }

    #[test]
    fn codigo_de_sala_invalido_nao_entra() {
        let err = Room::join("ws://127.0.0.1:1", "abc", JoinAs::named("Ana"), None, None, |_| {}).unwrap_err();
        assert_eq!(err, "Código de sala inválido.");
    }

    #[test]
    fn servico_de_salas_fora_do_ar_avisa_ao_entrar() {
        // Porta 1 no próprio PC: ninguém escuta, a conexão é recusada na hora.
        let code = sv_net::signaling::new_room_code();
        let err = Room::join("ws://127.0.0.1:1", &code, JoinAs::named("Ana"), None, None, |_| {}).unwrap_err();
        assert!(err.starts_with("não deu para alcançar o serviço de salas"), "{err}");
    }
}
