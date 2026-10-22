//! Mensagens privadas entre dois amigos (ADR 0017): texto e arquivos direto entre os dois PCs,
//! por QUIC, sem nada passar pelo servidor. O registro de amigos do Worker só repassa o sinal
//! ([`DmSignal`]) para os dois se acharem.
//!
//! Quem quer falar e não tem conexão manda uma oferta (endereços e a impressão digital do nosso
//! certificado); quem recebe autoriza essa impressão digital e responde com a dele; quem ofereceu
//! conecta. Se os dois oferecerem ao mesmo tempo, fica a oferta de quem tem o id menor.
//!
//! Nada aqui roda no caminho da mídia: é outro `Peer`, outro runtime e outro ALPN.

mod frame;
mod history;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quinn::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use sv_net::signaling::TurnCredentials;
use sv_net::{Peer, Purpose};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::runtime::Runtime;

pub use frame::{Frame, MAX_TEXT_CHARS};
pub use history::{Attachment, Conversation, DmMessage, History, MAX_MESSAGES, Reaction, Status};

use crate::surface::lock;
use frame::{FEATURE_REACTIONS, FileHeader, MAX_REACTIONS, parse_frame, read_frame, valid_emoji, write_frame};

/// Uma oferta sem resposta por esse tempo pode ser repetida (o amigo pode ter caído no meio).
const OFFER_RETRY: Duration = Duration::from_secs(10);
/// Duas conexões com o mesmo amigo abertas dentro desse tempo vieram dos dois chamando juntos:
/// fica a mesma nos dois lados (ver [`keeps_existing`]). Depois disso, a nova é uma volta (o amigo
/// reabriu o app) e substitui a antiga.
const GLARE_WINDOW: Duration = Duration::from_secs(10);
/// De quanto em quanto o progresso de um arquivo é avisado (bytes).
const PROGRESS_EVERY: u64 = 512 * 1024;
const CHUNK: usize = 256 * 1024;

/// O sinal que o registro de amigos repassa (vai em `dm-signal.payload`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DmSignal {
    DmOffer {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
        /// O que este app entende além do básico (ex.: reações). Apps antigos não mandam nem leem.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        features: Vec<String>,
    },
    DmAnswer {
        candidates: Vec<SocketAddr>,
        fingerprint: String,
        /// Nosso endereço no repassador TURN, para quem não conecta direto (ADR 0014).
        #[serde(default)]
        relay: Option<SocketAddr>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        features: Vec<String>,
    },
}

/// O que este app anuncia nos sinais da conversa.
fn our_features() -> Vec<String> {
    vec![FEATURE_REACTIONS.to_owned()]
}

/// O que a interface fica sabendo.
// A mensagem é o evento quase sempre, e os eventos são poucos (um por mensagem ou mudança): não vale
// pôr numa caixa só para os avisos de online ficarem menores.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DmEvent {
    /// Mensagem nova ou que mudou (entregue, progresso do anexo).
    Message { friend: String, message: DmMessage },
    /// A conexão direta com o amigo abriu ou fechou.
    Online { friend: String, online: bool },
    /// O amigo não está online: as mensagens esperam no nosso PC.
    Unreachable { friend: String },
}

pub type SignalOut = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
pub type OnEvent = Arc<dyn Fn(DmEvent) + Send + Sync>;

/// Por que não deu para mandar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    Empty,
    TooLong,
    /// Arquivo que não dá para ler (sumiu, sem permissão, é uma pasta).
    Unreadable(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "mensagem vazia"),
            Self::TooLong => write!(f, "mensagem com mais de {MAX_TEXT_CHARS} caracteres"),
            Self::Unreadable(e) => write!(f, "não deu para ler o arquivo: {e}"),
        }
    }
}

/// Uma conexão aberta com um amigo.
struct Link {
    conn: Connection,
    control: Arc<tokio::sync::Mutex<SendStream>>,
    /// Fomos nós que conectamos (respondendo a oferta do amigo, foi ele).
    dialed_by_me: bool,
    since: Instant,
}

/// O que importa de uma conexão que já existe para decidir se ela fica.
#[derive(Debug, Clone, Copy)]
struct LinkFacts {
    dialed_by_me: bool,
    age: Duration,
    alive: bool,
}

