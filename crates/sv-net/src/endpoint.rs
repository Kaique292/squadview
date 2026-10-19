//! O ponto QUIC de cada membro: um socket UDP só, usado para descobrir o endereço público (STUN),
//! "furar" o NAT na direção do outro e conectar/receber conexões.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Connection, Endpoint, EndpointConfig, ServerConfig, TokioRuntime, TransportConfig, VarInt};

use crate::cert::{Allowed, AllowedClients, ExpectFingerprint, Identity, provider};
use crate::{ALPN, stun};

/// Quanto tempo tentar conectar antes de desistir.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
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

/// Um membro na rede.
#[derive(Debug)]
pub struct Peer {
    endpoint: Endpoint,
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
        let candidates = candidates(public, local_ip(), port);

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
            Some(server_config(&identity, &allowed)?),
            socket,
            Arc::new(TokioRuntime),
        )?;
        Ok(Self {
            endpoint,
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
        let Ok(socket) = self.punch.try_clone() else { return };
        let targets = targets.to_vec();
        tokio::task::spawn_blocking(move || {
            let until = std::time::Instant::now() + PUNCH_FOR;
            while std::time::Instant::now() < until {
                for t in &targets {
                    let _ = socket.send_to(b"svpunch", t);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }

    /// Conecta em quem tem a fingerprint `expected`, tentando todos os endereços ao mesmo tempo.
    pub async fn connect(&self, targets: &[SocketAddr], expected: &str) -> Result<Connection, ConnectError> {
        self.punch(targets);
        let config = client_config(&self.identity, expected).map_err(|e| ConnectError::Other(e.to_string()))?;
        let mut attempts = tokio::task::JoinSet::new();
        for &target in targets {
            let Ok(connecting) = self.endpoint.connect_with(config.clone(), target, "squadview") else {
                continue;
            };
            attempts.spawn(connecting);
        }
        let result = tokio::time::timeout(CONNECT_TIMEOUT, async {
            while let Some(attempt) = attempts.join_next().await {
                if let Ok(Ok(conn)) = attempt {
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

    /// Espera a próxima conexão autorizada. `None` quando o ponto fecha.
    pub async fn accept(&self) -> Option<Connection> {
        loop {
            let incoming = self.endpoint.accept().await?;
            // Certificado não autorizado falha aqui, no handshake; segue esperando a próxima.
            if let Ok(conn) = incoming.await {
                return Some(conn);
            }
        }
    }

    pub fn close(&self) {
        self.keepalive.store(false, Ordering::Relaxed);
        self.endpoint.close(VarInt::from_u32(0), b"tchau");
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.keepalive.store(false, Ordering::Relaxed);
    }
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

fn transport() -> Arc<TransportConfig> {
    let mut t = TransportConfig::default();
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

fn server_config(identity: &Identity, allowed: &Allowed) -> io::Result<ServerConfig> {
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .with_client_cert_verifier(AllowedClients::new(allowed.clone()))
        .with_single_cert(vec![identity.cert.clone()], identity.key())
        .map_err(io::Error::other)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicServerConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport());
    Ok(config)
}

fn client_config(identity: &Identity, expected: &str) -> io::Result<ClientConfig> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(ExpectFingerprint::new(expected.to_owned()))
        .with_client_auth_cert(vec![identity.cert.clone()], identity.key())
        .map_err(io::Error::other)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport());
    Ok(config)
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
