//! Sala: junta a sinalização (quem está, quem está ao vivo) e a rede P2P (quem assiste quem).
//!
//! Fluxo para assistir alguém (ver `docs/protocolo-malha.md`):
//! 1. quem quer assistir manda `WatchRequest` com seus endereços e sua fingerprint;
//! 2. quem transmite autoriza essa fingerprint, "fura" o NAT na direção dele e responde
//!    `WatchAccept` com os próprios endereços, fingerprint e o formato do vídeo;
//! 3. quem quer assistir conecta (QUIC) e começa a receber.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use quinn::Connection;
use sv_encode::EncodedFrame;
use sv_net::protocol::{FrameHeader, Signal, StreamDescription};
use sv_net::signaling::error_message;
use sv_net::{
    Broadcaster, DiscordUser, JoinRequester, Member, Peer, RoomClient, RoomEvent as WireEvent, VideoFrame, stun,
};
use tokio::runtime::Runtime;

use crate::surface::lock;
use crate::voice_mesh::VoiceMesh;
use crate::{AudioPacket, KeyframeHandle, StreamInfo, StreamTap, VoiceEngine, VoiceEvent, VoiceSettings};

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
    /// Não deu para ligar a voz com `member_id` (sem conexão direta).
    VoiceFailed { member_id: String, reason: String },
    /// A conexão com quem pedimos para assistir abriu.
    WatchReady {
        member: Member,
        conn: Connection,
        desc: StreamDescription,
    },
    /// Não deu para assistir (recusado ou sem conexão direta).
    WatchFailed { member_id: String, reason: String },
    /// Um amigo de alguém da sala pediu para entrar.
    JoinRequest { id: String, from: JoinRequester },
    /// O pedido de entrada foi respondido ou venceu.
    JoinRequestClosed(String),
    /// Erro da sala (cheia, versão diferente...).
    Error(String),
    /// A conexão com a sinalização caiu; tentando voltar para a sala.
    Reconnecting,
    /// Voltamos para a sala depois de uma queda.
    Reconnected,
    /// Perdemos a conexão com a sinalização e não deu para voltar.
    Disconnected,
}

impl std::fmt::Debug for RoomUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Members(m) => f.debug_tuple("Members").field(m).finish(),
            Self::RoomName(n) => f.debug_tuple("RoomName").field(n).finish(),
            Self::Voice(e) => f.debug_tuple("Voice").field(e).finish(),
            Self::VoicePeers(p) => f.debug_tuple("VoicePeers").field(p).finish(),
            Self::VoiceFailed { member_id, reason } => f
                .debug_struct("VoiceFailed")
                .field("member_id", member_id)
                .field("reason", reason)
                .finish(),
            Self::WatchReady { member, .. } => f.debug_struct("WatchReady").field("member", member).finish(),
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
            Self::Reconnecting => f.write_str("Reconnecting"),
            Self::Reconnected => f.write_str("Reconnected"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}

type OnUpdate = Arc<dyn Fn(RoomUpdate) + Send + Sync>;

/// Quanto esperar a resposta de quem transmite a um pedido para assistir.
const WATCH_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);
/// Intervalos entre as tentativas de voltar para a sala depois de uma queda (~2 min no total).
const RECONNECT_DELAYS_S: [u64; 8] = [1, 2, 4, 8, 15, 30, 30, 30];

struct Inner {
    /// Trocado quando a sala reconecta depois de uma queda.
    client: Mutex<RoomClient>,
    signaling_url: String,
    /// Sessão do SquadView, para entrar de novo como conferido ao reconectar.
    session: Option<String>,
    code: String,
    peer: Peer,
    me: Mutex<Member>,
    room_name: Mutex<Option<String>>,
    members: Mutex<BTreeMap<String, Member>>,
    broadcaster: Broadcaster,
    /// Formato do que estamos transmitindo (None: não estamos ao vivo ou o encoder ainda não começou).
    stream: Mutex<Option<StreamDescription>>,
    keyframes: Mutex<Option<KeyframeHandle>>,
    /// Quem pedimos para assistir, esperando resposta.
    pending_watch: Mutex<Option<String>>,
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
}

