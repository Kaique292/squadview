//! O ponto QUIC de cada membro: um socket UDP só, usado para descobrir o endereço público (STUN),
//! "furar" o NAT na direção do outro e conectar/receber conexões.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Connection, Endpoint, EndpointConfig, ServerConfig, TokioRuntime, TransportConfig, VarInt};
use tokio::sync::mpsc;

use crate::cert::fingerprint;
use crate::cert::{Allowed, AllowedClients, ExpectFingerprint, Identity, provider};
use crate::turn::{TurnError, TurnServer, TurnSocket};
use crate::{ALPN, VOICE_ALPN, stun};

/// Quanto tempo tentar conectar antes de desistir.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// Quanto esperar a conexão direta antes de tentar também pelo repassador (ADR 0014): direto é
/// melhor (sem custo e sem desvio), então o repassador só entra se o direto não fechar logo.
pub const RELAY_AFTER: Duration = Duration::from_millis(1500);
/// Quanto quem aceita segura uma conexão nova antes de entregar. Quem conecta tenta vários
/// endereços ao mesmo tempo (IPv4, IPv6, rede local, repassador) e fica com o primeiro; às vezes
/// outro também completa e é fechado logo em seguida. Se ele fosse entregue, a voz trocava a
/// conexão boa pela que ia fechar e caía.
const SETTLE: Duration = Duration::from_millis(250);
/// Tamanho de pacote pelo repassador: cabe com o cabeçalho do TURN sem fragmentar.
const RELAY_MTU: u16 = 1200;
/// Por quanto tempo mandar pacotes de "furo" depois de combinar a conexão.
const PUNCH_FOR: Duration = Duration::from_secs(4);
/// De quanto em quanto tempo "lembrar" o roteador da nossa porta. Sem tráfego, muitos roteadores
/// esquecem o mapeamento em 30 a 120 s e o endereço que mandamos aos amigos deixa de valer.
const NAT_KEEPALIVE: Duration = Duration::from_secs(15);

/// Como o roteador escolhe a porta pública do nosso socket (visto por dois servidores STUN).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NatMapping {
    /// Não deu para saber (só um servidor respondeu, ou nenhum).
    Unknown,
    /// A mesma porta para qualquer destino: o "furo" funciona.
    EndpointIndependent,
    /// Uma porta diferente para cada destino (NAT simétrico, comum em CGNAT): conexão direta
    /// quase nunca funciona; só a ponte por outro membro resolve.
    Symmetric,
}

impl NatMapping {
    /// Compara o endereço público visto por dois servidores diferentes.
    pub fn from_observations(a: Option<SocketAddr>, b: Option<SocketAddr>) -> Self {
        match (a, b) {
            (Some(a), Some(b)) if a == b => Self::EndpointIndependent,
            (Some(_), Some(_)) => Self::Symmetric,
            _ => Self::Unknown,
        }
    }
}

/// Por que a conexão não saiu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// Nenhum endereço respondeu: provavelmente os dois estão atrás de NAT restritivo (CGNAT).
    Unreachable,
    Other(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(
                f,
                "não deu para conectar direto com esse amigo (a internet de um de vocês bloqueia conexões \
                 diretas, comum em CGNAT). A ponte por outro membro da sala chega numa próxima versão."
            ),
            Self::Other(e) => write!(f, "erro na conexão: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// Para que serve uma conexão: assistir uma transmissão ou a voz da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Stream,
    Voice,
}

impl Purpose {
    fn alpn(self) -> &'static [u8] {
        match self {
            Self::Stream => ALPN,
            Self::Voice => VOICE_ALPN,
        }
    }
}

/// Uma conexão que alguém abriu com a gente (já autorizada pela fingerprint).
#[derive(Debug)]
pub struct Accepted {
    pub conn: Connection,
    pub purpose: Purpose,
    /// Fingerprint do certificado de quem conectou (diz quem é).
    pub fingerprint: Option<String>,
}