/// Chegou outra conexão com o amigo: a que já existe fica? Os dois lados precisam escolher a
/// **mesma**. Antes ficava a última que chegava, e quando os dois chamavam juntos as duas podiam
/// chegar em ordens diferentes nos dois PCs: cada um ficava com uma e fechava a do outro, e as
/// mensagens paravam (o teste `os_dois_chamando_juntos_fica_uma_conexao_so` falhava na CI).
///
/// Regra: dentro da janela de [`GLARE_WINDOW`], vence a conexão aberta por quem tem o id menor,
/// que é também quem os dois escolhem para chamar quando as ofertas se cruzam. Fora dela, ou com a
/// antiga já fechada, fica a nova.
fn keeps_existing(me: &str, friend: &str, existing: Option<LinkFacts>, new_dialed_by_me: bool) -> bool {
    let Some(old) = existing else { return false };
    if !old.alive || old.age >= GLARE_WINDOW {
        return false;
    }
    // A conexão aberta pelo id menor: por nós se o nosso id é o menor, senão pelo amigo.
    let preferred = |dialed_by_me: bool| dialed_by_me == (me < friend);
    preferred(old.dialed_by_me) && !preferred(new_dialed_by_me)
}

/// Um download em andamento (sobrevive a quedas: na volta, pede de onde parou).
#[derive(Clone)]
struct Download {
    friend: String,
    dest: PathBuf,
}

struct Inner {
    me: String,
    history: History,
    stun: Vec<&'static str>,
    peer: tokio::sync::OnceCell<Arc<Peer>>,
    links: Mutex<HashMap<String, Link>>,
    /// Ofertas nossas esperando resposta.
    offering: Mutex<HashMap<String, Instant>>,
    /// Impressão digital de quem pode conectar em nós → o amigo.
    expected: Mutex<HashMap<String, String>>,
    downloads: Mutex<HashMap<String, Download>>,
    /// Amigos cujo app entende reações (avisaram no último `dm-offer` ou `dm-answer`).
    reactions_ok: Mutex<std::collections::HashSet<String>>,
    signal_out: SignalOut,
    on_event: OnEvent,
    counter: AtomicU64,
}

/// As conversas privadas deste app. Descartar fecha as conexões.
pub struct DmEngine {
    inner: Arc<Inner>,
    runtime: Option<Runtime>,
}

impl std::fmt::Debug for DmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DmEngine")
            .field("me", &self.inner.me)
            .finish_non_exhaustive()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Tipo do arquivo pelo nome (para a interface mostrar prévia de imagem e vídeo).
