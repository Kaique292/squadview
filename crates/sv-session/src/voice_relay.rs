//! Voz repassada por um membro da sala (ADR 0020): quando A e B não se alcançam, H, que fala com
//! os dois, copia os pacotes de voz entre eles por uma conexão de repasse (ALPN próprio).
//!
//! Cada pacote leva o membro (destino, de A para H; origem, de H para B) e uma assinatura com a
//! chave que só A e B conhecem: H copia os bytes como estão, sem decodificar, e não consegue
//! inventar nem desviar a voz de ninguém.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use quinn::Connection;
use sv_net::protocol::{
    RELAY_PEER_BYTES, RELAY_TAG_BYTES, RelayKey, Relayed, decode_relayed, encode_relayed, relay_member, relay_peer,
};

use crate::surface::lock;

/// Quantos pares um membro repassa ao mesmo tempo. A voz é leve (~32 kbps por sentido): 6 pares
/// gastam menos de 400 kbps do upload de quem ajuda.
pub(crate) const MAX_RELAYED_PAIRS: usize = 6;

/// Um membro com quem falamos por repasse.
struct Route {
    /// Quem repassa.
    helper: String,
    /// O membro em bytes, como vai no pacote.
    peer: [u8; RELAY_PEER_BYTES],
    key: RelayKey,
}

/// O estado do repasse de voz de um membro: como ponta (as rotas) e como quem ajuda (os pares).
#[derive(Default)]
pub(crate) struct Relay {
    /// O nosso id em bytes (muda ao reconectar na sala).
    me: Mutex<Option<[u8; RELAY_PEER_BYTES]>>,
    /// Como ponta: membro → por quem a voz com ele passa.
    routes: Mutex<HashMap<String, Arc<Route>>>,
    /// Conexões de repasse abertas, por membro (com quem repassa para nós, ou com quem ajudamos).
    conns: Mutex<HashMap<String, Connection>>,
    /// Como quem ajuda: membro → para quem copiamos a voz dele.
    pairs: Mutex<HashMap<String, HashSet<String>>>,
}

impl Relay {
    pub(crate) fn set_me(&self, me: &str) {
        *lock(&self.me) = relay_peer(me);
    }

    /// A voz com `peer` passa por `helper`, assinada com `key`. Devolve `false` se o id não serve.
    pub(crate) fn route(&self, peer: &str, helper: &str, key: RelayKey) -> bool {
        let Some(bytes) = relay_peer(peer) else { return false };
        let route = Route {
            helper: helper.to_owned(),
            peer: bytes,
            key,
        };
        lock(&self.routes).insert(peer.to_owned(), Arc::new(route));
        true
    }

    /// A voz com `peer` deixa de passar por repasse (a conexão direta abriu, ou ele saiu).
    pub(crate) fn drop_route(&self, peer: &str) -> bool {
        lock(&self.routes).remove(peer).is_some()
    }

    /// Com quem falamos por repasse agora (a conexão com quem repassa já abriu) → quem repassa.
    pub(crate) fn routed(&self) -> Vec<(String, String)> {
        let conns = lock(&self.conns);
        lock(&self.routes)
            .iter()
            .filter(|(_, r)| conns.contains_key(&r.helper))
            .map(|(peer, r)| (peer.clone(), r.helper.clone()))
            .collect()
    }

    /// Como quem ajuda: passamos a repassar a voz entre `a` e `b`. `talks_to` diz se temos voz com
    /// alguém. Recusa com o motivo se não falamos com os dois, ou se já repassamos o máximo.
    pub(crate) fn add_pair(&self, a: &str, b: &str, talks_to: impl Fn(&str) -> bool) -> Result<(), String> {
        if a == b || relay_peer(a).is_none() || relay_peer(b).is_none() || !talks_to(a) || !talks_to(b) {
            return Err("não estou na voz com vocês dois".into());
        }
        let mut pairs = lock(&self.pairs);
        let already = pairs.get(a).is_some_and(|s| s.contains(b));
        let count = pairs.values().map(HashSet::len).sum::<usize>() / 2;
        if !already && count >= MAX_RELAYED_PAIRS {
            return Err("já estou repassando a voz de muita gente".into());
        }
        pairs.entry(a.to_owned()).or_default().insert(b.to_owned());
        pairs.entry(b.to_owned()).or_default().insert(a.to_owned());
        Ok(())
    }

    /// Repassamos a voz de alguém para `member` (ou dele para alguém)?
    pub(crate) fn helps(&self, member: &str) -> bool {
        lock(&self.pairs).get(member).is_some_and(|s| !s.is_empty())
    }

    pub(crate) fn has_conn(&self, member: &str) -> bool {
        lock(&self.conns).contains_key(member)
    }

    /// A conexão de repasse com `member` abriu. Devolve a anterior, se havia (fecha quem chamou).
    pub(crate) fn add_conn(&self, member: &str, conn: Connection) -> Option<Connection> {
        lock(&self.conns).insert(member.to_owned(), conn)
    }

