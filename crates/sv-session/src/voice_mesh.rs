//! Malha de voz da sala (ADR 0010): uma conexão QUIC de voz com cada membro, os pacotes do nosso
//! microfone para todos e os de cada um para o mixer do [`VoiceEngine`].
//!
//! Quem chama: entre dois membros, quem tem o `peerId` menor manda `voice-offer`; o outro aceita a
//! fingerprint, "fura" o NAT e responde `voice-answer`; quem chamou conecta com o ALPN de voz.
//!
//! Quando a voz entre dois membros não abre, um terceiro que fala com os dois repassa (ADR 0020,
//! [`crate::voice_relay`]).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use quinn::Connection;
use sv_net::protocol::{RelayKey, decode_audio, encode_audio};

use crate::surface::lock;
use crate::voice::{OnPacket, VoiceEngine, VoiceEvent, VoiceSettings};
use crate::voice_relay::Relay;

/// Conexões de voz abertas, por membro.
type Conns = Arc<Mutex<HashMap<String, Connection>>>;

/// Avisa a sala que o repasse mudou: com quem a voz repassada caiu e por quem ela passava (para
/// pedir outra ponte, a outro membro), ou vazio quando só abriu uma conexão de repasse (quem está
/// ligado na voz mudou).
type OnRelayChange = Arc<dyn Fn(Vec<(String, String)>) + Send + Sync>;

/// Manda cada pacote do nosso microfone para todos com quem a voz está ligada (direto ou por
/// repasse).
fn send_to_all(conns: Conns, relay: Arc<Relay>) -> OnPacket {
    // Um buffer por chamada é pequeno (~100 bytes) e a voz manda no máximo 50 pacotes por segundo.
    Arc::new(move |seq, peak, opus| {
        let mut datagram = Vec::with_capacity(opus.len() + 16);
        encode_audio(seq, sv_gpu::clock::now_us(), peak, opus, &mut datagram);
        relay.send_mic(&datagram);
        let datagram = bytes::Bytes::from(datagram);
        for conn in lock(&conns).values() {
            let _ = conn.send_datagram(datagram.clone());
        }
    })
}

pub(crate) struct VoiceMesh {
    conns: Conns,
    /// Fingerprint de quem chamou a gente → membro (para reconhecer a conexão quando chegar).
    fingerprints: Mutex<HashMap<String, String>>,
    /// Fingerprint de cada membro com quem a voz abriu (para a conexão de repasse).
    fingerprint_of: Mutex<HashMap<String, String>>,
    /// Para quem mandamos `voice-offer` e ainda esperamos a resposta.
    offered: Mutex<HashSet<String>>,
    relay: Arc<Relay>,
    on_relay_change: Mutex<Option<OnRelayChange>>,
    engine: Option<Arc<VoiceEngine>>,
}

impl VoiceMesh {
    /// Liga o motor de voz (se `settings` vier). Sem voz (ou se o áudio falhar), a sala segue só
    /// com vídeo.
    pub(crate) fn start(
        settings: Option<VoiceSettings>,
        on_event: impl Fn(VoiceEvent) + Send + Sync + 'static,
    ) -> Self {
        let conns: Conns = Arc::default();
        let relay: Arc<Relay> = Arc::default();
        let on_packet = send_to_all(conns.clone(), relay.clone());
        let engine = settings.and_then(|s| VoiceEngine::start(s, on_packet, on_event).ok().map(Arc::new));
        Self::with_engine(conns, relay, engine)
    }

    fn with_engine(conns: Conns, relay: Arc<Relay>, engine: Option<Arc<VoiceEngine>>) -> Self {
        Self {
            conns,
            fingerprints: Mutex::new(HashMap::new()),
            fingerprint_of: Mutex::new(HashMap::new()),
            offered: Mutex::new(HashSet::new()),
            relay,
            on_relay_change: Mutex::new(None),
            engine,
        }
    }

    /// O nosso id na sala (muda ao reconectar): vai nas assinaturas da voz repassada.
    pub(crate) fn set_me(&self, me: &str) {
        self.relay.set_me(me);
    }

