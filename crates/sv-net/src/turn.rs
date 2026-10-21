//! TURN mínimo (RFC 8656) para quem não conecta direto (CGNAT): o repassador na nuvem empresta um
//! endereço público ("relayed address") e encaminha o que chega nele para nós, e o que mandamos,
//! para quem estiver do outro lado. Ver `docs/adr/0014-conexao-em-camadas.md`.
//!
//! Só o lado que **aceita** a conexão precisa disso: ele publica o endereço emprestado e quem
//! conecta manda pacotes UDP comuns para lá (de saída, o que atravessa qualquer NAT).
//!
//! O QUIC desse lado roda num [`TurnSocket`]: um socket UDP próprio (separado do caminho direto,
//! que continua com GSO e sem cabeçalho extra) que embrulha cada pacote numa *Send indication* e
//! desembrulha as *Data indications*. Para o QUIC, o pacote chega do endereço real de quem conectou.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use quinn::udp::{RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, UdpPoller};
use tokio::sync::Notify;

use crate::stun::{MAGIC_COOKIE, decode_address};

const ALLOCATE: u16 = 0x0003;
const ALLOCATE_OK: u16 = 0x0103;
const ALLOCATE_ERROR: u16 = 0x0113;
const REFRESH: u16 = 0x0004;
const REFRESH_ERROR: u16 = 0x0114;
const CREATE_PERMISSION: u16 = 0x0008;
const CREATE_PERMISSION_ERROR: u16 = 0x0118;
const SEND_INDICATION: u16 = 0x0016;
const DATA_INDICATION: u16 = 0x0017;

const ATTR_USERNAME: u16 = 0x0006;
const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
const ATTR_ERROR_CODE: u16 = 0x0009;
const ATTR_LIFETIME: u16 = 0x000D;
const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_DATA: u16 = 0x0013;
const ATTR_REALM: u16 = 0x0014;
const ATTR_NONCE: u16 = 0x0015;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;

/// Protocolo UDP no REQUESTED-TRANSPORT.
const TRANSPORT_UDP: u8 = 17;
/// Quanto a alocação dura sem renovar (o servidor pode dar menos).
const LIFETIME_S: u32 = 600;
/// De quanto em quanto tempo renovar a alocação e as permissões (as permissões valem 5 minutos).
const REFRESH_EVERY: Duration = Duration::from_secs(4 * 60);
/// Maior pacote que embrulhamos: o QUIC usa até 1452 bytes; o cabeçalho da indicação soma 36.
const MAX_WRAPPED: usize = 1600;