pub fn mime_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// O arquivo em andamento de um download (`<destino>.part`).
fn part_of(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

impl DmEngine {
    /// Começa as conversas de `me` (id do Discord), com o histórico em `dir`. `signal_out` manda o
    /// sinal pela conexão de amigos; `on_event` avisa a interface.
    pub fn start(
        me: &str,
        dir: impl Into<PathBuf>,
        stun: &[&'static str],
        signal_out: SignalOut,
        on_event: OnEvent,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("sv-dm")
            .enable_all()
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                me: me.to_owned(),
                history: History::new(dir),
                stun: stun.to_vec(),
                peer: tokio::sync::OnceCell::new(),
                links: Mutex::new(HashMap::new()),
                offering: Mutex::new(HashMap::new()),
                expected: Mutex::new(HashMap::new()),
                downloads: Mutex::new(HashMap::new()),
                reactions_ok: Mutex::default(),
                signal_out,
                on_event,
                counter: AtomicU64::new(0),
            }),
            runtime: Some(runtime),
        })
    }

    fn spawn<F: Future<Output = ()> + Send + 'static>(&self, task: F) {
        if let Some(rt) = &self.runtime {
            rt.spawn(task);
        }
    }

    pub fn conversations(&self) -> Vec<Conversation> {
        self.inner.history.conversations()
    }

    pub fn messages(&self, friend: &str) -> Vec<DmMessage> {
        self.inner.history.messages(friend)
    }

    /// A conexão direta com o amigo está aberta agora.
    pub fn online(&self, friend: &str) -> bool {
        lock(&self.inner.links).contains_key(friend)
    }

    /// Há mensagens nossas esperando: tenta abrir a conexão com o amigo de novo (ele acabou de
    /// ficar online, ou a tentativa anterior não teve resposta). Sem pendentes, ou já conectado,
    /// não faz nada; entre tentativas espera `OFFER_RETRY`. Devolve se tentou.
    pub fn retry(&self, friend: &str) -> bool {
        let nothing =
            self.inner.history.pending(friend).is_empty() && self.inner.history.unsent_reactions(friend).is_empty();
        if self.inner.has_link(friend) || nothing {
            return false;
        }
        let inner = self.inner.clone();
        let friend = friend.to_owned();
        self.spawn(async move { inner.ensure_link(&friend).await });
        true
    }

    /// Apaga o histórico de uma conversa (ou de todas).
    pub fn clear(&self, friend: Option<&str>) {
        self.inner.history.clear(friend);
    }

    /// Manda um texto. Sem conexão, fica pendente e vai quando os dois estiverem online.
    pub fn send_text(&self, friend: &str, body: &str) -> Result<DmMessage, SendError> {
        self.reply(friend, body, None)
    }

    /// Manda um texto respondendo a mensagem `reply_to` (ou um texto normal, com `None`).
    pub fn reply(&self, friend: &str, body: &str, reply_to: Option<&str>) -> Result<DmMessage, SendError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(SendError::Empty);
        }
        if body.chars().count() > MAX_TEXT_CHARS {
            return Err(SendError::TooLong);
        }
        let message = DmMessage {
            id: self.inner.new_id(),
            mine: true,
            at: now_ms(),
            text: Some(body.to_owned()),
            file: None,
            status: Status::Pending,
            reply_to: reply_to.map(str::to_owned),
            reactions: Vec::new(),
            reactions_unsent: false,
        };
        self.queue(friend, message.clone());
        Ok(message)
    }

    /// Põe (`on`) ou tira a nossa reação `emoji` da mensagem `id`. O amigo fica sabendo agora ou,
    /// sem conexão (ou com o app dele sem reações), quando der. Devolve a mensagem como ficou.
    pub fn react(&self, friend: &str, id: &str, emoji: &str, on: bool) -> Option<DmMessage> {
        if !valid_emoji(emoji) {
            return None;
        }
        let message = self.inner.history.update(friend, id, |m| {
            match m.reactions.iter().position(|r| r.emoji == emoji) {
                Some(i) => m.reactions[i].mine = on,
                None if on && m.reactions.len() < MAX_REACTIONS => m.reactions.push(Reaction {
                    emoji: emoji.to_owned(),
                    mine: true,
                    theirs: false,
                }),
                None => {}
            }
            m.reactions.retain(|r| r.mine || r.theirs);
            m.reactions_unsent = true;
        })?;
        self.inner.emit(DmEvent::Message {
            friend: friend.to_owned(),
            message: message.clone(),
        });
        let inner = self.inner.clone();
        let friend = friend.to_owned();
        let id = id.to_owned();
        self.spawn(async move {
            if inner.has_link(&friend) {
                inner.send_reactions(&friend, &id).await;
            } else {
                inner.ensure_link(&friend).await;
            }
        });
        Some(message)
    }

    /// Manda um arquivo. Com `keep_copy`, o app guarda uma cópia nessa pasta (e a conversa aponta
    /// para ela); sem, a conversa aponta para o arquivo original, sem cópia nenhuma.
    pub fn send_file(&self, friend: &str, path: &Path, keep_copy: Option<&Path>) -> Result<DmMessage, SendError> {
        let meta = std::fs::metadata(path).map_err(|e| SendError::Unreadable(e.to_string()))?;
        if !meta.is_file() {
            return Err(SendError::Unreadable("não é um arquivo".into()));
        }
        let name = path
            .file_name()
            .map_or_else(|| "arquivo".into(), |n| n.to_string_lossy().into_owned());
        let id = self.inner.new_id();
        let local = match keep_copy {
            Some(dir) => {
                let copy = dir.join(format!("{id}-{name}"));
                std::fs::create_dir_all(dir)
                    .and_then(|()| std::fs::copy(path, &copy))
                    .map_err(|e| SendError::Unreadable(e.to_string()))?;
                copy
            }
            None => path.to_path_buf(),
        };
        let message = DmMessage {
            id,
            mine: true,
            at: now_ms(),
            text: None,
            file: Some(Attachment {
                mime: mime_of(&name).to_owned(),
                name,
                size: meta.len(),
                path: Some(local.to_string_lossy().into_owned()),
                done: 0,
                gone: false,
            }),
            status: Status::Pending,
            reply_to: None,
            reactions: Vec::new(),
            reactions_unsent: false,
        };
        self.queue(friend, message.clone());
        Ok(message)
    }

    /// Cancela o download do anexo `id`: para de receber (quem manda também para) e apaga o que já
    /// tinha chegado. A mensagem volta a mostrar "Baixar". Devolve se havia algo baixando.
    pub fn cancel_download(&self, friend: &str, id: &str) -> bool {
        let removed = {
            let mut downloads = lock(&self.inner.downloads);
            match downloads.get(id) {
                Some(d) if d.friend == friend => downloads.remove(id),
                _ => None,
            }
        };
        let Some(download) = removed else { return false };
        // Se o stream estiver chegando, ele percebe na próxima volta e também apaga.
        let _ = std::fs::remove_file(part_of(&download.dest));
        self.inner.progress(friend, id, |f| f.done = 0);
        true
    }

    /// Baixa o anexo `id` do amigo para `dest`. Se já veio uma parte antes, continua de onde parou.
    pub fn download(&self, friend: &str, id: &str, dest: &Path) {
        lock(&self.inner.downloads).insert(
            id.to_owned(),
            Download {
                friend: friend.to_owned(),
                dest: dest.to_path_buf(),
            },
        );
        let inner = self.inner.clone();
        let (friend, id) = (friend.to_owned(), id.to_owned());
        self.spawn(async move {
            if inner.has_link(&friend) {
                inner.request_file(&friend, &id).await;
            } else {
                inner.ensure_link(&friend).await;
            }
        });
    }

    /// Chegou um sinal do amigo `from` pela conexão de amigos.
    pub fn on_signal(&self, from: &str, payload: serde_json::Value) {
        let Ok(signal) = serde_json::from_value::<DmSignal>(payload) else {
            return;
        };
        let inner = self.inner.clone();
        let from = from.to_owned();
        self.spawn(async move { inner.on_signal(from, signal).await });
    }

    /// O registro avisou que o amigo não está online: a oferta não vai ter resposta.
    pub fn on_unreachable(&self, friend: &str) {
        lock(&self.inner.offering).remove(friend);
        self.inner.emit(DmEvent::Unreachable {
            friend: friend.to_owned(),
        });
    }

    /// Liga o repassador TURN (credenciais pedidas ao registro), para quem está atrás de CGNAT.
    pub fn set_turn(&self, credentials: TurnCredentials) {
        let inner = self.inner.clone();
        self.spawn(async move {
            let Some(peer) = inner.peer().await else { return };
            let server = tokio::task::spawn_blocking(move || credentials.server())
                .await
                .ok()
                .flatten();
            if let Some(server) = server {
                let _ = peer.enable_relay(server).await;
            }
        });
    }

    /// Guarda a mensagem e manda (agora, ou quando a conexão abrir).
    fn queue(&self, friend: &str, message: DmMessage) {
        self.inner.history.push(friend, message.clone());
        self.inner.emit(DmEvent::Message {
            friend: friend.to_owned(),
            message: message.clone(),
        });
        let inner = self.inner.clone();
        let friend = friend.to_owned();
        self.spawn(async move {
            if inner.has_link(&friend) {
                inner.send(&friend, &frame_of(&message)).await;
            } else {
                inner.ensure_link(&friend).await;
            }
        });
    }
}