impl JoinAs {
    /// Só o nome (sem Discord), como nos testes.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
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
        let JoinAs { name, discord, session } = me;
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
            )
            .await?;
            // A primeira resposta diz se entramos.
            let me = match events.recv().await {
                Some(WireEvent::Joined { you, members, room }) => (you, members, room),
                Some(WireEvent::Error(code)) => return Err(error_message(&code).to_owned()),
                _ => return Err("o serviço de salas não respondeu".to_owned()),
            };
            let (you, members, room) = me;
            let inner = Arc::new(Inner {
                client: Mutex::new(client),
                signaling_url: signaling_url.to_owned(),
                session: session.clone(),
                code: code.to_owned(),
                peer,
                me: Mutex::new(Member {
                    id: you,
                    name: name.to_owned(),
                    live: false,
                    muted: false,
                    deafened: false,
                    discord,
                }),
                room_name: Mutex::new(room),
                members: Mutex::new(members.into_iter().map(|m| (m.id.clone(), m)).collect()),
                broadcaster: Broadcaster::new(),
                stream: Mutex::new(None),
                keyframes: Mutex::new(None),
                pending_watch: Mutex::new(None),
                voice,
                runtime: tokio::runtime::Handle::current(),
                on_update: on_update.clone(),
            });
            Ok::<_, String>((inner, events))
        })?;

        inner.publish_members();
        (inner.on_update)(RoomUpdate::RoomName(lock(&inner.room_name).clone()));
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

    /// Paramos de transmitir: desconecta quem assistia.
    pub fn stream_stopped(&self) {
        *lock(&self.inner.keyframes) = None;
        *lock(&self.inner.stream) = None;
        self.inner.broadcaster.close();
        lock(&self.inner.me).live = false;
        self.inner.send_status();
        self.inner.publish_members();
    }

    /// Pede para assistir `member_id`. A resposta chega como [`RoomUpdate::WatchReady`] ou
    /// [`RoomUpdate::WatchFailed`].
    pub fn watch(&self, member_id: &str) {
        *lock(&self.inner.pending_watch) = Some(member_id.to_owned());
        // Sem resposta a tempo (quem transmite saiu, caiu ou tem um app antigo): desiste e avisa.
        let inner = self.inner.clone();
        let id = member_id.to_owned();
        self.runtime.spawn(async move {
            tokio::time::sleep(WATCH_ANSWER_TIMEOUT).await;
            let expired = {
                let mut pending = lock(&inner.pending_watch);
                let expired = pending.as_deref() == Some(id.as_str());
                if expired {
                    *pending = None;
                }
                expired
            };
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

    /// Quantos amigos estão assistindo a gente.
    pub fn viewer_count(&self) -> usize {
        self.inner.broadcaster.viewer_count()
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        self.inner.broadcaster.close();
        self.inner.voice.close();
        self.inner.peer.close();
    }
}

impl Inner {
    /// Manda para a sala o nosso estado (ao vivo, mudo, surdo).
    fn send_status(&self) {
        let me = lock(&self.me).clone();
        lock(&self.client).set_status(me.live, me.muted, me.deafened);
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
        let members: Vec<String> = lock(&self.members).keys().cloned().collect();
        for m in members {
            self.offer_voice_to(&m);
        }
    }

    fn publish_voice_peers(&self) {
        (self.on_update)(RoomUpdate::VoicePeers(self.voice.connected()));
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

    fn on_signal(self: &Arc<Self>, from: String, signal: Signal) {
        match signal {
            Signal::WatchRequest {
                candidates,
                fingerprint,
            } => {
                let desc = *lock(&self.stream);
                let ready = desc.is_some() && lock(&self.keyframes).is_some();
                let reply = match (ready, desc) {
                    (true, Some(stream)) => {
                        self.peer.allowed().allow(&fingerprint);
                        self.peer.punch(&candidates);
                        Signal::WatchAccept {
                            candidates: self.peer.candidates().to_vec(),
                            fingerprint: self.peer.fingerprint().to_owned(),
                            stream,
                        }
                    }
                    _ => Signal::WatchDeny {
                        reason: "Não está transmitindo agora.".into(),
                    },
                };
                lock(&self.client).signal(&from, &reply);
            }
            Signal::WatchAccept {
                candidates,
                fingerprint,
                stream,
            } => {
                let expected = lock(&self.pending_watch).as_deref() == Some(from.as_str());
                if !expected {
                    return;
                }
                let inner = self.clone();
                tokio::spawn(async move {
                    let result = inner.peer.connect(&candidates, &fingerprint).await;
                    *lock(&inner.pending_watch) = None;
                    let member = inner.member(&from).unwrap_or(Member {
                        id: from.clone(),
                        name: "?".into(),
                        live: true,
                        muted: false,
                        deafened: false,
                        discord: None,
                    });
                    match result {
                        Ok(conn) => (inner.on_update)(RoomUpdate::WatchReady {
                            member,
                            conn,
                            desc: stream,
                        }),
                        Err(e) => (inner.on_update)(RoomUpdate::WatchFailed {
                            member_id: from,
                            reason: connect_failure(&e, inner.peer.nat()),
                        }),
                    }
                });
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
                let answer = Signal::VoiceAnswer {
                    candidates: self.peer.candidates().to_vec(),
                    fingerprint: self.peer.fingerprint().to_owned(),
                };
                lock(&self.client).signal(&from, &answer);
            }
            Signal::VoiceAnswer {
                candidates,
                fingerprint,
            } => {
                if !self.voice.answered(&from) {
                    return;
                }
                let inner = self.clone();
                tokio::spawn(async move {
                    match inner
                        .peer
                        .connect_for(&candidates, &fingerprint, sv_net::Purpose::Voice)
                        .await
                    {
                        Ok(conn) => {
                            inner.voice.register(&inner.runtime, from, conn);
                            inner.publish_voice_peers();
                        }
                        Err(e) => (inner.on_update)(RoomUpdate::VoiceFailed {
                            member_id: from,
                            reason: connect_failure(&e, inner.peer.nat()),
                        }),
                    }
                });
            }
            Signal::WatchDeny { reason } => {
                if lock(&self.pending_watch).take().as_deref() == Some(from.as_str()) {
                    (self.on_update)(RoomUpdate::WatchFailed {
                        member_id: from,
                        reason,
                    });
                }
            }
        }
    }
}

/// Explica por que não deu para conectar. Se o nosso NAT é simétrico, a causa é quase certa.
fn connect_failure(error: &sv_net::ConnectError, nat: sv_net::NatMapping) -> String {
    match (error, nat) {
        (sv_net::ConnectError::Unreachable, sv_net::NatMapping::Symmetric) => {
            "não deu para conectar direto: a sua internet troca de porta a cada conexão (NAT simétrico,              comum em CGNAT de operadora e em 4G). Tente por outra rede, como o roteador de casa ou o              celular. A ponte por outro membro da sala chega numa próxima versão."
                .into()
        }
        _ => error.to_string(),
    }
}

async fn handle_events(inner: Arc<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<WireEvent>) {
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
                inner.voice.unregister(&id);
                inner.publish_members();
                inner.publish_voice_peers();
            }
            WireEvent::Signal { from, signal } => inner.on_signal(from, signal),
            WireEvent::JoinRequest { id, from } => (inner.on_update)(RoomUpdate::JoinRequest { id, from }),
            WireEvent::JoinRequestClosed(id) => (inner.on_update)(RoomUpdate::JoinRequestClosed(id)),
            WireEvent::Error(code) => (inner.on_update)(RoomUpdate::Error(error_message(&code).to_owned())),
            WireEvent::Disconnected => return,
        }
    }
}