/// Um repassador TURN com credenciais temporárias (o Worker gera e manda ao entrar na sala).
#[derive(Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub addr: SocketAddr,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for TurnServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A senha não vai para o registro.
        f.debug_struct("TurnServer")
            .field("addr", &self.addr)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl TurnServer {
    /// Escolhe a primeira URL `turn:` por UDP (o que o QUIC precisa) e resolve o endereço IPv4.
    pub fn from_urls(urls: &[String], username: &str, password: &str) -> Option<Self> {
        let addr = urls.iter().find_map(|url| {
            let rest = url.strip_prefix("turn:")?;
            let (host_port, query) = rest.split_once('?').unwrap_or((rest, ""));
            if !query.is_empty() && !query.contains("transport=udp") {
                return None;
            }
            let host_port = if host_port.contains(':') {
                host_port.to_owned()
            } else {
                format!("{host_port}:3478")
            };
            host_port.to_socket_addrs().ok()?.find(SocketAddr::is_ipv4)
        })?;
        Some(Self {
            addr,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }
}

/// Erro ao pedir o endereço emprestado.
#[derive(Debug)]
pub enum TurnError {
    /// O repassador não respondeu.
    Timeout,
    /// Recusou (credencial vencida, sem cota...), com o código e o motivo.
    Rejected(u16, String),
    Io(io::Error),
}

impl fmt::Display for TurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "o repassador não respondeu"),
            Self::Rejected(code, reason) => write!(f, "o repassador recusou ({code} {reason})"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TurnError {}

impl From<io::Error> for TurnError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// --- Codificação -------------------------------------------------------------------------------

/// Monta uma mensagem STUN/TURN com atributos (para pedidos de controle, que são raros).
struct Message {
    buf: Vec<u8>,
}

impl Message {
    fn new(kind: u16, txid: [u8; 12]) -> Self {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(&kind.to_be_bytes());
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        buf.extend_from_slice(&txid);
        Self { buf }
    }

    fn attr(mut self, kind: u16, value: &[u8]) -> Self {
        self.buf.extend_from_slice(&kind.to_be_bytes());
        self.buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.buf.extend_from_slice(value);
        self.buf.resize(self.buf.len() + (4 - value.len() % 4) % 4, 0);
        self.set_len(self.buf.len() - 20);
        self
    }

    fn set_len(&mut self, len: usize) {
        self.buf[2..4].copy_from_slice(&(len as u16).to_be_bytes());
    }

    /// Fecha com MESSAGE-INTEGRITY (HMAC-SHA1 com a chave longa da credencial).
    fn sign(mut self, key: &[u8; 16]) -> Vec<u8> {
        // O comprimento já conta o atributo de integridade (24 bytes) antes de calcular.
        let len = self.buf.len() - 20 + 24;
        self.set_len(len);
        let hmac = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
        let tag = ring::hmac::sign(&hmac, &self.buf);
        self.buf.extend_from_slice(&ATTR_MESSAGE_INTEGRITY.to_be_bytes());
        self.buf.extend_from_slice(&20u16.to_be_bytes());
        self.buf.extend_from_slice(tag.as_ref());
        self.buf
    }

    fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Chave longa da credencial: MD5("usuário:realm:senha").
fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    let mut md5 = Md5::new();
    md5.update(format!("{username}:{realm}:{password}").as_bytes());
    md5.finalize().into()
}

/// Endereço no formato XOR (XOR-PEER-ADDRESS e afins). Só IPv4: o resto do app também é IPv4.
fn xor_address(addr: SocketAddr) -> Option<[u8; 8]> {
    let IpAddr::V4(ip) = addr.ip() else { return None };
    let cookie = MAGIC_COOKIE.to_be_bytes();
    let mut v = [0u8; 8];
    v[1] = 0x01;
    v[2..4].copy_from_slice(&(addr.port() ^ (MAGIC_COOKIE >> 16) as u16).to_be_bytes());
    for (i, (b, c)) in ip.octets().iter().zip(cookie).enumerate() {
        v[4 + i] = b ^ c;
    }
    Some(v)
}

/// Embrulha `data` numa Send indication para `peer`, dentro de `out`. Sem alocar: é o caminho de
/// cada pacote. Devolve o tamanho, ou `None` se não couber (ou o endereço não for IPv4).
pub(crate) fn encode_send(out: &mut [u8], txid: [u8; 12], peer: SocketAddr, data: &[u8]) -> Option<usize> {
    let peer = xor_address(peer)?;
    let padded = data.len().div_ceil(4) * 4;
    let total = 20 + 12 + 4 + padded;
    if total > out.len() || data.len() > u16::MAX as usize {
        return None;
    }
    out[0..2].copy_from_slice(&SEND_INDICATION.to_be_bytes());
    out[2..4].copy_from_slice(&((total - 20) as u16).to_be_bytes());
    out[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    out[8..20].copy_from_slice(&txid);
    out[20..22].copy_from_slice(&ATTR_XOR_PEER_ADDRESS.to_be_bytes());
    out[22..24].copy_from_slice(&8u16.to_be_bytes());
    out[24..32].copy_from_slice(&peer);
    out[32..34].copy_from_slice(&ATTR_DATA.to_be_bytes());
    out[34..36].copy_from_slice(&(data.len() as u16).to_be_bytes());
    out[36..36 + data.len()].copy_from_slice(data);
    out[36 + data.len()..total].fill(0);
    Some(total)
}

/// O que chegou do repassador.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Incoming<'a> {
    /// Um pacote de `peer` para nós (Data indication).
    Data { peer: SocketAddr, data: &'a [u8] },
    /// Resposta a um pedido nosso.
    Response(Response),
    /// Outra coisa (ignorar).
    Other,
}

/// Uma resposta (sucesso ou erro) do repassador, com o que interessa.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Response {
    pub kind: u16,
    pub txid: [u8; 12],
    pub error: Option<(u16, String)>,
    pub realm: Option<String>,
    pub nonce: Option<String>,
    pub relayed: Option<SocketAddr>,
    pub lifetime: Option<u32>,
}

/// Lê uma mensagem do repassador.
pub(crate) fn parse(msg: &[u8]) -> Incoming<'_> {
    if msg.len() < 20 || msg[0] & 0xC0 != 0 || msg[4..8] != MAGIC_COOKIE.to_be_bytes() {
        return Incoming::Other;
    }
    let kind = u16::from_be_bytes([msg[0], msg[1]]);
    let len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
    let Some(attrs) = msg.get(20..20 + len) else {
        return Incoming::Other;
    };
    let txid: [u8; 12] = msg[8..20].try_into().unwrap_or_default();
    let mut response = Response {
        kind,
        txid,
        ..Response::default()
    };
    let mut peer = None;
    let mut data = None;
    let mut i = 0;
    while i + 4 <= attrs.len() {
        let attr = u16::from_be_bytes([attrs[i], attrs[i + 1]]);
        let size = u16::from_be_bytes([attrs[i + 2], attrs[i + 3]]) as usize;
        let Some(value) = attrs.get(i + 4..i + 4 + size) else {
            break;
        };
        match attr {
            ATTR_XOR_PEER_ADDRESS => peer = decode_address(value, Some(txid)),
            ATTR_DATA => data = Some(value),
            ATTR_XOR_RELAYED_ADDRESS => response.relayed = decode_address(value, Some(txid)),
            ATTR_LIFETIME if size == 4 => {
                response.lifetime = Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            ATTR_REALM => response.realm = Some(String::from_utf8_lossy(value).into_owned()),
            ATTR_NONCE => response.nonce = Some(String::from_utf8_lossy(value).into_owned()),
            ATTR_ERROR_CODE if size >= 4 => {
                let code = u16::from(value[2] & 0x07) * 100 + u16::from(value[3]);
                response.error = Some((code, String::from_utf8_lossy(&value[4..]).into_owned()));
            }
            _ => {}
        }
        i += 4 + size.div_ceil(4) * 4;
    }
    match (kind, peer, data) {
        (DATA_INDICATION, Some(peer), Some(data)) => Incoming::Data { peer, data },
        (DATA_INDICATION, ..) => Incoming::Other,
        // Respostas: classe "sucesso" (0x0100) ou "erro" (0x0110).
        _ if kind & 0x0110 != 0 => Incoming::Response(response),
        _ => Incoming::Other,
    }
}

fn txid(counter: &AtomicU64) -> [u8; 12] {
    let n = counter.fetch_add(1, Ordering::Relaxed);
    let mut t = [0u8; 12];
    // Prefixo aleatório por processo + contador: único e barato (não precisa ser secreto).
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u32)
        ^ std::process::id();
    t[0..4].copy_from_slice(&seed.to_be_bytes());
    t[4..12].copy_from_slice(&n.to_be_bytes());
    t
}

// --- Alocação ----------------------------------------------------------------------------------

/// Credencial em uso (o nonce muda quando o repassador pede). Sem `Debug`: a chave não vai para
/// o registro.
#[derive(Clone)]
struct Auth {
    username: String,
    realm: String,
    nonce: String,
    key: [u8; 16],
}

impl Auth {
    fn request(&self, kind: u16, txid: [u8; 12]) -> Message {
        Message::new(kind, txid)
            .attr(ATTR_USERNAME, self.username.as_bytes())
            .attr(ATTR_REALM, self.realm.as_bytes())
            .attr(ATTR_NONCE, self.nonce.as_bytes())
    }
}

/// Pede o endereço emprestado (Allocate com o desafio de credencial). Bloqueia até ~3 s.
fn allocate(socket: &UdpSocket, server: &TurnServer, ids: &AtomicU64) -> Result<(SocketAddr, Auth), TurnError> {
    let previous = socket.read_timeout().ok().flatten();
    socket.set_read_timeout(Some(Duration::from_millis(400)))?;
    let transport = [TRANSPORT_UDP, 0, 0, 0];
    let lifetime = LIFETIME_S.to_be_bytes();

    // 1) Sem credencial: o repassador responde 401 com o realm e o nonce.
    let mut auth: Option<Auth> = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buf = [0u8; 1500];
    let result = loop {
        if Instant::now() > deadline {
            break Err(TurnError::Timeout);
        }
        let id = txid(ids);
        let request = match &auth {
            None => Message::new(ALLOCATE, id)
                .attr(ATTR_REQUESTED_TRANSPORT, &transport)
                .finish(),
            Some(a) => a
                .request(ALLOCATE, id)
                .attr(ATTR_REQUESTED_TRANSPORT, &transport)
                .attr(ATTR_LIFETIME, &lifetime)
                .sign(&a.key),
        };
        socket.send_to(&request, server.addr)?;
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        if from != server.addr {
            continue;
        }
        let Incoming::Response(r) = parse(&buf[..n]) else {
            continue;
        };
        if r.txid != id {
            continue;
        }
        match (r.kind, r.error) {
            (ALLOCATE_OK, _) => match (r.relayed, &auth) {
                (Some(relayed), Some(a)) => break Ok((relayed, a.clone())),
                _ => break Err(TurnError::Rejected(0, "resposta sem endereço".into())),
            },
            // 401 (primeira vez) ou 438 (nonce vencido): tenta de novo com o desafio novo.
            (ALLOCATE_ERROR, Some((401 | 438, _))) if r.nonce.is_some() => {
                let realm = r
                    .realm
                    .or_else(|| auth.as_ref().map(|a| a.realm.clone()))
                    .unwrap_or_default();
                auth = Some(Auth {
                    key: long_term_key(&server.username, &realm, &server.password),
                    username: server.username.clone(),
                    realm,
                    nonce: r.nonce.unwrap_or_default(),
                });
            }
            (_, Some((code, reason))) => break Err(TurnError::Rejected(code, reason)),
            _ => {}
        }
    };
    let _ = socket.set_read_timeout(previous);
    result
}

// --- Socket para o QUIC ------------------------------------------------------------------------

/// Socket UDP que fala com o QUIC como se fosse direto, mas passa tudo pelo repassador.
pub struct TurnSocket {
    io: tokio::net::UdpSocket,
    server: SocketAddr,
    relayed: SocketAddr,
    ids: AtomicU64,
    auth: Mutex<Auth>,
    /// IPs autorizados a mandar para o nosso endereço emprestado (quem pediu para conectar).
    permitted: Mutex<HashSet<IpAddr>>,
    /// Acorda a tarefa de manutenção (nonce trocado ou socket fechado).
    wake: Arc<Notify>,
    closed: AtomicBool,
}

impl fmt::Debug for TurnSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TurnSocket")
            .field("server", &self.server)
            .field("relayed", &self.relayed)
            .finish_non_exhaustive()
    }
}