impl Drop for DmEngine {
    fn drop(&mut self) {
        for (_, link) in lock(&self.inner.links).drain() {
            link.conn.close(0u32.into(), b"saiu");
        }
        if let Some(peer) = self.inner.peer.get() {
            peer.close();
        }
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

/// O quadro que leva uma mensagem nossa (texto ou o aviso do arquivo).
fn frame_of(message: &DmMessage) -> Frame {
    match &message.file {
        Some(f) => Frame::FileOffer {
            id: message.id.clone(),
            at: message.at,
            name: f.name.clone(),
            size: f.size,
            mime: f.mime.clone(),
        },
        None => Frame::Text {
            id: message.id.clone(),
            at: message.at,
            body: message.text.clone().unwrap_or_default(),
            reply_to: message.reply_to.clone(),
        },
    }
}

impl Inner {
    fn emit(&self, event: DmEvent) {
        (self.on_event)(event);
    }

    fn new_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let tail = &self.me[self.me.len().saturating_sub(6)..];
        format!("{tail}-{:x}-{n:x}", now_ms())
    }

    fn has_link(&self, friend: &str) -> bool {
        lock(&self.links).contains_key(friend)
    }

    /// O nosso ponto QUIC, aberto na primeira conversa (e o laço que aceita quem conecta em nós).
    async fn peer(self: &Arc<Self>) -> Option<Arc<Peer>> {
        let me = self.clone();
        self.peer
            .get_or_try_init(|| async move {
                let peer = Arc::new(Peer::bind(&me.stun).await?);
                tokio::spawn(accept_loop(me.clone(), peer.clone()));
                Ok::<_, std::io::Error>(peer)
            })
            .await
            .ok()
            .cloned()
    }

    /// Sem conexão com o amigo: oferece (uma vez a cada tanto, se ele não responder).
    async fn ensure_link(self: &Arc<Self>, friend: &str) {
        if self.has_link(friend) {
            return;
        }
        {
            let mut offering = lock(&self.offering);
            if offering.get(friend).is_some_and(|at| at.elapsed() < OFFER_RETRY) {
                return;
            }
            offering.insert(friend.to_owned(), Instant::now());
        }
        let Some(peer) = self.peer().await else { return };
        let offer = DmSignal::DmOffer {
            candidates: peer.candidates().to_vec(),
            fingerprint: peer.fingerprint().to_owned(),
            features: our_features(),
        };
        if let Ok(json) = serde_json::to_value(&offer) {
            (self.signal_out)(friend, json);
        }
    }

    async fn on_signal(self: Arc<Self>, from: String, signal: DmSignal) {
        match signal {
            DmSignal::DmOffer {
                candidates,
                fingerprint,
                features,
            } => {
                self.note_features(&from, &features);
                // Os dois ofereceram: fica a oferta de quem tem o id menor (o outro só responde).
                let mine_pending = lock(&self.offering).contains_key(&from);
                if mine_pending && self.me < from {
                    return;
                }
                lock(&self.offering).remove(&from);
                let Some(peer) = self.peer().await else { return };
                peer.allowed().allow(&fingerprint);
                lock(&self.expected).insert(fingerprint, from.clone());
                peer.punch(&candidates);
                peer.permit_relay(&candidates);
                let answer = DmSignal::DmAnswer {
                    candidates: peer.candidates().to_vec(),
                    fingerprint: peer.fingerprint().to_owned(),
                    relay: peer.relay_addr(),
                    features: our_features(),
                };
                if let Ok(json) = serde_json::to_value(&answer) {
                    (self.signal_out)(&from, json);
                }
            }
            DmSignal::DmAnswer {
                candidates,
                fingerprint,
                relay,
                features,
            } => {
                if lock(&self.offering).remove(&from).is_none() {
                    return;
                }
                self.note_features(&from, &features);
                let Some(peer) = self.peer().await else { return };
                match peer.connect_for(&candidates, relay, &fingerprint, Purpose::Dm).await {
                    Ok(conn) => match conn.open_bi().await {
                        Ok((mut send, recv)) => {
                            if write_frame(&mut send, &Frame::Hello).await.is_ok() {
                                self.attach(from, conn, send, recv, true).await;
                            }
                        }
                        Err(_) => conn.close(0u32.into(), b"sem controle"),
                    },
                    Err(_) => self.emit(DmEvent::Unreachable { friend: from }),
                }
            }
        }
    }

    /// Conexão aberta (por nós, `dialed_by_me`, ou pelo amigo): passa a ser a do amigo, manda o que
    /// estava pendente e retoma os downloads. Se já existe outra e os dois lados ficam com ela (ver
    /// [`keeps_existing`]), a nova é fechada.
    async fn attach(
        self: &Arc<Self>,
        friend: String,
        conn: Connection,
        send: SendStream,
        recv: RecvStream,
        dialed_by_me: bool,
    ) {
        let previous = {
            let mut links = lock(&self.links);
            let existing = links.get(&friend).map(|l| LinkFacts {
                dialed_by_me: l.dialed_by_me,
                age: l.since.elapsed(),
                alive: l.conn.close_reason().is_none(),
            });
            if keeps_existing(&self.me, &friend, existing, dialed_by_me) {
                drop(links);
                conn.close(0u32.into(), b"duplicada");
                return;
            }
            links.insert(
                friend.clone(),
                Link {
                    conn: conn.clone(),
                    control: Arc::new(tokio::sync::Mutex::new(send)),
                    dialed_by_me,
                    since: Instant::now(),
                },
            )
        };
        if let Some(old) = previous {
            old.conn.close(0u32.into(), b"trocada");
        }
        self.emit(DmEvent::Online {
            friend: friend.clone(),
            online: true,
        });
        tokio::spawn(read_loop(self.clone(), friend.clone(), conn.clone(), recv));
        tokio::spawn(file_streams(self.clone(), friend.clone(), conn));
        for message in self.history.pending(&friend) {
            self.send(&friend, &frame_of(&message)).await;
        }
        for message in self.history.unsent_reactions(&friend) {
            self.send_reactions(&friend, &message.id).await;
        }
        let resume: Vec<String> = lock(&self.downloads)
            .iter()
            .filter(|(_, d)| d.friend == friend)
            .map(|(id, _)| id.clone())
            .collect();
        for id in resume {
            self.request_file(&friend, &id).await;
        }
    }

    /// Guarda o que o app do amigo entende (do sinal que ele mandou).
    fn note_features(&self, friend: &str, features: &[String]) {
        let mut ok = lock(&self.reactions_ok);
        if features.iter().any(|f| f == FEATURE_REACTIONS) {
            ok.insert(friend.to_owned());
        } else {
            ok.remove(friend);
        }
    }

    /// Manda as nossas reações atuais na mensagem `id`, se o app do amigo entende; chegando, a
    /// mensagem deixa de estar pendente.
    async fn send_reactions(&self, friend: &str, id: &str) {
        if !lock(&self.reactions_ok).contains(friend) {
            return;
        }
        let Some(message) = self.history.messages(friend).into_iter().find(|m| m.id == id) else {
            return;
        };
        let emojis = message
            .reactions
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.emoji.clone())
            .collect();
        let frame = Frame::Reactions {
            id: id.to_owned(),
            emojis,
        };
        if self.send(friend, &frame).await {
            self.history.update(friend, id, |m| m.reactions_unsent = false);
        }
    }