    /// A conexão de repasse com `member` ainda é esta?
    pub(crate) fn is_current(&self, member: &str, conn: &Connection) -> bool {
        lock(&self.conns).get(member).map(Connection::stable_id) == Some(conn.stable_id())
    }

    /// Manda o pacote do nosso microfone a quem só alcançamos por repasse, assinado para cada um.
    pub(crate) fn send_mic(&self, audio: &[u8]) {
        let Some(me) = *lock(&self.me) else { return };
        let routes: Vec<Arc<Route>> = lock(&self.routes).values().cloned().collect();
        if routes.is_empty() {
            return;
        }
        let conns = lock(&self.conns);
        for route in routes {
            let Some(conn) = conns.get(&route.helper) else { continue };
            let tag = route.key.tag(&me, &route.peer, audio);
            // Como no envio direto: um buffer pequeno por pacote, no máximo 50 por segundo.
            let mut packet = Vec::with_capacity(RELAY_PEER_BYTES + RELAY_TAG_BYTES + audio.len());
            encode_relayed(&route.peer, &tag, audio, &mut packet);
            let _ = conn.send_datagram(packet.into());
        }
    }

    /// Um pacote chegou pela conexão de repasse com `from`. Se for a voz de alguém para nós (e a
    /// assinatura conferir), entrega `(origem, áudio)` a `deliver`; se for de alguém que ajudamos,
    /// copia para o destino. Devolve o membro cuja voz parou de chegar por `from`, se for o aviso.
    pub(crate) fn handle(&self, from: &str, datagram: &[u8], deliver: impl FnOnce(&str, &[u8])) -> Option<String> {
        match decode_relayed(datagram)? {
            Relayed::Audio { peer, tag, audio } => {
                let member = relay_member(&peer);
                // Somos a ponta: a voz de `member`, repassada por `from`.
                let route = lock(&self.routes).get(&member).cloned();
                if let Some(route) = route.filter(|r| r.helper == from) {
                    let me = *lock(&self.me);
                    if me.is_some_and(|me| route.key.verify(&peer, &me, audio, &tag)) {
                        deliver(&member, audio);
                    }
                    return None;
                }
                // Somos quem ajuda: `from` fala com `member` por nós. Copia como está, com a origem.
                let allowed = lock(&self.pairs).get(from).is_some_and(|s| s.contains(&member));
                if allowed
                    && let Some(origin) = relay_peer(from)
                    && let Some(conn) = lock(&self.conns).get(&member)
                {
                    let mut packet = Vec::with_capacity(datagram.len());
                    encode_relayed(&origin, &tag, audio, &mut packet);
                    let _ = conn.send_datagram(packet.into());
                }
                None
            }
            Relayed::Ended { peer } => {
                let member = relay_member(&peer);
                let mut routes = lock(&self.routes);
                if routes.get(&member).is_some_and(|r| r.helper == from) {
                    routes.remove(&member);
                    return Some(member);
                }
                None
            }
        }
    }

    /// A conexão de repasse com `member` caiu (ou não abriu): quem a gente ajudava com ele fica
    /// sabendo, e a voz que passava por ele para. Devolve com quem perdemos a voz.
    pub(crate) fn conn_closed(&self, member: &str) -> Vec<String> {
        if let Some(conn) = lock(&self.conns).remove(member) {
            conn.close(quinn::VarInt::from_u32(0), b"fim do repasse");
        }
        self.end_pairs_with(member);
        let mut routes = lock(&self.routes);
        let lost: Vec<String> = routes
            .iter()
            .filter(|(_, r)| r.helper == member)
            .map(|(peer, _)| peer.clone())
            .collect();
        for peer in &lost {
            routes.remove(peer);
        }
        lost
    }

    /// `member` saiu da sala (ou a voz com ele caiu): tudo o que passava por ele ou ia para ele acaba.
    pub(crate) fn forget(&self, member: &str) -> Vec<String> {
        self.drop_route(member);
        self.conn_closed(member)
    }

    /// Paramos de repassar a voz entre `member` e os outros: avisa cada um pela conexão dele.
    fn end_pairs_with(&self, member: &str) {
        let mut pairs = lock(&self.pairs);
        let partners = pairs.remove(member).unwrap_or_default();
        let Some(ended) = relay_peer(member) else { return };
        let conns = lock(&self.conns);
        for partner in partners {
            if let Some(set) = pairs.get_mut(&partner) {
                set.remove(member);
                if set.is_empty() {
                    pairs.remove(&partner);
                }
            }
            if let Some(conn) = conns.get(&partner) {
                let _ = conn.send_datagram(bytes::Bytes::copy_from_slice(&ended));
            }
        }
    }