impl TurnSocket {
    /// Abre um socket só para o repassador e pede o endereço emprestado. Precisa rodar dentro de um
    /// runtime tokio (a manutenção da alocação roda em segundo plano até [`TurnSocket::close`]).
    pub async fn open(server: TurnServer) -> Result<Arc<Self>, TurnError> {
        let ids = AtomicU64::new(1);
        let server_addr = server.addr;
        let (socket, relayed, auth, ids) = tokio::task::spawn_blocking(move || {
            let socket = UdpSocket::bind(("0.0.0.0", 0))?;
            let (relayed, auth) = allocate(&socket, &server, &ids)?;
            Ok::<_, TurnError>((socket, relayed, auth, ids))
        })
        .await
        .map_err(|e| TurnError::Io(io::Error::other(e)))??;
        socket.set_nonblocking(true)?;
        let wake = Arc::new(Notify::new());
        let this = Arc::new(Self {
            io: tokio::net::UdpSocket::from_std(socket)?,
            server: server_addr,
            relayed,
            ids,
            auth: Mutex::new(auth),
            permitted: Mutex::new(HashSet::new()),
            wake: wake.clone(),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(maintain(Arc::downgrade(&this), wake));
        Ok(this)
    }

    /// O endereço público emprestado: é o que mandamos para quem vai conectar.
    pub fn relayed(&self) -> SocketAddr {
        self.relayed
    }

    /// Deixa esses IPs mandarem para o nosso endereço emprestado (o repassador descarta o resto).
    pub fn permit(&self, peers: &[SocketAddr]) {
        let mut set = lock(&self.permitted);
        let before = set.len();
        set.extend(peers.iter().map(SocketAddr::ip).filter(|ip| is_public(*ip)));
        if set.len() != before {
            drop(set);
            self.send_permissions();
        }
    }

    /// Devolve o endereço ao repassador (LIFETIME 0) e para a manutenção.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::Relaxed) {
            return;
        }
        let auth = lock(&self.auth).clone();
        let msg = auth
            .request(REFRESH, txid(&self.ids))
            .attr(ATTR_LIFETIME, &0u32.to_be_bytes())
            .sign(&auth.key);
        let _ = self.io.try_send_to(&msg, self.server);
        self.wake.notify_one();
    }