    async fn send(&self, friend: &str, frame: &Frame) -> bool {
        let control = lock(&self.links).get(friend).map(|l| l.control.clone());
        let Some(control) = control else { return false };
        let mut stream = control.lock().await;
        write_frame(&mut *stream, frame).await.is_ok()
    }

    /// Pede o anexo a partir do que já chegou (`.part`).
    async fn request_file(&self, friend: &str, id: &str) {
        let dest = lock(&self.downloads).get(id).map(|d| d.dest.clone());
        let Some(dest) = dest else { return };
        let offset = tokio::fs::metadata(part_of(&dest)).await.map_or(0, |m| m.len());
        self.send(
            friend,
            &Frame::FileRequest {
                id: id.to_owned(),
                offset,
            },
        )
        .await;
    }

    async fn on_frame(self: &Arc<Self>, friend: &str, frame: Frame) {
        match frame {
            Frame::Hello => {}
            Frame::Text { id, at, body, reply_to } => {
                if body.chars().count() <= MAX_TEXT_CHARS {
                    let message = DmMessage {
                        id: id.clone(),
                        mine: false,
                        at,
                        text: Some(body),
                        file: None,
                        status: Status::Received,
                        reply_to,
                        reactions: Vec::new(),
                        reactions_unsent: false,
                    };
                    self.receive(friend, message);
                }
                self.send(friend, &Frame::Ack { id }).await;
            }
            Frame::FileOffer {
                id,
                at,
                name,
                size,
                mime,
            } => {
                let message = DmMessage {
                    id: id.clone(),
                    mine: false,
                    at,
                    text: None,
                    file: Some(Attachment {
                        name,
                        size,
                        mime,
                        path: None,
                        done: 0,
                        gone: false,
                    }),
                    status: Status::Received,
                    reply_to: None,
                    reactions: Vec::new(),
                    reactions_unsent: false,
                };
                self.receive(friend, message);
                self.send(friend, &Frame::Ack { id }).await;
            }
            Frame::Reactions { id, emojis } => {
                let theirs: Vec<String> = emojis
                    .into_iter()
                    .filter(|e| valid_emoji(e))
                    .take(MAX_REACTIONS)
                    .collect();
                if let Some(message) = self.history.update(friend, &id, |m| {
                    for r in &mut m.reactions {
                        r.theirs = theirs.contains(&r.emoji);
                    }
                    for emoji in &theirs {
                        if m.reactions.len() < MAX_REACTIONS && !m.reactions.iter().any(|r| &r.emoji == emoji) {
                            m.reactions.push(Reaction {
                                emoji: emoji.clone(),
                                mine: false,
                                theirs: true,
                            });
                        }
                    }
                    m.reactions.retain(|r| r.mine || r.theirs);
                }) {
                    self.emit(DmEvent::Message {
                        friend: friend.to_owned(),
                        message,
                    });
                }
            }
            Frame::Ack { id } => {
                if let Some(message) = self.history.update(friend, &id, |m| {
                    if m.mine {
                        m.status = Status::Delivered;
                    }
                }) {
                    self.emit(DmEvent::Message {
                        friend: friend.to_owned(),
                        message,
                    });
                }
            }
            Frame::FileRequest { id, offset } => {
                let path = self
                    .history
                    .messages(friend)
                    .into_iter()
                    .find(|m| m.id == id && m.mine)
                    .and_then(|m| m.file?.path);
                if let Some(path) = path {
                    tokio::spawn(send_file(
                        self.clone(),
                        friend.to_owned(),
                        id,
                        PathBuf::from(path),
                        offset,
                    ));
                }
            }
        }
    }

