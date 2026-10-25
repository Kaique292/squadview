//! A malha na sala (ADR 0030): o vídeo de uma live chega em faixas, parte direto de quem transmite
//! e parte repassada por outros membros, cada um na sua faixa da casa.
//!
//! - **Quem transmite:** coordena ([`MeshCoordinator`]). Quem pediu para assistir pela malha entra
//!   no plano quando a conexão abre; cada membro recebe pela sinalização de quem vem cada faixa e
//!   para quem repassar a dele. Quem sai, ou perde uma faixa, é consertado só onde quebrou.
//! - **Quem assiste:** conecta nos pais (ALPN da malha), aceita os filhos e avisa quando um pai
//!   para de mandar. O som, o relógio e os keyframes continuam na conexão direta com quem
//!   transmite.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::Connection;
use sv_net::protocol::{MeshHello, MeshParent, MeshPeer, Signal};

use super::Inner;
use crate::mesh::{MeshCoordinator, MeshViewer, VIEWER_UPLOAD_KBPS, mesh_nat, new_mask};
use crate::remote::MeshEndpoint;
use crate::surface::lock;

/// Quantas vezes o filho tenta conectar num pai antes de avisar que a faixa não chega: o pai pode
/// ainda não ter recebido o plano que autoriza a gente.
const CONNECT_TRIES: u32 = 3;
/// Espera entre as tentativas.
const CONNECT_RETRY: Duration = Duration::from_millis(400);

/// Quem pediu para assistir pela malha, até a conexão dele chegar.
#[derive(Debug, Clone)]
pub(super) struct MeshRequest {
    pub id: String,
    pub candidates: Vec<SocketAddr>,
    pub hello: MeshHello,
}

/// A malha de uma live que assistimos.
#[derive(Debug)]
struct MeshView {
    endpoint: MeshEndpoint,
    /// O pai de cada faixa que vem de um membro (id e conexão).
    parents: HashMap<u8, (String, Connection)>,
}

/// O estado da malha na sala, como quem transmite e como quem assiste.
#[derive(Debug, Default)]
pub(super) struct MeshState {
    /// A malha da nossa live (só enquanto transmitimos).
    coordinator: Mutex<Option<MeshCoordinator>>,
    /// Quem pediu para assistir pela malha, por fingerprint, até a conexão chegar.
    pending: Mutex<HashMap<String, MeshRequest>>,
    /// As malhas das lives que assistimos, por quem transmite.
    views: Mutex<HashMap<String, MeshView>>,
    /// O último plano recebido de cada live, até o vídeo dela abrir.
    assigned: Mutex<HashMap<String, Signal>>,
    /// Fingerprint de quem é nosso filho na malha → de qual live.
    children: Mutex<HashMap<String, String>>,
}

impl Inner {
    /// O que mandamos no pedido para assistir: o nosso NAT e o upload que doamos.
    pub(super) fn mesh_hello(&self) -> MeshHello {
        MeshHello {
            nat: mesh_nat(self.peer.nat()),
            upload_kbps: VIEWER_UPLOAD_KBPS,
        }
    }

    /// Começamos a transmitir: abre a malha da nossa live.
    pub(super) fn mesh_start(&self) {
        let me = lock(&self.me).id.clone();
        *lock(&self.mesh.coordinator) = Some(MeshCoordinator::new(&me));
    }

    /// Paramos de transmitir: a malha acaba junto.
    pub(super) fn mesh_stop(&self) {
        lock(&self.mesh.coordinator).take();
        lock(&self.mesh.pending).clear();
    }

    /// Respondendo um pedido para assistir que veio com a malha: guarda até a conexão chegar.
    /// Devolve se a pessoa vai receber pela malha.
    pub(super) fn mesh_offer(&self, fingerprint: &str, request: MeshRequest) -> bool {
        if lock(&self.mesh.coordinator).is_none() {
            return false;
        }
        lock(&self.mesh.pending).insert(fingerprint.to_owned(), request);
        true
    }

