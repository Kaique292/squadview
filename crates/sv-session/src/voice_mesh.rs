//! Malha de voz da sala (ADR 0010): uma conexão QUIC de voz com cada membro, os pacotes do nosso
//! microfone para todos e os de cada um para o mixer do [`VoiceEngine`].
//!
//! Quem chama: entre dois membros, quem tem o `peerId` menor manda `voice-offer`; o outro aceita a
//! fingerprint, "fura" o NAT e responde `voice-answer`; quem chamou conecta com o ALPN de voz.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use quinn::Connection;
use sv_net::protocol::{decode_audio, encode_audio};

use crate::surface::lock;
use crate::voice::{OnPacket, VoiceEngine, VoiceEvent, VoiceSettings};

/// Conexões de voz abertas, por membro.
type Conns = Arc<Mutex<HashMap<String, Connection>>>;

/// Manda cada pacote do nosso microfone para todos com quem a voz está ligada.
fn send_to_all(conns: Conns) -> OnPacket {
    // Um buffer por chamada é pequeno (~100 bytes) e a voz manda no máximo 50 pacotes por segundo.
    Arc::new(move |seq, peak, opus| {
        let mut datagram = Vec::with_capacity(opus.len() + 16);
        encode_audio(seq, sv_gpu::clock::now_us(), peak, opus, &mut datagram);
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
    /// Para quem mandamos `voice-offer` e ainda esperamos a resposta.
    offered: Mutex<HashSet<String>>,
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
        let on_packet = send_to_all(conns.clone());
        let engine = settings.and_then(|s| VoiceEngine::start(s, on_packet, on_event).ok().map(Arc::new));
        Self::with_engine(conns, engine)
    }

    fn with_engine(conns: Conns, engine: Option<Arc<VoiceEngine>>) -> Self {
        Self {
            conns,
            fingerprints: Mutex::new(HashMap::new()),
            offered: Mutex::new(HashSet::new()),
            engine,
        }
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

    /// A conexão de voz com `member` abriu: começa a ouvir e a mandar.
    pub(crate) fn register(self: &Arc<Self>, runtime: &tokio::runtime::Handle, member: String, conn: Connection) {
        let Some(engine) = self.engine.clone() else {
            conn.close(quinn::VarInt::from_u32(0), b"sem voz");
            return;
        };
        if engine.add_peer(&member).is_err() {
            return;
        }
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

    /// `member` saiu da sala (ou a conexão caiu): para de mandar e de ouvir.
    pub(crate) fn unregister(&self, member: &str) {
        lock(&self.offered).remove(member);
        if let Some(conn) = lock(&self.conns).remove(member) {
            conn.close(quinn::VarInt::from_u32(0), b"saiu");
        }
        if let Some(engine) = &self.engine {
            engine.remove_peer(member);
        }
    }

    /// Com quem a voz está ligada agora.
    pub(crate) fn connected(&self) -> Vec<String> {
        lock(&self.conns).keys().cloned().collect()
    }

    pub(crate) fn close(&self) {
        for (_, conn) in lock(&self.conns).drain() {
            conn.close(quinn::VarInt::from_u32(0), b"tchau");
        }
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
        let engine = VoiceEngine::without_devices(settings, send_to_all(conns.clone()), |_| {});
        Arc::new(VoiceMesh::with_engine(conns, Some(Arc::new(engine))))
    }

    /// Uma conexão de voz entre dois pontos no mesmo PC: (quem chamou, quem atendeu).
    async fn voice_link() -> (Peer, Peer, Connection, Connection) {
        let caller = Peer::bind(&[]).await.unwrap();
        let callee = Peer::bind(&[]).await.unwrap();
        callee.allowed().allow(caller.fingerprint());
        let (accepted, conn) = tokio::join!(
            callee.accept(),
            caller.connect_for(callee.candidates(), callee.fingerprint(), Purpose::Voice)
        );
        let accepted = accepted.expect("aceitar");
        assert_eq!(accepted.purpose, Purpose::Voice);
        (caller, callee, conn.expect("conectar"), accepted.conn)
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
        mesh.register(&tokio::runtime::Handle::current(), "ana".into(), callee_conn);
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
        ana.register(&runtime, "beto".into(), ana_side);
        beto.register(&runtime, "ana".into(), beto_side);
        assert_eq!(ana.connected(), ["beto"]);
        assert_eq!(beto.engine().unwrap().peers(), ["ana"]);

        let mut mic = ana.engine().unwrap().fake_mic();
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin())
            .collect();
        for block in tone.chunks(480) {
            mic(block);
        }
        let engine = beto.engine().unwrap();
        until("pacotes da Ana no Beto", || engine.buffered("ana").unwrap_or(0) >= 10).await;

        // A Ana sai: o Beto para de ouvir e a conexão fecha.
        beto.unregister("ana");
        assert!(beto.connected().is_empty());
        assert_eq!(engine.buffered("ana"), None);
        until("a Ana ver a conexão cair", || ana.connected().is_empty()).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn conexao_nova_do_mesmo_amigo_substitui_a_antiga() {
        let runtime = tokio::runtime::Handle::current();
        let beto = mesh();
        let (_a1, _b1, old_remote, old_local) = voice_link().await;
        let (_a2, _b2, _new_remote, new_local) = voice_link().await;
        beto.register(&runtime, "ana".into(), old_local);
        beto.register(&runtime, "ana".into(), new_local.clone());
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