    fn receive(&self, friend: &str, message: DmMessage) {
        if self.history.push(friend, message.clone()) {
            self.emit(DmEvent::Message {
                friend: friend.to_owned(),
                message,
            });
        }
    }

    /// Atualiza o progresso de um anexo e avisa a interface.
    fn progress(&self, friend: &str, id: &str, change: impl FnOnce(&mut Attachment)) {
        if let Some(message) = self.history.update(friend, id, |m| {
            if let Some(f) = m.file.as_mut() {
                change(f);
            }
        }) {
            self.emit(DmEvent::Message {
                friend: friend.to_owned(),
                message,
            });
        }
    }

    /// A conexão com o amigo fechou: só tira se ainda for a mesma (pode já ter outra no lugar).
    fn detach(&self, friend: &str, conn: &Connection) {
        let mut links = lock(&self.links);
        if links
            .get(friend)
            .is_some_and(|l| l.conn.stable_id() == conn.stable_id())
        {
            links.remove(friend);
            drop(links);
            self.emit(DmEvent::Online {
                friend: friend.to_owned(),
                online: false,
            });
        }
    }
}

/// Aceita quem conecta em nós: só conversas, e só de quem respondemos uma oferta.
async fn accept_loop(inner: Arc<Inner>, peer: Arc<Peer>) {
    while let Some(accepted) = peer.accept().await {
        let friend = accepted
            .fingerprint
            .as_ref()
            .and_then(|fp| lock(&inner.expected).remove(fp));
        match (accepted.purpose, friend) {
            (Purpose::Dm, Some(friend)) => {
                let inner = inner.clone();
                tokio::spawn(async move {
                    match accepted.conn.accept_bi().await {
                        Ok((send, mut recv)) => {
                            // O primeiro quadro é o `Hello` de quem conectou.
                            if let Ok(Some(Frame::Hello)) = read_frame::<_, Frame>(&mut recv).await {
                                inner.attach(friend, accepted.conn, send, recv, false).await;
                            }
                        }
                        Err(_) => accepted.conn.close(0u32.into(), b"sem controle"),
                    }
                });
            }
            _ => accepted.conn.close(0u32.into(), b"desconhecido"),
        }
    }
}