/// O ponto QUIC pelo repassador (ADR 0014): só aceita conexões, que chegam pelo endereço emprestado.
#[derive(Debug)]
struct Relay {
    socket: Arc<TurnSocket>,
    endpoint: Endpoint,
}

impl Relay {
    fn close(&self) {
        self.endpoint.close(VarInt::from_u32(0), b"tchau");
        self.socket.close();
    }
}

/// O ponto QUIC por IPv6, quando o PC tem endereço IPv6 global. No IPv6 não há NAT (nem CGNAT):
/// cada PC tem endereço público, e o "furo" dos dois lados abre o firewall do roteador. É a saída
/// grátis para quem está atrás de CGNAT no IPv4 (ADR 0014).
#[derive(Debug)]
struct V6 {
    endpoint: Endpoint,
    punch: UdpSocket,
}

/// Um membro na rede.
#[derive(Debug)]
pub struct Peer {
    endpoint: Endpoint,
    v6: Option<V6>,
    /// Conexões aceitas pelo caminho direto e pelo repassador, numa fila só.
    accepted: tokio::sync::Mutex<mpsc::UnboundedReceiver<Accepted>>,
    accepted_tx: mpsc::UnboundedSender<Accepted>,
    relay: Mutex<Option<Relay>>,
    punch: UdpSocket,
    identity: Arc<Identity>,
    allowed: Allowed,
    candidates: Vec<SocketAddr>,
    nat: NatMapping,
    /// Liga o "mantenha vivo" do NAT; desliga quando o ponto fecha.
    keepalive: Arc<AtomicBool>,
}

impl Peer {
    /// Abre o socket, descobre o endereço público e começa a aceitar conexões autorizadas.
    /// Precisa rodar dentro de um runtime tokio.
    pub async fn bind(stun_servers: &[&'static str]) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        let port = socket.local_addr()?.port();
        let servers: Vec<&'static str> = stun_servers.to_vec();
        let (socket, public, nat) = tokio::task::spawn_blocking(move || {
            // Pergunta a dois servidores separados: se cada um vê uma porta, o NAT é simétrico.
            let first = stun::discover(&socket, &servers[..servers.len().min(1)], Duration::from_millis(1500));
            let second = servers
                .get(1..2)
                .and_then(|s| stun::discover(&socket, s, Duration::from_millis(1000)));
            let public = first.or(second);
            (socket, public, NatMapping::from_observations(first, second))
        })
        .await
        .map_err(io::Error::other)?;
        let mut candidates = candidates(public, local_ip(), port);

        let identity = Arc::new(Identity::generate());
        let allowed = Allowed::default();
        let punch = socket.try_clone()?;
        let keepalive = Arc::new(AtomicBool::new(true));
        if let Some(server) = stun_servers.first() {
            spawn_nat_keepalive(punch.try_clone()?, server, keepalive.clone());
        }
        socket.set_nonblocking(true)?;
        let endpoint = Endpoint::new(
            EndpointConfig::default(),
            Some(server_config(&identity, &allowed, false)?),
            socket,
            Arc::new(TokioRuntime),
        )?;
        let (accepted_tx, accepted) = mpsc::unbounded_channel();
        tokio::spawn(accept_loop(endpoint.clone(), accepted_tx.clone()));
        // IPv6, se houver: um socket à parte, para o caminho IPv4 continuar igual.
        let v6 = match global_ipv6() {
            Some(ip) => match bind_v6(&identity, &allowed) {
                Ok((v6, port)) => {
                    tokio::spawn(accept_loop(v6.endpoint.clone(), accepted_tx.clone()));
                    // Logo depois do público IPv4: os dois são tentados ao mesmo tempo.
                    candidates.insert(1.min(candidates.len()), SocketAddr::new(IpAddr::V6(ip), port));
                    Some(v6)
                }
                Err(_) => None,
            },
            None => None,
        };
        Ok(Self {
            endpoint,
            v6,
            accepted: tokio::sync::Mutex::new(accepted),
            accepted_tx,
            relay: Mutex::new(None),
            punch,
            identity,
            allowed,
            candidates,
            nat,
            keepalive,
        })
    }

    /// Como o nosso roteador mapeia portas (ver [`NatMapping`]).
    pub fn nat(&self) -> NatMapping {
        self.nat
    }

    /// Endereços por onde os outros podem nos alcançar (público, rede local e o próprio PC).
    pub fn candidates(&self) -> &[SocketAddr] {
        &self.candidates
    }

    /// Pede um endereço no repassador TURN e passa a aceitar conexões por ele também. Devolve o
    /// endereço emprestado (vai no `relay` das respostas). Chamar de novo troca o repassador.
    pub async fn enable_relay(&self, server: TurnServer) -> Result<SocketAddr, TurnError> {
        let socket = TurnSocket::open(server).await?;
        let relayed = socket.relayed();
        let endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            Some(server_config(&self.identity, &self.allowed, true)?),
            socket.clone(),
            Arc::new(TokioRuntime),
        )?;
        tokio::spawn(accept_loop(endpoint.clone(), self.accepted_tx.clone()));
        let previous = lock(&self.relay).replace(Relay { socket, endpoint });
        if let Some(old) = previous {
            old.close();
        }
        Ok(relayed)
    }