    fn send_refresh(&self) {
        let auth = lock(&self.auth).clone();
        let msg = auth
            .request(REFRESH, txid(&self.ids))
            .attr(ATTR_LIFETIME, &LIFETIME_S.to_be_bytes())
            .sign(&auth.key);
        let _ = self.io.try_send_to(&msg, self.server);
    }

    fn send_permissions(&self) {
        let peers: Vec<IpAddr> = lock(&self.permitted).iter().copied().collect();
        if peers.is_empty() {
            return;
        }
        let auth = lock(&self.auth).clone();
        let mut msg = auth.request(CREATE_PERMISSION, txid(&self.ids));
        for ip in peers {
            // A porta não importa na permissão (vale o IP).
            if let Some(v) = xor_address(SocketAddr::new(ip, 0)) {
                msg = msg.attr(ATTR_XOR_PEER_ADDRESS, &v);
            }
        }
        let _ = self.io.try_send_to(&msg.sign(&auth.key), self.server);
    }

    /// Resposta a um pedido de manutenção: se o nonce venceu (438), guarda o novo e repete logo.
    fn on_response(&self, r: Response) {
        if matches!(r.kind, REFRESH_ERROR | CREATE_PERMISSION_ERROR)
            && matches!(r.error, Some((438, _)))
            && let Some(nonce) = r.nonce
        {
            lock(&self.auth).nonce = nonce;
            self.wake.notify_one();
        }
    }
}