    /// Quem avisar quando o repasse mudar (ver [`OnRelayChange`]).
    pub(crate) fn on_relay_change(&self, cb: impl Fn(Vec<(String, String)>) + Send + Sync + 'static) {
        *lock(&self.on_relay_change) = Some(Arc::new(cb));
    }

    pub(crate) fn engine(&self) -> Option<&VoiceEngine> {
        self.engine.as_deref()
    }

    /// Devemos chamar `member` (temos o id menor, e ainda não há conexão nem convite)?
    pub(crate) fn should_offer(&self, me: &str, member: &str) -> bool {
        self.engine.is_some()
            && me < member
            && !lock(&self.conns).contains_key(member)
            && lock(&self.offered).insert(member.to_owned())
    }

    /// `member` nos chamou com essa fingerprint.
    pub(crate) fn expect(&self, member: &str, fingerprint: &str) {
        lock(&self.fingerprints).insert(fingerprint.to_owned(), member.to_owned());
    }

    /// Chegou a resposta de `member`: devolve `true` se a gente tinha chamado (e deve conectar).
    pub(crate) fn answered(&self, member: &str) -> bool {
        lock(&self.offered).remove(member)
    }

    /// De quem é a conexão de voz que chegou (pela fingerprint).
    pub(crate) fn member_for(&self, fingerprint: &str) -> Option<String> {
        lock(&self.fingerprints).get(fingerprint).cloned()
    }

    /// De quem é a conexão de repasse que chegou: só de alguém com quem a voz já está aberta.
    pub(crate) fn voice_member_for(&self, fingerprint: &str) -> Option<String> {
        let conns = lock(&self.conns);
        lock(&self.fingerprint_of)
            .iter()
            .find(|(member, fp)| fp.as_str() == fingerprint && conns.contains_key(member.as_str()))
            .map(|(member, _)| member.clone())
    }

    /// Onde está `member` (o endereço da conexão de voz) e a fingerprint dele, para abrir a
    /// conexão de repasse pelo mesmo caminho.
    pub(crate) fn reach(&self, member: &str) -> Option<(SocketAddr, String)> {
        let addr = lock(&self.conns).get(member)?.remote_address();
        let fingerprint = lock(&self.fingerprint_of).get(member)?.clone();
        Some((addr, fingerprint))
    }

    /// Quem pode repassar a voz com `target`: os membros com quem a voz está aberta (menos ele),
    /// do menor ping para o maior.
    pub(crate) fn helpers_for(&self, target: &str) -> Vec<String> {
        let mut helpers: Vec<(String, std::time::Duration)> = lock(&self.conns)
            .iter()
            .filter(|(member, _)| member.as_str() != target)
            .map(|(member, conn)| (member.clone(), conn.rtt()))
            .collect();
        helpers.sort_by_key(|(_, rtt)| *rtt);
        helpers.into_iter().map(|(member, _)| member).collect()
    }

    /// Temos voz aberta com `member` (direto)?
    pub(crate) fn talks_to(&self, member: &str) -> bool {
        lock(&self.conns).contains_key(member)
    }

    /// Como quem ajuda: passamos a repassar a voz entre `a` e `b` (ver [`Relay::add_pair`]).
    pub(crate) fn add_relay_pair(&self, a: &str, b: &str) -> Result<(), String> {
        if self.engine.is_none() {
            return Err("estou sem voz".into());
        }
        self.relay.add_pair(a, b, |m| self.talks_to(m))
    }

    /// Precisamos abrir a conexão de repasse com `member` (ajudamos ele e ela ainda não existe)?
    pub(crate) fn needs_relay_conn(&self, member: &str) -> bool {
        self.relay.helps(member) && !self.relay.has_conn(member)
    }

    /// Como ponta: a voz com `peer` passa por `helper`, assinada com `key`.
    pub(crate) fn route(&self, peer: &str, helper: &str, key: RelayKey) -> bool {
        let Some(engine) = &self.engine else { return false };
        if self.talks_to(peer) || engine.add_peer(peer).is_err() {
            return false;
        }
        self.relay.route(peer, helper, key)
    }

    /// Com quem a voz passa por outro membro agora → quem repassa.
    pub(crate) fn bridges(&self) -> Vec<(String, String)> {
        self.relay.routed()
    }