/// Ouve a sala; se a sinalização cair, volta para ela e continua ouvindo.
async fn run_session(inner: Arc<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<WireEvent>) {
    loop {
        handle_events(inner.clone(), events).await;
        match reconnect(&inner).await {
            Some(next) => events = next,
            None => return,
        }
    }
}

/// A sinalização caiu: entra de novo na mesma sala, com o mesmo nome. As conexões QUIC (quem
/// assiste a gente e quem a gente assiste) não dependem dela e continuam. `None` se desistiu.
async fn reconnect(inner: &Arc<Inner>) -> Option<tokio::sync::mpsc::UnboundedReceiver<WireEvent>> {
    (inner.on_update)(RoomUpdate::Reconnecting);
    for delay in RECONNECT_DELAYS_S {
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        let (name, discord, live) = {
            let me = lock(&inner.me);
            (me.name.clone(), me.discord.clone(), me.live)
        };
        let room_name = lock(&inner.room_name).clone();
        let Ok((client, mut events)) = RoomClient::join(
            &inner.signaling_url,
            &inner.code,
            &name,
            discord.as_ref(),
            inner.session.as_deref(),
            room_name.as_deref(),
        )
        .await
        else {
            continue;
        };
        match events.recv().await {
            Some(WireEvent::Joined { you, members, room }) => {
                *lock(&inner.client) = client;
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
                (inner.on_update)(RoomUpdate::Reconnected);
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
            match accepted.fingerprint.as_deref().and_then(|f| inner.voice.member_for(f)) {
                Some(member) => {
                    inner.voice.register(&inner.runtime, member, accepted.conn);
                    inner.publish_voice_peers();
                }
                None => accepted.conn.close(quinn::VarInt::from_u32(2), b"desconhecido"),
            }
            continue;
        }
        let conn = accepted.conn;
        let keyframes = lock(&inner.keyframes).clone();
        let Some(keyframes) = keyframes else {
            conn.close(quinn::VarInt::from_u32(1), b"nao esta transmitindo");
            continue;
        };
        inner.broadcaster.add(conn, Arc::new(move || keyframes.request()));
    }
}

/// Entrega o que a prévia produz para os amigos conectados.
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