    /// Chegou a conexão de quem assiste: se veio pela malha, entra no plano e recebe só as faixas
    /// que vão direto. Devolve `false` se não é da malha (segue pelo caminho de sempre).
    pub(super) fn mesh_accept(self: &Arc<Self>, conn: &Connection, fingerprint: Option<&str>) -> bool {
        let Some(request) = fingerprint.and_then(|f| lock(&self.mesh.pending).remove(f)) else {
            return false;
        };
        let Some(keyframes) = lock(&self.keyframes).clone() else {
            return false;
        };
        let mask = new_mask();
        self.broadcaster
            .add_mesh(conn.clone(), mask.clone(), Arc::new(move |_| keyframes.request()));
        let viewer = MeshViewer {
            id: request.id.clone(),
            fingerprint: fingerprint.unwrap_or_default().to_owned(),
            candidates: request.candidates,
            hello: request.hello,
            mask,
        };
        let out = lock(&self.mesh.coordinator)
            .as_mut()
            .map(|c| c.join(viewer))
            .unwrap_or_default();
        self.send_mesh(out);
        // Parou de assistir (ou caiu): sai do plano.
        let inner = Arc::downgrade(self);
        let closed = conn.clone();
        let id = request.id;
        self.runtime.spawn(async move {
            closed.closed().await;
            if let Some(inner) = inner.upgrade() {
                inner.mesh_leave(&id);
            }
        });
        true
    }

    /// `id` saiu da sala ou parou de assistir a nossa live.
    pub(super) fn mesh_leave(&self, id: &str) {
        let out = lock(&self.mesh.coordinator)
            .as_mut()
            .map(|c| c.leave(id))
            .unwrap_or_default();
        self.send_mesh(out);
    }

    fn send_mesh(&self, out: Vec<(String, Signal)>) {
        if out.is_empty() {
            return;
        }
        let client = lock(&self.client);
        for (to, signal) in out {
            client.signal(&to, &signal);
        }
    }

    /// Sinais da malha (os outros voltam em `Err`).
    pub(super) fn mesh_signal(self: &Arc<Self>, from: &str, signal: Signal) -> Result<(), Signal> {
        match signal {
            Signal::MeshLost { stripe } => {
                let out = lock(&self.mesh.coordinator)
                    .as_mut()
                    .map(|c| c.lost(from, stripe))
                    .unwrap_or_default();
                self.send_mesh(out);
                Ok(())
            }
            assign @ Signal::MeshAssign { .. } => {
                lock(&self.mesh.assigned).insert(from.to_owned(), assign);
                self.mesh_apply(from);
                Ok(())
            }
            other => Err(other),
        }
    }

    /// O app abriu o vídeo de uma live que veio pela malha: liga o plano que já chegou.
    pub(super) fn mesh_view_opened(self: &Arc<Self>, streamer: &str, endpoint: MeshEndpoint) {
        lock(&self.mesh.views).insert(
            streamer.to_owned(),
            MeshView {
                endpoint,
                parents: HashMap::new(),
            },
        );
        self.mesh_apply(streamer);
    }

    /// Paramos de assistir a live: fecha as conexões com os pais (os filhos caem com o vídeo).
    pub(super) fn mesh_view_closed(&self, streamer: &str) {
        if let Some(view) = lock(&self.mesh.views).remove(streamer) {
            for (_, conn) in view.parents.into_values() {
                conn.close(quinn::VarInt::from_u32(0), b"parou de assistir");
            }
        }
        lock(&self.mesh.assigned).remove(streamer);
        lock(&self.mesh.children).retain(|_, s| s != streamer);
    }