    pub(crate) fn close(&self) {
        for (_, conn) in lock(&self.conns).drain() {
            conn.close(quinn::VarInt::from_u32(0), b"tchau");
        }
        lock(&self.routes).clear();
        lock(&self.pairs).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANA: &str = "aaaaaaaaaaaaaaaa";
    const BETO: &str = "bbbbbbbbbbbbbbbb";
    const HUGO: &str = "cccccccccccccccc";

    fn key() -> [u8; 32] {
        RelayKey::random().unwrap()
    }

    /// O pacote que a Ana manda ao Hugo para o Beto, assinado com `k`.
    fn from_ana_to_beto(k: &[u8; 32], audio: &[u8]) -> Vec<u8> {
        let (ana, beto) = (relay_peer(ANA).unwrap(), relay_peer(BETO).unwrap());
        let tag = RelayKey::new(k).tag(&ana, &beto, audio);
        let mut packet = Vec::new();
        // Já como chega ao Beto: com a origem (a Ana) no lugar do destino.
        encode_relayed(&ana, &tag, audio, &mut packet);
        packet
    }

    #[test]
    fn so_entrega_a_voz_assinada_por_quem_tem_a_chave() {
        let k = key();
        let beto = Relay::default();
        beto.set_me(BETO);
        assert!(beto.route(ANA, HUGO, RelayKey::new(&k)));
        let mut heard = Vec::new();
        beto.handle(HUGO, &from_ana_to_beto(&k, b"oi"), |from, audio| {
            heard.push((from.to_owned(), audio.to_vec()))
        });
        assert_eq!(heard, [(ANA.to_owned(), b"oi".to_vec())]);

        // O Hugo inventa uma voz da Ana (sem a chave): não toca.
        heard.clear();
        let forged = from_ana_to_beto(&key(), b"golpe");
        beto.handle(HUGO, &forged, |from, audio| {
            heard.push((from.to_owned(), audio.to_vec()))
        });
        // Nem um pacote certo vindo por quem não é a ponte combinada.
        beto.handle(ANA, &from_ana_to_beto(&k, b"oi"), |from, audio| {
            heard.push((from.to_owned(), audio.to_vec()))
        });
        // Nem pacote truncado.
        beto.handle(HUGO, &[1, 2, 3], |from, audio| {
            heard.push((from.to_owned(), audio.to_vec()))
        });
        assert!(heard.is_empty());
    }

    #[test]
    fn sem_o_nosso_id_ainda_nao_confere_nada() {
        let k = key();
        let beto = Relay::default();
        beto.route(ANA, HUGO, RelayKey::new(&k));
        let mut heard = false;
        beto.handle(HUGO, &from_ana_to_beto(&k, b"oi"), |_, _| heard = true);
        assert!(!heard);
        // Sem id, também não manda nada (não sabe assinar).
        beto.send_mic(b"oi");
    }

    #[test]
    fn o_aviso_de_fim_so_vale_de_quem_repassa() {
        let beto = Relay::default();
        beto.route(ANA, HUGO, RelayKey::new(&key()));
        let ended = relay_peer(ANA).unwrap();
        assert_eq!(beto.handle(ANA, &ended, |_, _| {}), None);
        assert_eq!(beto.handle(HUGO, &ended, |_, _| {}), Some(ANA.to_owned()));
        assert_eq!(beto.handle(HUGO, &ended, |_, _| {}), None, "já tinha acabado");
    }

    #[test]
    fn quem_ajuda_so_aceita_quem_fala_com_os_dois_e_ate_o_limite() {
        let hugo = Relay::default();
        let everyone = |_: &str| true;
        assert!(hugo.add_pair(ANA, ANA, everyone).is_err());
        assert!(hugo.add_pair(ANA, "curto", everyone).is_err());
        assert!(hugo.add_pair(ANA, BETO, |m| m == ANA).is_err(), "não fala com o Beto");
        assert!(hugo.add_pair(ANA, BETO, everyone).is_ok());
        assert!(hugo.helps(ANA) && hugo.helps(BETO));
        // O mesmo par de novo não conta duas vezes.
        assert!(hugo.add_pair(BETO, ANA, everyone).is_ok());
        for i in 1..MAX_RELAYED_PAIRS {
            let other = format!("{i:016x}");
            assert!(hugo.add_pair(ANA, &other, everyone).is_ok(), "par {i}");
        }
        let err = hugo.add_pair(BETO, &format!("{:016x}", 99), everyone).unwrap_err();
        assert!(err.contains("muita gente"), "{err}");
        // O Beto saiu: os pares dele acabam e abre vaga.
        assert!(hugo.forget(BETO).is_empty());
        assert!(!hugo.helps(BETO));
        assert!(hugo.add_pair(ANA, &format!("{:016x}", 99), everyone).is_ok());
        hugo.close();
        assert!(!hugo.helps(ANA));
    }

    #[test]
    fn quem_repassa_saiu_a_voz_que_passava_por_ele_cai() {
        let beto = Relay::default();
        beto.route(ANA, HUGO, RelayKey::new(&key()));
        // A conexão com o Hugo nem chegou a abrir: a Ana não conta como ligada.
        assert!(beto.routed().is_empty());
        assert_eq!(beto.forget(HUGO), [ANA.to_owned()]);
        assert!(!beto.drop_route(ANA), "a rota já tinha caído");
        assert!(!beto.route("curto", HUGO, RelayKey::new(&key())));
    }
}