    /// A conexão de repasse com `member` abriu (nós ajudamos ele, ou ele nos ajuda): lê os pacotes,
    /// entrega a voz que é para nós e copia a dos outros.
    pub(crate) fn register_relay(self: &Arc<Self>, runtime: &tokio::runtime::Handle, member: String, conn: Connection) {
        let Some(engine) = self.engine.clone() else {
            conn.close(quinn::VarInt::from_u32(0), b"sem voz");
            return;
        };
        if let Some(old) = self.relay.add_conn(&member, conn.clone()) {
            old.close(quinn::VarInt::from_u32(0), b"substituida");
        }
        // Quem fala com a gente por ele já conta como ligado na voz.
        self.notify(Vec::new());
        let mesh = self.clone();
        runtime.spawn(async move {
            while let Ok(datagram) = conn.read_datagram().await {
                let ended = mesh.relay.handle(&member, &datagram, |origin, audio| {
                    // Abriu a conexão direta com ele: a voz direta vale, a repassada não.
                    if mesh.talks_to(origin) {
                        return;
                    }
                    if let Some((seq, _ts, peak, opus)) = decode_audio(audio) {
                        engine.submit(origin, seq, peak, opus);
                    }
                });
                if let Some(origin) = ended {
                    mesh.lose(vec![origin], &member);
                }
            }
            // Caiu: só limpa se ainda for esta a conexão de repasse com ele.
            if mesh.relay.is_current(&member, &conn) {
                let lost = mesh.relay.conn_closed(&member);
                mesh.lose(lost, &member);
            }
        });
    }

    /// A conexão de repasse com `member` não abriu: quem a gente ia ajudar com ele fica sabendo.
    pub(crate) fn relay_failed(&self, member: &str) {
        let lost = self.relay.conn_closed(member);
        self.lose(lost, member);
    }

    /// Perdemos a voz repassada por `helper` com `peers`: para de tocar e avisa a sala.
    fn lose(&self, peers: Vec<String>, helper: &str) {
        if peers.is_empty() {
            return;
        }
        if let Some(engine) = &self.engine {
            for peer in &peers {
                if !self.talks_to(peer) {
                    engine.remove_peer(peer);
                }
            }
        }
        self.notify(peers.into_iter().map(|p| (p, helper.to_owned())).collect());
    }

    fn notify(&self, lost: Vec<(String, String)>) {
        let cb = lock(&self.on_relay_change).clone();
        if let Some(cb) = cb {
            cb(lost);
        }
    }

    /// A conexão de voz com `member` abriu: começa a ouvir e a mandar. `fingerprint` é a dele.
    pub(crate) fn register(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
        member: String,
        fingerprint: &str,
        conn: Connection,
    ) {
        let Some(engine) = self.engine.clone() else {
            conn.close(quinn::VarInt::from_u32(0), b"sem voz");
            return;
        };
        if engine.add_peer(&member).is_err() {
            return;
        }
        lock(&self.fingerprint_of).insert(member.clone(), fingerprint.to_owned());
        // A conexão direta abriu: a voz com ele não precisa mais de repasse.
        self.relay.drop_route(&member);
        if let Some(old) = lock(&self.conns).insert(member.clone(), conn.clone()) {
            old.close(quinn::VarInt::from_u32(0), b"substituida");
        }
        let mesh = self.clone();
        runtime.spawn(async move {
            while let Ok(datagram) = conn.read_datagram().await {
                if let Some((seq, _ts, peak, opus)) = decode_audio(&datagram) {
                    engine.submit(&member, seq, peak, opus);
                }
            }
            // Caiu: só tira se ainda for esta a conexão dele (pode ter sido trocada por outra).
            let current = lock(&mesh.conns).get(&member).map(Connection::stable_id);
            if current == Some(conn.stable_id()) {
                mesh.unregister(&member);
            }
        });
    }