    /// O nosso endereço no repassador, se houver.
    pub fn relay_addr(&self) -> Option<SocketAddr> {
        lock(&self.relay).as_ref().map(|r| r.socket.relayed())
    }

    /// Quem vai conectar em nós pelo repassador precisa de permissão lá (por IP). Chamar com os
    /// endereços que a pessoa mandou, antes de responder com o nosso `relay`.
    pub fn permit_relay(&self, peers: &[SocketAddr]) {
        if let Some(r) = lock(&self.relay).as_ref() {
            r.socket.permit(peers);
        }
    }

    pub fn fingerprint(&self) -> &str {
        &self.identity.fingerprint
    }

    /// Quem pode conectar em nós (quem pediu para assistir).
    pub fn allowed(&self) -> &Allowed {
        &self.allowed
    }

    /// Manda pacotes curtos para os endereços do outro durante alguns segundos. Isso abre o caminho
    /// no nosso roteador para os pacotes dele entrarem (hole punching). O QUIC ignora esses pacotes.
    pub fn punch(&self, targets: &[SocketAddr]) {
        let Ok(v4) = self.punch.try_clone() else { return };
        // IPv6: o "furo" abre o firewall do roteador para o outro (não há NAT para atravessar).
        let v6 = self.v6.as_ref().and_then(|v| v.punch.try_clone().ok());
        let targets = targets.to_vec();
        tokio::task::spawn_blocking(move || {
            let until = std::time::Instant::now() + PUNCH_FOR;
            while std::time::Instant::now() < until {
                for t in &targets {
                    let socket = if t.is_ipv6() { v6.as_ref() } else { Some(&v4) };
                    if let Some(socket) = socket {
                        let _ = socket.send_to(b"svpunch", t);
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }

    /// Conecta em quem tem a fingerprint `expected` para assistir, tentando todos os endereços ao
    /// mesmo tempo (e o repassador do outro, se o direto não fechar logo).
    pub async fn connect(
        &self,
        targets: &[SocketAddr],
        relay: Option<SocketAddr>,
        expected: &str,
    ) -> Result<Connection, ConnectError> {
        self.connect_for(targets, relay, expected, Purpose::Stream).await
    }

    /// Como [`Peer::connect`], para o propósito escolhido (vídeo ou voz). Em camadas (ADR 0014):
    /// primeiro os endereços diretos; depois de [`RELAY_AFTER`] sem sucesso, também o endereço do
    /// outro no repassador. Fica com a primeira que fechar.
    pub async fn connect_for(
        &self,
        targets: &[SocketAddr],
        relay: Option<SocketAddr>,
        expected: &str,
        purpose: Purpose,
    ) -> Result<Connection, ConnectError> {
        self.punch(targets);
        let direct =
            client_config(&self.identity, expected, purpose, false).map_err(|e| ConnectError::Other(e.to_string()))?;
        let mut attempts = tokio::task::JoinSet::new();
        for &target in targets {
            // Cada família pelo seu socket; endereço IPv6 sem IPv6 aqui: não dá para tentar.
            let endpoint = match (target.is_ipv6(), &self.v6) {
                (false, _) => &self.endpoint,
                (true, Some(v6)) => &v6.endpoint,
                (true, None) => continue,
            };
            let Ok(connecting) = endpoint.connect_with(direct.clone(), target, "squadview") else {
                continue;
            };
            attempts.spawn(async move { connecting.await.ok() });
        }
        if let Some(relay) = relay {
            let config = client_config(&self.identity, expected, purpose, true)
                .map_err(|e| ConnectError::Other(e.to_string()))?;
            let endpoint = self.endpoint.clone();
            attempts.spawn(async move {
                tokio::time::sleep(RELAY_AFTER).await;
                endpoint.connect_with(config, relay, "squadview").ok()?.await.ok()
            });
        }
        let result = tokio::time::timeout(CONNECT_TIMEOUT, async {
            while let Some(attempt) = attempts.join_next().await {
                if let Ok(Some(conn)) = attempt {
                    return Some(conn);
                }
            }
            None
        })
        .await;
        attempts.abort_all();
        match result {
            Ok(Some(conn)) => Ok(conn),
            _ => Err(ConnectError::Unreachable),
        }
    }

    /// Espera a próxima conexão autorizada (direta ou pelo repassador). `None` quando o ponto fecha.
    pub async fn accept(&self) -> Option<Accepted> {
        self.accepted.lock().await.recv().await
    }

    pub fn close(&self) {
        self.keepalive.store(false, Ordering::Relaxed);
        self.endpoint.close(VarInt::from_u32(0), b"tchau");
        if let Some(v6) = &self.v6 {
            v6.endpoint.close(VarInt::from_u32(0), b"tchau");
        }
        if let Some(relay) = lock(&self.relay).take() {
            relay.close();
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.keepalive.store(false, Ordering::Relaxed);
        if let Some(relay) = lock(&self.relay).take() {
            relay.close();
        }
    }
}

/// Aceita as conexões de um ponto (direto ou repassador) e põe na fila do [`Peer`]. Termina quando
/// o ponto fecha ou ninguém mais lê a fila.
async fn accept_loop(endpoint: Endpoint, accepted: mpsc::UnboundedSender<Accepted>) {
    while let Some(incoming) = endpoint.accept().await {
        let tx = accepted.clone();
        // Cada handshake corre à parte: um lento (pelo repassador) não segura os outros.
        tokio::spawn(async move {
            // Certificado não autorizado falha aqui, no handshake, e a conexão é descartada.
            let Ok(conn) = incoming.await else { return };
            // A tentativa que perdeu do outro lado fecha logo: essa não é entregue.
            if tokio::time::timeout(SETTLE, conn.closed()).await.is_err() {
                let _ = tx.send(Accepted {
                    purpose: purpose_of(&conn),
                    fingerprint: peer_fingerprint(&conn),
                    conn,
                });
            }
        });
        if accepted.is_closed() {
            return;
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Manda um pedido STUN (20 bytes) de tempos em tempos pelo mesmo socket do QUIC, para o
/// roteador manter a nossa porta pública. A resposta chega no QUIC, que a descarta.
fn spawn_nat_keepalive(socket: UdpSocket, server: &'static str, alive: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("sv-nat-keepalive".into())
        .spawn(move || {
            let Some(target) = std::net::ToSocketAddrs::to_socket_addrs(server)
                .ok()
                .and_then(|mut a| a.find(SocketAddr::is_ipv4))
            else {
                return;
            };
            let request = stun::binding_request([0x53; 12]);
            // Dorme em passos curtos para parar logo quando o ponto fecha.
            let step = Duration::from_millis(500);
            let mut waited = Duration::ZERO;
            while alive.load(Ordering::Relaxed) {
                std::thread::sleep(step);
                waited += step;
                if waited >= NAT_KEEPALIVE {
                    waited = Duration::ZERO;
                    let _ = socket.send_to(&request, target);
                }
            }
        });
}

/// `relayed`: pacotes pequenos e sem procurar MTU maior (o TURN soma 36 bytes a cada pacote).
fn transport(relayed: bool) -> Arc<TransportConfig> {
    let mut t = TransportConfig::default();
    if relayed {
        t.initial_mtu(RELAY_MTU);
        t.mtu_discovery_config(None);
    }
    t.max_concurrent_uni_streams(VarInt::from_u32(512));
    t.max_concurrent_bidi_streams(VarInt::from_u32(4));
    t.keep_alive_interval(Some(Duration::from_secs(4)));
    t.max_idle_timeout(Some(VarInt::from_u32(15_000).into()));
    t.datagram_receive_buffer_size(Some(1 << 20));
    // Janela grande: keyframes 1080p chegam a centenas de KB.
    t.stream_receive_window(VarInt::from_u32(8 << 20));
    t.receive_window(VarInt::from_u32(32 << 20));
    Arc::new(t)
}

fn server_config(identity: &Identity, allowed: &Allowed, relayed: bool) -> io::Result<ServerConfig> {
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .with_client_cert_verifier(AllowedClients::new(allowed.clone()))
        .with_single_cert(vec![identity.cert.clone()], identity.key())
        .map_err(io::Error::other)?;
    // Os dois propósitos; o app 0.1.0 só conhece o primeiro.
    tls.alpn_protocols = vec![ALPN.to_vec(), VOICE_ALPN.to_vec()];
    let crypto = QuicServerConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport(relayed));
    Ok(config)
}

fn client_config(identity: &Identity, expected: &str, purpose: Purpose, relayed: bool) -> io::Result<ClientConfig> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(ExpectFingerprint::new(expected.to_owned()))
        .with_client_auth_cert(vec![identity.cert.clone()], identity.key())
        .map_err(io::Error::other)?;
    tls.alpn_protocols = vec![purpose.alpn().to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport(relayed));
    Ok(config)
}

/// O ALPN que a conexão negociou diz o propósito (sem ALPN, ou desconhecido: vídeo).
fn purpose_of(conn: &Connection) -> Purpose {
    let protocol = conn
        .handshake_data()
        .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|d| d.protocol);
    if protocol.as_deref() == Some(VOICE_ALPN) {
        Purpose::Voice
    } else {
        Purpose::Stream
    }
}

/// Fingerprint do certificado que o outro lado apresentou.
fn peer_fingerprint(conn: &Connection) -> Option<String> {
    let certs = conn
        .peer_identity()?
        .downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
        .ok()?;
    certs.first().map(fingerprint)
}

/// Abre o ponto IPv6 (só IPv6: o Windows já cria o socket assim) e devolve a porta dele.
fn bind_v6(identity: &Identity, allowed: &Allowed) -> io::Result<(V6, u16)> {
    let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))?;
    let port = socket.local_addr()?.port();
    let punch = socket.try_clone()?;
    socket.set_nonblocking(true)?;
    let endpoint = Endpoint::new(
        EndpointConfig::default(),
        Some(server_config(identity, allowed, false)?),
        socket,
        Arc::new(TokioRuntime),
    )?;
    Ok((V6 { endpoint, punch }, port))
}

/// O endereço IPv6 global desta máquina (o que sai para a internet), se houver. Sem NAT, ele é o
/// próprio endereço público.
fn global_ipv6() -> Option<Ipv6Addr> {
    // `connect` em UDP não manda nada: só escolhe a interface e o endereço de origem.
    let probe = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).ok()?;
    probe
        .connect((Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888), 53))
        .ok()?;
    match probe.local_addr().ok()?.ip() {
        IpAddr::V6(ip) if is_global_v6(ip) => Some(ip),
        _ => None,
    }
}

/// Unicast global (2000::/3): nem link-local, nem ULA da rede de casa, nem loopback, nem o 6to4/
/// Teredo de compatibilidade (que não atravessam firewall de verdade).
fn is_global_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let global_unicast = s[0] & 0xE000 == 0x2000;
    let teredo = s[0] == 0x2001 && s[1] == 0;
    let documentation = s[0] == 0x2001 && s[1] == 0x0DB8;
    let six_to_four = s[0] == 0x2002;
    global_unicast && !teredo && !documentation && !six_to_four
}

/// IP desta máquina na rede local (o da interface que sai para a internet).
fn local_ip() -> Option<IpAddr> {
    // `connect` em UDP não manda nada: só escolhe a interface e o endereço de origem.
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect((Ipv4Addr::new(1, 1, 1, 1), 80)).ok()?;
    Some(probe.local_addr().ok()?.ip())
}

/// Endereços para os outros tentarem, do mais provável ao menos: público (outra rede), local
/// (mesma rede) e loopback (mesmo PC, útil para testar).
pub(crate) fn candidates(public: Option<SocketAddr>, local: Option<IpAddr>, port: u16) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::new();
    for addr in [
        public,
        local.map(|ip| SocketAddr::new(ip, port)),
        Some(SocketAddr::from((Ipv4Addr::LOCALHOST, port))),
    ]
    .into_iter()
    .flatten()
    {
        if !out.contains(&addr) && !addr.ip().is_unspecified() {
            out.push(addr);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_ipv6_global_de_verdade_vira_candidato() {
        for ip in ["2804:14c:1:2::3", "2001:4860::8888", "2600::1"] {
            assert!(is_global_v6(ip.parse().unwrap()), "{ip}");
        }
        // Link-local, rede de casa (ULA), loopback, Teredo, 6to4 e documentação: não.
        for ip in ["fe80::1", "fd00::1", "::1", "2001:0::1", "2002::1", "2001:db8::1", "::"] {
            assert!(!is_global_v6(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn nat_simetrico_e_quando_cada_servidor_ve_uma_porta() {
        let a: SocketAddr = "200.1.2.3:40000".parse().unwrap();
        let b: SocketAddr = "200.1.2.3:40007".parse().unwrap();
        assert_eq!(
            NatMapping::from_observations(Some(a), Some(a)),
            NatMapping::EndpointIndependent
        );
        assert_eq!(NatMapping::from_observations(Some(a), Some(b)), NatMapping::Symmetric);
        assert_eq!(NatMapping::from_observations(Some(a), None), NatMapping::Unknown);
        assert_eq!(NatMapping::from_observations(None, None), NatMapping::Unknown);
    }

    #[test]
    fn candidatos_em_ordem_sem_repetir() {
        let public: SocketAddr = "203.0.113.5:40000".parse().unwrap();
        let c = candidates(Some(public), Some("192.168.0.10".parse().unwrap()), 5000);
        assert_eq!(
            c,
            vec![
                public,
                "192.168.0.10:5000".parse().unwrap(),
                "127.0.0.1:5000".parse().unwrap()
            ]
        );
        // Sem STUN e sem rede: sobra o loopback.
        assert_eq!(candidates(None, None, 5000), vec!["127.0.0.1:5000".parse().unwrap()]);
        // Endereço público igual ao local (sem NAT): não repete.
        let same: SocketAddr = "192.168.0.10:5000".parse().unwrap();
        assert_eq!(candidates(Some(same), Some(same.ip()), 5000).len(), 2);
    }

    #[test]
    fn mensagem_de_cgnat_explica_o_que_fazer() {
        assert!(ConnectError::Unreachable.to_string().contains("CGNAT"));
    }
}