impl Drop for TurnSocket {
    fn drop(&mut self) {
        self.close();
    }
}

/// Mantém a alocação e as permissões vivas enquanto o socket existir. Não segura o socket entre
/// as rodadas: quando o QUIC solta ele, a tarefa termina sozinha.
async fn maintain(socket: std::sync::Weak<TurnSocket>, wake: Arc<Notify>) {
    let mut last_refresh = Instant::now();
    loop {
        let woke = tokio::time::timeout(Duration::from_secs(30), wake.notified())
            .await
            .is_ok();
        let Some(s) = socket.upgrade() else { return };
        if s.closed.load(Ordering::Relaxed) {
            return;
        }
        if woke || last_refresh.elapsed() >= REFRESH_EVERY {
            s.send_refresh();
            s.send_permissions();
            last_refresh = Instant::now();
        }
    }
}

/// O IP é alcançável pela internet (endereços da rede local não fazem sentido no repassador).
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()),
        IpAddr::V6(_) => false,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Avisa o QUIC quando dá para mandar de novo (sem alocar: só consulta o socket).
#[derive(Debug)]
struct Writable(Arc<TurnSocket>);

impl UdpPoller for Writable {
    fn poll_writable(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.0.io.poll_send_ready(cx)
    }
}

impl AsyncUdpSocket for TurnSocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(Writable(self))
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        let mut out = [0u8; MAX_WRAPPED];
        // Sem GSO neste socket (max_transmit_segments = 1): um pacote por vez.
        let n = encode_send(&mut out, txid(&self.ids), transmit.destination, transmit.contents)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pacote grande demais para o repassador"))?;
        self.io.try_send_to(&out[..n], self.server).map(|_| ())
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let mut raw = [0u8; MAX_WRAPPED + 200];
        loop {
            ready!(self.io.poll_recv_ready(cx))?;
            let (n, from) = match self.io.try_recv_from(&mut raw) {
                Ok(r) => r,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                // ICMP "porta inalcançável" no Windows vira erro de leitura: ignora e segue.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => return Poll::Ready(Err(e)),
            };
            if from != self.server {
                continue;
            }
            match parse(&raw[..n]) {
                Incoming::Data { peer, data } => {
                    let (Some(buf), Some(m)) = (bufs.first_mut(), meta.first_mut()) else {
                        return Poll::Ready(Ok(0));
                    };
                    let len = data.len().min(buf.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    *m = RecvMeta {
                        addr: peer,
                        len,
                        stride: len,
                        ..RecvMeta::default()
                    };
                    m.ecn = None;
                    m.dst_ip = None;
                    return Poll::Ready(Ok(1));
                }
                Incoming::Response(r) => self.on_response(r),
                Incoming::Other => {}
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.local_addr()
    }
}

#[cfg(test)]
mod tests;