    /// `member` saiu da sala (ou a conexão caiu): para de mandar e de ouvir, e o repasse que
    /// passava por ele (ou que fazíamos para ele) acaba.
    pub(crate) fn unregister(&self, member: &str) {
        lock(&self.offered).remove(member);
        lock(&self.fingerprint_of).remove(member);
        if let Some(conn) = lock(&self.conns).remove(member) {
            conn.close(quinn::VarInt::from_u32(0), b"saiu");
        }
        if let Some(engine) = &self.engine {
            engine.remove_peer(member);
        }
        let lost = self.relay.forget(member);
        self.lose(lost, member);
    }

    /// Com quem a voz está ligada agora (direto ou por repasse).
    pub(crate) fn connected(&self) -> Vec<String> {
        let mut peers: Vec<String> = lock(&self.conns).keys().cloned().collect();
        for (peer, _) in self.relay.routed() {
            if !peers.contains(&peer) {
                peers.push(peer);
            }
        }
        peers
    }

    pub(crate) fn close(&self) {
        for (_, conn) in lock(&self.conns).drain() {
            conn.close(quinn::VarInt::from_u32(0), b"tchau");
        }
        self.relay.close();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sv_net::{Peer, Purpose};

    use super::*;

    /// Uma malha com voz, mas sem microfone nem saída de verdade.
    fn mesh() -> Arc<VoiceMesh> {
        let conns: Conns = Arc::default();
        let settings = VoiceSettings {
            noise_suppression: false,
            ..VoiceSettings::default()
        };
        let relay: Arc<Relay> = Arc::default();
        let engine = VoiceEngine::without_devices(settings, send_to_all(conns.clone(), relay.clone()), |_| {});
        Arc::new(VoiceMesh::with_engine(conns, relay, Some(Arc::new(engine))))
    }

    /// Uma conexão de voz entre dois pontos no mesmo PC: (quem chamou, quem atendeu).
    async fn voice_link() -> (Peer, Peer, Connection, Connection) {
        let caller = Peer::bind(&[]).await.unwrap();
        let callee = Peer::bind(&[]).await.unwrap();
        let (caller_conn, callee_conn) = link(&caller, &callee, Purpose::Voice).await;
        (caller, callee, caller_conn, callee_conn)
    }

    /// Uma conexão `purpose` de `caller` para `callee`: (lado de quem chamou, lado de quem atendeu).
    async fn link(caller: &Peer, callee: &Peer, purpose: Purpose) -> (Connection, Connection) {
        callee.allowed().allow(caller.fingerprint());
        let (accepted, conn) = tokio::join!(
            callee.accept(),
            caller.connect_for(callee.candidates(), None, callee.fingerprint(), purpose)
        );
        let accepted = accepted.expect("aceitar");
        assert_eq!(accepted.purpose, purpose);
        (conn.expect("conectar"), accepted.conn)
    }

    /// Um tom de 1 s no microfone de mentira de `mesh`.
    fn speak(mesh: &VoiceMesh) {
        let mut mic = mesh.engine().unwrap().fake_mic();
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin())
            .collect();
        for block in tone.chunks(480) {
            mic(block);
        }
    }

    async fn until(what: &str, mut done: impl FnMut() -> bool) {
        for _ in 0..100 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("não aconteceu: {what}");
    }

    #[test]
    fn sem_voz_ninguem_e_chamado() {
        let mesh = VoiceMesh::start(None, |_| {});
        assert!(mesh.engine().is_none());
        assert!(!mesh.should_offer("a", "b"));
        assert!(mesh.connected().is_empty());
        mesh.unregister("b");
        mesh.close();
    }

    #[test]
    fn quem_tem_o_id_menor_chama_uma_vez_so() {
        let mesh = mesh();
        assert!(mesh.should_offer("a", "b"));
        assert!(!mesh.should_offer("a", "b"), "convite já mandado");
        assert!(!mesh.should_offer("b", "a"), "quem tem o id maior espera ser chamado");
        assert!(mesh.answered("b"));
        assert!(!mesh.answered("b"), "a mesma resposta não conecta duas vezes");
        assert!(!mesh.answered("c"), "resposta de quem não chamamos é ignorada");
        // Saiu antes de responder: um convite novo pode ser feito quando voltar.
        assert!(mesh.should_offer("a", "c"));
        mesh.unregister("c");
        assert!(mesh.should_offer("a", "c"));
    }