    /// Aplica o último plano de `streamer`: autoriza e "fura" o NAT para os filhos, liga a faixa
    /// da casa e conecta nos pais que mudaram.
    fn mesh_apply(self: &Arc<Self>, streamer: &str) {
        let Some(Signal::MeshAssign {
            parents,
            home,
            children,
        }) = lock(&self.mesh.assigned).get(streamer).cloned()
        else {
            return;
        };
        let mut views = lock(&self.mesh.views);
        let Some(view) = views.get_mut(streamer) else {
            return;
        };

        // Filhos: conectam na gente para receber a faixa da casa.
        {
            let mut known = lock(&self.mesh.children);
            known.retain(|_, s| s != streamer);
            for child in &children {
                self.peer.allowed().allow(&child.fingerprint);
                self.peer.punch(&child.candidates);
                self.peer.permit_relay(&child.candidates);
                known.insert(child.fingerprint.clone(), streamer.to_owned());
            }
        }
        view.endpoint.stripes.set_home(home, view.endpoint.relay.clone());

        // Pais: fecha os que saíram do plano e conecta nos novos.
        let wanted: HashMap<u8, MeshPeer> = parents
            .into_iter()
            .filter_map(|MeshParent { stripe, via }| via.map(|v| (stripe, v)))
            .collect();
        view.parents.retain(|stripe, (id, conn)| {
            let keep = wanted.get(stripe).is_some_and(|p| p.id == *id);
            if !keep {
                conn.close(quinn::VarInt::from_u32(0), b"pai novo");
            }
            keep
        });
        let to_connect: Vec<(u8, MeshPeer)> = wanted
            .into_iter()
            .filter(|(stripe, _)| !view.parents.contains_key(stripe))
            .collect();
        drop(views);
        for (stripe, parent) in to_connect {
            self.mesh_connect(streamer.to_owned(), stripe, parent);
        }
    }

    /// Conecta no pai da faixa `stripe`. Se não abrir, ou se o pai parar de mandar, avisa quem
    /// transmite para trocar o pai.
    fn mesh_connect(self: &Arc<Self>, streamer: String, stripe: u8, parent: MeshPeer) {
        let inner = self.clone();
        self.runtime.spawn(async move {
            let mut conn = None;
            for attempt in 0..CONNECT_TRIES {
                if attempt > 0 {
                    tokio::time::sleep(CONNECT_RETRY).await;
                }
                if let Ok(c) = inner
                    .peer
                    .connect_for(&parent.candidates, None, &parent.fingerprint, sv_net::Purpose::Mesh)
                    .await
                {
                    conn = Some(c);
                    break;
                }
            }
            let mut views = lock(&inner.mesh.views);
            // Enquanto conectava, o plano pode ter mudado (ou paramos de assistir).
            let still_wanted = matches!(
                lock(&inner.mesh.assigned).get(&streamer),
                Some(Signal::MeshAssign { parents, .. })
                    if parents.iter().any(|p| p.stripe == stripe && p.via.as_ref().is_some_and(|v| v.id == parent.id))
            );
            let Some(view) = views.get_mut(&streamer).filter(|_| still_wanted) else {
                if let Some(c) = conn {
                    c.close(quinn::VarInt::from_u32(0), b"plano mudou");
                }
                return;
            };
            let Some(conn) = conn else {
                drop(views);
                lock(&inner.client).signal(&streamer, &Signal::MeshLost { stripe });
                return;
            };
            let id = conn.stable_id();
            let weak = Arc::downgrade(&inner);
            let lost_streamer = streamer.clone();
            view.endpoint.stripes.attach_parent(conn.clone(), move || {
                let Some(inner) = weak.upgrade() else { return };
                // Só avisa se este ainda é o pai da faixa (não uma conexão trocada de propósito).
                let current = lock(&inner.mesh.views)
                    .get_mut(&lost_streamer)
                    .and_then(|v| {
                        let is_current = v.parents.get(&stripe).is_some_and(|(_, c)| c.stable_id() == id);
                        is_current.then(|| v.parents.remove(&stripe))
                    })
                    .is_some();
                if current {
                    lock(&inner.client).signal(&lost_streamer, &Signal::MeshLost { stripe });
                }
            });
            view.parents.insert(stripe, (parent.id, conn));
        });
    }

    /// Um filho conectou com o ALPN da malha: passa a receber a nossa faixa da casa.
    pub(super) fn mesh_child(&self, conn: Connection, fingerprint: Option<&str>) {
        let streamer = fingerprint.and_then(|f| lock(&self.mesh.children).get(f).cloned());
        let relay = streamer.and_then(|s| lock(&self.mesh.views).get(&s).map(|v| v.endpoint.relay.clone()));
        match relay {
            Some(relay) => relay.add(conn),
            None => conn.close(quinn::VarInt::from_u32(2), b"desconhecido"),
        }
    }
}
