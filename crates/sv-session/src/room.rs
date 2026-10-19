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
use sv_net::{Broadcaster, Member, Peer, RoomClient, RoomEvent as WireEvent, VideoFrame, stun};
use tokio::runtime::Runtime;

use crate::surface::lock;
use crate::{AudioPacket, KeyframeHandle, StreamInfo, StreamTap};

/// O que a interface precisa saber da sala.
pub enum RoomUpdate {
    /// Lista completa de membros (nós primeiro).
    Members(Vec<Member>),
    /// A conexão com quem pedimos para assistir abriu.
    WatchReady {
        member: Member,
        conn: Connection,
        desc: StreamDescription,
    },
    /// Não deu para assistir (recusado ou sem conexão direta).
    WatchFailed { member_id: String, reason: String },
    /// Erro da sala (cheia, versão diferente...).
    Error(String),
    /// Perdemos a conexão com a sinalização.
    Disconnected,
}

impl std::fmt::Debug for RoomUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Members(m) => f.debug_tuple("Members").field(m).finish(),
            Self::WatchReady { member, .. } => f.debug_struct("WatchReady").field("member", member).finish(),
            Self::WatchFailed { member_id, reason } => f
                .debug_struct("WatchFailed")
                .field("member_id", member_id)
                .field("reason", reason)
                .finish(),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}

type OnUpdate = Arc<dyn Fn(RoomUpdate) + Send + Sync>;

struct Inner {
    client: RoomClient,
    peer: Peer,
    me: Mutex<Member>,
    members: Mutex<BTreeMap<String, Member>>,
    broadcaster: Broadcaster,
    /// Formato do que estamos transmitindo (None: não estamos ao vivo ou o encoder ainda não começou).
    stream: Mutex<Option<StreamDescription>>,
    keyframes: Mutex<Option<KeyframeHandle>>,
    /// Quem pedimos para assistir, esperando resposta.
    pending_watch: Mutex<Option<String>>,
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

impl Room {
    /// Entra na sala `code` em `signaling_url`. Bloqueia até a sala responder (ou falhar).
    pub fn join(
        signaling_url: &str,
        code: &str,
        name: &str,
        on_update: impl Fn(RoomUpdate) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("sv-net")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?,
        );
        let on_update: OnUpdate = Arc::new(on_update);
        let (inner, events) = runtime.block_on(async {
            let peer = Peer::bind(stun::DEFAULT_SERVERS)
                .await
                .map_err(|e| format!("rede indisponível: {e}"))?;
            let (client, mut events) = RoomClient::join(signaling_url, code, name).await?;
            // A primeira resposta diz se entramos.
            let me = match events.recv().await {
                Some(WireEvent::Joined { you, members }) => (you, members),
                Some(WireEvent::Error(code)) => return Err(error_message(&code).to_owned()),
                _ => return Err("o serviço de salas não respondeu".to_owned()),
            };
            let (you, members) = me;
            let inner = Arc::new(Inner {
                client,
                peer,
                me: Mutex::new(Member {
                    id: you,
                    name: name.to_owned(),
                    live: false,
                }),
                members: Mutex::new(members.into_iter().map(|m| (m.id.clone(), m)).collect()),
                broadcaster: Broadcaster::new(),
                stream: Mutex::new(None),
                keyframes: Mutex::new(None),
                pending_watch: Mutex::new(None),
                on_update: on_update.clone(),
            });
            Ok::<_, String>((inner, events))
        })?;

        inner.publish_members();
        runtime.spawn(handle_events(inner.clone(), events));
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
        self.inner.client.set_live(true);
        self.inner.publish_members();
    }

    /// Paramos de transmitir: desconecta quem assistia.
    pub fn stream_stopped(&self) {
        *lock(&self.inner.keyframes) = None;
        *lock(&self.inner.stream) = None;
        self.inner.broadcaster.close();
        lock(&self.inner.me).live = false;
        self.inner.client.set_live(false);
        self.inner.publish_members();
    }

    /// Pede para assistir `member_id`. A resposta chega como [`RoomUpdate::WatchReady`] ou
    /// [`RoomUpdate::WatchFailed`].
    pub fn watch(&self, member_id: &str) {
        *lock(&self.inner.pending_watch) = Some(member_id.to_owned());
        let signal = Signal::WatchRequest {
            candidates: self.inner.peer.candidates().to_vec(),
            fingerprint: self.inner.peer.fingerprint().to_owned(),
        };
        self.inner.client.signal(member_id, &signal);
    }

    /// Quantos amigos estão assistindo a gente.
    pub fn viewer_count(&self) -> usize {
        self.inner.broadcaster.viewer_count()
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        self.inner.broadcaster.close();
        self.inner.peer.close();
    }
}

impl Inner {
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
                self.client.signal(&from, &reply);
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
                    });
                    match result {
                        Ok(conn) => (inner.on_update)(RoomUpdate::WatchReady {
                            member,
                            conn,
                            desc: stream,
                        }),
                        Err(e) => (inner.on_update)(RoomUpdate::WatchFailed {
                            member_id: from,
                            reason: e.to_string(),
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

async fn handle_events(inner: Arc<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<WireEvent>) {
    while let Some(event) = events.recv().await {
        match event {
            WireEvent::Joined { .. } => {}
            WireEvent::MemberJoined(m) | WireEvent::MemberUpdated(m) => {
                lock(&inner.members).insert(m.id.clone(), m);
                inner.publish_members();
            }
            WireEvent::MemberLeft(id) => {
                lock(&inner.members).remove(&id);
                inner.publish_members();
            }
            WireEvent::Signal { from, signal } => inner.on_signal(from, signal),
            WireEvent::Error(code) => (inner.on_update)(RoomUpdate::Error(error_message(&code).to_owned())),
            WireEvent::Disconnected => {
                (inner.on_update)(RoomUpdate::Disconnected);
                return;
            }
        }
    }
}

async fn accept_viewers(inner: Arc<Inner>) {
    while let Some(conn) = inner.peer.accept().await {
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