    #[test]
    fn reconhece_quem_chamou_pela_fingerprint() {
        let mesh = mesh();
        mesh.expect("beto", "fp-beto");
        assert_eq!(mesh.member_for("fp-beto").as_deref(), Some("beto"));
        assert_eq!(mesh.member_for("fp-outro"), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sem_voz_recusa_a_conexao_que_chega() {
        let (_caller, _callee, caller_conn, callee_conn) = voice_link().await;
        let mesh = Arc::new(VoiceMesh::start(None, |_| {}));
        mesh.register(&tokio::runtime::Handle::current(), "ana".into(), "fp-ana", callee_conn);
        let reason = caller_conn.closed().await;
        assert!(reason.to_string().contains("sem voz"), "{reason}");
        assert!(mesh.connected().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_voz_de_um_chega_no_buffer_do_outro() {
        let (_caller, _callee, ana_side, beto_side) = voice_link().await;
        let runtime = tokio::runtime::Handle::current();
        // Ana fala com o Beto pela conexão `ana_side`; o Beto ouve pela `beto_side`.
        let ana = mesh();
        let beto = mesh();
        ana.register(&runtime, "beto".into(), "fp-beto", ana_side);
        beto.register(&runtime, "ana".into(), "fp-ana", beto_side);
        assert_eq!(ana.connected(), ["beto"]);
        assert_eq!(beto.engine().unwrap().peers(), ["ana"]);

        speak(&ana);
        let engine = beto.engine().unwrap();
        until("pacotes da Ana no Beto", || engine.buffered("ana").unwrap_or(0) >= 10).await;

        // A Ana sai: o Beto para de ouvir e a conexão fecha.
        beto.unregister("ana");
        assert!(beto.connected().is_empty());
        assert_eq!(engine.buffered("ana"), None);
        until("a Ana ver a conexão cair", || ana.connected().is_empty()).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_voz_de_quem_nao_alcancamos_chega_pela_conexao_de_outro_membro() {
        const ANA: &str = "aaaaaaaaaaaaaaaa";
        const BETO: &str = "bbbbbbbbbbbbbbbb";
        const HUGO: &str = "cccccccccccccccc";
        let runtime = tokio::runtime::Handle::current();
        let (ana_peer, beto_peer, hugo_peer) = (
            Peer::bind(&[]).await.unwrap(),
            Peer::bind(&[]).await.unwrap(),
            Peer::bind(&[]).await.unwrap(),
        );
        let (ana, beto, hugo) = (mesh(), mesh(), mesh());
        ana.set_me(ANA);
        beto.set_me(BETO);
        hugo.set_me(HUGO);
        let lost: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let seen = lost.clone();
        beto.on_relay_change(move |peers| lock(&seen).extend(peers));

        // A Ana e o Beto falam com o Hugo, mas não um com o outro (os dois atrás de CGNAT).
        let (a, h) = link(&ana_peer, &hugo_peer, Purpose::Voice).await;
        ana.register(&runtime, HUGO.into(), hugo_peer.fingerprint(), a);
        hugo.register(&runtime, ANA.into(), ana_peer.fingerprint(), h);
        let (b, h) = link(&beto_peer, &hugo_peer, Purpose::Voice).await;
        beto.register(&runtime, HUGO.into(), hugo_peer.fingerprint(), b);
        hugo.register(&runtime, BETO.into(), beto_peer.fingerprint(), h);
        assert_eq!(ana.helpers_for(BETO), [HUGO]);
        assert!(ana.helpers_for(HUGO).is_empty());

        // O Hugo aceita repassar e abre a conexão de repasse com os dois, pelo mesmo caminho.
        hugo.add_relay_pair(ANA, BETO).unwrap();
        for (member, peer, mesh) in [(ANA, &ana_peer, &ana), (BETO, &beto_peer, &beto)] {
            assert!(hugo.needs_relay_conn(member));
            let (addr, fingerprint) = hugo.reach(member).unwrap();
            assert_eq!(fingerprint, peer.fingerprint());
            assert_eq!(addr, hugo.reach(member).unwrap().0);
            let (from_hugo, at_member) = link(&hugo_peer, peer, Purpose::VoiceRelay).await;
            hugo.register_relay(&runtime, member.into(), from_hugo);
            assert_eq!(mesh.voice_member_for(hugo_peer.fingerprint()).as_deref(), Some(HUGO));
            mesh.register_relay(&runtime, HUGO.into(), at_member);
            assert!(!hugo.needs_relay_conn(member));
        }
        // A chave do par, que só a Ana e o Beto conhecem.
        let key = RelayKey::random().unwrap();
        assert!(ana.route(BETO, HUGO, RelayKey::new(&key)));
        assert!(beto.route(ANA, HUGO, RelayKey::new(&key)));
        assert!(
            !ana.route(HUGO, BETO, RelayKey::new(&key)),
            "com o Hugo a voz já é direta"
        );
        until("a Ana aparecer ligada para o Beto", || {
            beto.connected().contains(&ANA.to_owned())
        })
        .await;
        assert_eq!(beto.bridges(), [(ANA.to_owned(), HUGO.to_owned())]);

        // A Ana fala; o Beto ouve a voz dela (não a do Hugo) pelo repasse, e vice-versa.
        speak(&ana);
        let engine = beto.engine().unwrap();
        until("a voz da Ana no Beto", || engine.buffered(ANA).unwrap_or(0) >= 10).await;
        speak(&beto);
        let engine = ana.engine().unwrap();
        until("a voz do Beto na Ana", || engine.buffered(BETO).unwrap_or(0) >= 10).await;

        // O Hugo sai da voz da Ana: para de repassar e avisa o Beto, que perde a voz dela.
        hugo.unregister(ANA);
        until("o Beto saber que a ponte caiu", || {
            lock(&lost).contains(&(ANA.to_owned(), HUGO.to_owned()))
        })
        .await;
        assert!(!beto.connected().contains(&ANA.to_owned()));
        assert_eq!(beto.engine().unwrap().buffered(ANA), None);
        hugo.close();
        beto.close();
        ana.close();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn conexao_de_repasse_que_nao_abre_desfaz_os_pares() {
        let hugo = mesh();
        let lost: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let seen = lost.clone();
        hugo.on_relay_change(move |peers| lock(&seen).extend(peers));
        // Sem voz com ninguém: não dá para ajudar, nem saber onde a pessoa está.
        assert!(hugo.add_relay_pair("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb").is_err());
        assert!(hugo.reach("aaaaaaaaaaaaaaaa").is_none());
        assert!(hugo.voice_member_for("fp").is_none());
        hugo.relay_failed("aaaaaaaaaaaaaaaa");
        assert!(lock(&lost).is_empty(), "nada passava por ali");
        // Sem voz nenhuma (sem motor de áudio), nem tenta.
        let mute = VoiceMesh::start(None, |_| {});
        assert!(mute.add_relay_pair("a", "b").is_err());
        assert!(!mute.route(
            "aaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbb",
            RelayKey::new(&RelayKey::random().unwrap())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sem_voz_recusa_a_conexao_de_repasse() {
        let (_caller, _callee, caller_conn, callee_conn) = voice_link().await;
        let mesh = Arc::new(VoiceMesh::start(None, |_| {}));
        mesh.register_relay(&tokio::runtime::Handle::current(), "ana".into(), callee_conn);
        let reason = caller_conn.closed().await;
        assert!(reason.to_string().contains("sem voz"), "{reason}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn conexao_nova_do_mesmo_amigo_substitui_a_antiga() {
        let runtime = tokio::runtime::Handle::current();
        let beto = mesh();
        let (_a1, _b1, old_remote, old_local) = voice_link().await;
        let (_a2, _b2, _new_remote, new_local) = voice_link().await;
        beto.register(&runtime, "ana".into(), "fp-ana", old_local);
        beto.register(&runtime, "ana".into(), "fp-ana", new_local.clone());
        let reason = old_remote.closed().await;
        assert!(reason.to_string().contains("substituida"), "{reason}");
        // A antiga caiu, mas a nova continua valendo.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(beto.connected(), ["ana"]);
        assert_eq!(
            lock(&beto.conns).get("ana").map(Connection::stable_id),
            Some(new_local.stable_id())
        );
        beto.close();
        assert!(beto.connected().is_empty());
    }
}