/// Lê os quadros de controle até a conexão fechar.
async fn read_loop(inner: Arc<Inner>, friend: String, conn: Connection, mut recv: RecvStream) {
    // Lê como JSON e só então como quadro: um tipo novo (de um app mais novo) é ignorado, em vez
    // de derrubar a conexão como acontecia nos apps até a 0.9.
    while let Ok(Some(value)) = read_frame::<_, serde_json::Value>(&mut recv).await {
        if let Some(frame) = parse_frame(value) {
            inner.on_frame(&friend, frame).await;
        }
    }
    inner.detach(&friend, &conn);
}

/// Recebe os arquivos que pedimos (um stream por arquivo).
async fn file_streams(inner: Arc<Inner>, friend: String, conn: Connection) {
    while let Ok(recv) = conn.accept_uni().await {
        tokio::spawn(receive_file(inner.clone(), friend.clone(), recv));
    }
}

async fn receive_file(inner: Arc<Inner>, friend: String, mut recv: RecvStream) {
    let Ok(Some(header)) = read_frame::<_, FileHeader>(&mut recv).await else {
        return;
    };
    let download = lock(&inner.downloads).get(&header.id).cloned();
    let Some(download) = download.filter(|d| d.friend == friend) else {
        return;
    };
    let size = inner
        .history
        .messages(&friend)
        .into_iter()
        .find(|m| m.id == header.id)
        .and_then(|m| m.file)
        .map_or(0, |f| f.size);
    let part = part_of(&download.dest);
    if let Some(parent) = part.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .await
    else {
        return;
    };
    // Continua exatamente de onde o pedido disse (o que passou disso é descartado).
    if file.set_len(header.offset).await.is_err() || file.seek(std::io::SeekFrom::Start(header.offset)).await.is_err() {
        return;
    }
    let mut done = header.offset;
    let mut reported = done;
    let mut buf = vec![0u8; CHUNK];
    loop {
        // Cancelado no meio: para de receber (quem manda recebe o "pare") e apaga o que chegou.
        if !lock(&inner.downloads).contains_key(&header.id) {
            let _ = recv.stop(0u32.into());
            close(file).await;
            let _ = tokio::fs::remove_file(&part).await;
            return;
        }
        match recv.read(&mut buf).await {
            Ok(Some(n)) => {
                if file.write_all(&buf[..n]).await.is_err() {
                    return;
                }
                done += n as u64;
                if done - reported >= PROGRESS_EVERY {
                    reported = done;
                    inner.progress(&friend, &header.id, |f| f.done = done);
                }
            }
            Ok(None) if done < size => {
                // Quem manda fechou antes do fim: o arquivo não está mais lá.
                close(file).await;
                let _ = tokio::fs::remove_file(&part).await;
                lock(&inner.downloads).remove(&header.id);
                inner.progress(&friend, &header.id, |f| {
                    f.done = 0;
                    f.gone = true;
                });
                return;
            }
            Ok(None) => break,
            // Caiu no meio: o `.part` fica, e a volta pede a partir daqui.
            Err(_) => {
                let _ = file.flush().await;
                inner.progress(&friend, &header.id, |f| f.done = done);
                return;
            }
        }
    }
    if file.flush().await.is_err() || done != size {
        return;
    }
    close(file).await;
    if tokio::fs::rename(&part, &download.dest).await.is_ok() {
        lock(&inner.downloads).remove(&header.id);
        let dest = download.dest.to_string_lossy().into_owned();
        inner.progress(&friend, &header.id, |f| {
            f.done = size;
            f.path = Some(dest);
        });
    }
}

