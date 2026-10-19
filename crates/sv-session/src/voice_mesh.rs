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
        let send_conns = conns.clone();
        // Um buffer por chamada é pequeno (~100 bytes) e a voz manda no máximo 50 pacotes por segundo.
        let on_packet: OnPacket = Arc::new(move |seq, peak, opus| {
            let mut datagram = Vec::with_capacity(opus.len() + 16);
            encode_audio(seq, sv_gpu::clock::now_us(), peak, opus, &mut datagram);
            let datagram = bytes::Bytes::from(datagram);
            for conn in lock(&send_conns).values() {
                let _ = conn.send_datagram(datagram.clone());
            }
        });
        let engine = settings.and_then(|s| VoiceEngine::start(s, on_packet, on_event).ok().map(Arc::new));
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