/// Fecha o arquivo de verdade antes de apagar ou renomear. Um `tokio::fs::File` descartado pode
/// continuar aberto por um instante numa thread de bloqueio, e no Windows um arquivo aberto não é
/// apagado nem renomeado: o `.part` ficava para trás (o teste
/// `arquivo_apagado_por_quem_mandou_avisa_em_vez_de_travar` falhava de vez em quando).
async fn close(file: tokio::fs::File) {
    drop(file.into_std().await);
}

/// Manda o arquivo pedido, de `offset` até o fim, num stream só dele.
async fn send_file(inner: Arc<Inner>, friend: String, id: String, path: PathBuf, offset: u64) {
    let conn = lock(&inner.links).get(&friend).map(|l| l.conn.clone());
    let Some(conn) = conn else { return };
    let Ok(mut send) = conn.open_uni().await else { return };
    // O arquivo sumiu (apagado ou movido): o stream fecha sem nada, e quem pediu entende que não
    // existe mais. Antes, sem resposta nenhuma, o download dele ficava parado em 0% para sempre.
    let opened = match tokio::fs::File::open(&path).await {
        Ok(mut file) => {
            let size = file.metadata().await.map_or(0, |m| m.len());
            (offset <= size && file.seek(std::io::SeekFrom::Start(offset)).await.is_ok()).then_some((file, size))
        }
        Err(_) => None,
    };
    let Some((mut file, size)) = opened else {
        if write_frame(&mut send, &FileHeader { id, offset }).await.is_ok() {
            let _ = send.finish();
        }
        return;
    };
    if write_frame(&mut send, &FileHeader { id: id.clone(), offset })
        .await
        .is_err()
    {
        return;
    }
    let mut buf = vec![0u8; CHUNK];
    let mut done = offset;
    let mut reported = done;
    loop {
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if send.write_all(&buf[..n]).await.is_err() {
                    return;
                }
                done += n as u64;
                if done - reported >= PROGRESS_EVERY {
                    reported = done;
                    inner.progress(&friend, &id, |f| f.done = done);
                }
            }
            Err(_) => return,
        }
    }
    if send.finish().is_ok() {
        inner.progress(&friend, &id, |f| f.done = size);
    }
}

#[cfg(test)]
mod tests;
