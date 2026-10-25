//! Coordenação da malha por quem transmite (ADR 0030): quem entra na malha, de quem cada um
//! recebe cada faixa e o que muda quando alguém sai ou perde uma faixa.
//!
//! É lógica pura (sem rede): a sala entrega os eventos e manda os avisos que voltam daqui. As
//! faixas que vão direto de quem transmite ficam nas máscaras do `Broadcaster`, que o coordenador
//! atualiza.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use sv_core::{PeerId, QualityLayer};
use sv_mesh::plan::{Config, Member, Nat, Plan, TreeKey, plan, repair_with};
use sv_net::StripeMask;
use sv_net::protocol::{MeshHello, MeshNat, MeshParent, MeshPeer, Signal};
use sv_net::stripes::{DATA_STRIPES, STRIPES};

/// Todas as faixas direto (até o primeiro plano: a imagem começa na hora).
const ALL_STRIPES: u8 = (1 << STRIPES) - 1;
/// Só as faixas de dados direto: quem não coube no plano recebe como se não houvesse malha, sem
/// pagar a paridade (e pode trocar de camada pelo simulcast).
const DATA_STRIPES_ONLY: u8 = sv_net::stripes::DATA_MASK;
/// Upload que supomos para quem transmite enquanto não há medição: o planejador limita a 2 cópias
/// do stream de qualquer jeito (ADR 0012).
pub const SOURCE_UPLOAD_KBPS: u32 = 30_000;
/// Latência suposta entre dois membros enquanto não há medição.
const ASSUMED_RTT_MS: u32 = 30;
/// Upload que quem assiste anuncia enquanto não há medição: conservador (o de uma fibra de
/// entrada pela metade), para ninguém repassar mais do que aguenta. Com a folga do planejador,
/// dá para 3 faixas de 1080p60.
pub const VIEWER_UPLOAD_KBPS: u32 = 10_000;

/// Um membro da malha, do ponto de vista de quem transmite.
#[derive(Debug, Clone)]
pub struct MeshViewer {
    pub id: String,
    pub fingerprint: String,
    pub candidates: Vec<SocketAddr>,
    pub hello: MeshHello,
    /// As faixas que quem transmite manda direto (lida pelo `Broadcaster`).
    pub mask: StripeMask,
}

/// O que um membro precisa saber: de quem vem cada faixa, a casa e os filhos.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Assignment {
    parents: Vec<MeshParent>,
    home: Option<u8>,
    children: Vec<MeshPeer>,
}

/// A malha de uma live, coordenada por quem transmite.
#[derive(Debug)]
pub struct MeshCoordinator {
    source: Member,
    cfg: Config,
    viewers: BTreeMap<String, MeshViewer>,
    plan: Plan,
    /// O último aviso mandado a cada um (só manda de novo se mudar).
    sent: HashMap<String, Assignment>,
    /// Pares (filho, pai) cuja conexão não abriu: não são escolhidos de novo.
    failed: HashSet<(PeerId, PeerId)>,
}

/// O id de um membro da sala como `PeerId` (os ids da sala são 16 dígitos hexadecimais).
pub fn peer_id(id: &str) -> PeerId {
    u64::from_str_radix(id, 16).map_or_else(
        |_| {
            // Id fora do padrão: um hash estável (FNV-1a), longe do 0 de quem transmite.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in id.bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
            PeerId(h | 1)
        },
        PeerId,
    )
}

/// O NAT que quem assiste conta, como o planejador entende. Sem saber (só um servidor STUN
/// respondeu), conta como NAT comum: se a conexão com um pai não abrir, o filho avisa e o conserto
/// não volta para aquele par.
pub fn mesh_nat(nat: sv_net::NatMapping) -> MeshNat {
    match nat {
        sv_net::NatMapping::EndpointIndependent | sv_net::NatMapping::Unknown => MeshNat::Cone,
        sv_net::NatMapping::Symmetric => MeshNat::Symmetric,
    }
}

fn plan_nat(nat: MeshNat) -> Nat {
    match nat {
        MeshNat::Open => Nat::Open,
        MeshNat::Cone => Nat::Cone,
        MeshNat::Symmetric => Nat::Symmetric,
    }
}

const fn tree(stripe: u8) -> TreeKey {
    (QualityLayer::High, stripe)
}

impl MeshCoordinator {
    /// `source_id`: o nosso id na sala.
    pub fn new(source_id: &str) -> Self {
        Self {
            source: Member {
                id: PeerId(0),
                upload_kbps: SOURCE_UPLOAD_KBPS,
                download_kbps: u32::MAX,
                // Quem entra na malha já está conectado direto na gente: para ele, somos rede aberta.
                nat: Nat::Open,
                in_game: true,
            },
            // A malha deste passo carrega só a camada principal (ADR 0030).
            cfg: Config {
                top_layer: QualityLayer::High,
                min_layer: QualityLayer::High,
                ..Config::default()
            },
            viewers: BTreeMap::new(),
            plan: Plan::default(),
            sent: HashMap::new(),
            failed: HashSet::new(),
        }
        .with_source_id(source_id)
    }

    fn with_source_id(mut self, id: &str) -> Self {
        self.source.id = peer_id(id);
        self
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.viewers.len()
    }

    /// Alguém entrou na malha: o plano é refeito (ADR 0012). Devolve os avisos a mandar.
    pub fn join(&mut self, viewer: MeshViewer) -> Vec<(String, Signal)> {
        viewer.mask.store(ALL_STRIPES, Ordering::Relaxed);
        self.viewers.insert(viewer.id.clone(), viewer);
        self.plan = plan(&self.source, &self.members(), &|_, _| ASSUMED_RTT_MS, &self.cfg);
        self.publish()
    }

    /// Alguém saiu (da sala ou parou de assistir): conserto só onde quebrou (ADR 0029).
    pub fn leave(&mut self, id: &str) -> Vec<(String, Signal)> {
        if self.viewers.remove(id).is_none() {
            return Vec::new();
        }
        self.sent.remove(id);
        self.repair(&[])
    }

    /// `id` perdeu a faixa `stripe` (o pai saiu, ficou para trás ou a conexão não abriu): troca
    /// só esse pai, sem voltar para ele.
    pub fn lost(&mut self, id: &str, stripe: u8) -> Vec<(String, Signal)> {
        if !self.viewers.contains_key(id) || usize::from(stripe) >= STRIPES {
            return Vec::new();
        }
        let child = peer_id(id);
        let key = tree(stripe);
        if let Some(parent) = self.plan.parents.get(&key).and_then(|p| p.get(&child)) {
            self.failed.insert((child, *parent));
        }
        self.repair(&[(key, child)])
    }

    fn repair(&mut self, broken: &[(TreeKey, PeerId)]) -> Vec<(String, Signal)> {
        let failed = self.failed.clone();
        self.plan = repair_with(
            &self.plan,
            &self.source,
            &self.members(),
            &|_, _| ASSUMED_RTT_MS,
            &self.cfg,
            broken,
            &|c, p| failed.contains(&(c, p)),
        );
        self.publish()
    }

    fn members(&self) -> Vec<Member> {
        self.viewers
            .values()
            .map(|v| Member {
                id: peer_id(&v.id),
                upload_kbps: v.hello.upload_kbps,
                download_kbps: u32::MAX,
                nat: plan_nat(v.hello.nat),
                in_game: false,
            })
            .collect()
    }

    /// Atualiza as faixas que vão direto e devolve o aviso de quem teve alguma coisa mudada.
    fn publish(&mut self) -> Vec<(String, Signal)> {
        let by_peer: HashMap<PeerId, &MeshViewer> = self.viewers.values().map(|v| (peer_id(&v.id), v)).collect();
        let peer_of = |v: &MeshViewer| MeshPeer {
            id: v.id.clone(),
            candidates: v.candidates.clone(),
            fingerprint: v.fingerprint.clone(),
        };
        let mut out = Vec::new();
        for viewer in self.viewers.values() {
            let me = peer_id(&viewer.id);
            let mut mask = 0u8;
            let mut parents = Vec::new();
            for s in 0..STRIPES as u8 {
                match self.plan.parents.get(&tree(s)).and_then(|p| p.get(&me)) {
                    Some(p) if *p == self.source.id => {
                        mask |= 1 << s;
                        parents.push(MeshParent { stripe: s, via: None });
                    }
                    Some(p) => {
                        if let Some(parent) = by_peer.get(p) {
                            parents.push(MeshParent {
                                stripe: s,
                                via: Some(peer_of(parent)),
                            });
                        }
                    }
                    None => {}
                }
            }
            // Sem faixas o bastante para montar a imagem (não coube no plano), ou com tudo vindo
            // direto (a paridade não protege nada num caminho só): as de dados direto, para
            // ninguém ficar sem vídeo nem custar mais do que sem a malha.
            if parents.len() < DATA_STRIPES || parents.iter().all(|p| p.via.is_none()) {
                mask = DATA_STRIPES_ONLY;
                parents = (0..DATA_STRIPES as u8)
                    .map(|stripe| MeshParent { stripe, via: None })
                    .collect();
            }
            viewer.mask.store(mask, Ordering::Relaxed);
            let home = self.plan.home.get(&me).map(|k| k.1);
            let children: Vec<MeshPeer> = home
                .and_then(|h| self.plan.parents.get(&tree(h)))
                .map(|p| {
                    let mut kids: Vec<MeshPeer> = p
                        .iter()
                        .filter(|(_, parent)| **parent == me)
                        .filter_map(|(child, _)| by_peer.get(child).map(|v| peer_of(v)))
                        .collect();
                    kids.sort_by(|a, b| a.id.cmp(&b.id));
                    kids
                })
                .unwrap_or_default();
            let assignment = Assignment {
                parents,
                home: home.filter(|_| !children.is_empty()),
                children,
            };
            if self.sent.get(&viewer.id) != Some(&assignment) {
                out.push((
                    viewer.id.clone(),
                    Signal::MeshAssign {
                        parents: assignment.parents.clone(),
                        home: assignment.home,
                        children: assignment.children.clone(),
                    },
                ));
                self.sent.insert(viewer.id.clone(), assignment);
            }
        }
        out
    }
}

/// Uma máscara nova (todas as faixas direto até o primeiro plano).
pub fn new_mask() -> StripeMask {
    Arc::new(AtomicU8::new(ALL_STRIPES))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer(n: u64, upload: u32, nat: MeshNat) -> MeshViewer {
        MeshViewer {
            id: format!("{n:016x}"),
            fingerprint: format!("fp{n}"),
            candidates: vec![format!("10.0.0.{n}:5000").parse().unwrap()],
            hello: MeshHello {
                nat,
                upload_kbps: upload,
            },
            mask: new_mask(),
        }
    }

    fn coordinator() -> MeshCoordinator {
        MeshCoordinator::new("00000000000000ff")
    }

    /// De quem cada membro recebe cada faixa, pelo último aviso (`None` = direto).
    fn routes(c: &MeshCoordinator) -> HashMap<String, Vec<(u8, Option<String>)>> {
        c.sent
            .iter()
            .map(|(id, a)| {
                (
                    id.clone(),
                    a.parents
                        .iter()
                        .map(|p| (p.stripe, p.via.as_ref().map(|v| v.id.clone())))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn ids_da_sala_viram_peer_ids_e_nat_desconhecido_vira_nat_comum() {
        assert_eq!(peer_id("00000000000000ff"), PeerId(255));
        assert_ne!(peer_id("Ana"), PeerId(0));
        assert_eq!(peer_id("Ana"), peer_id("Ana"));
        assert_eq!(mesh_nat(sv_net::NatMapping::EndpointIndependent), MeshNat::Cone);
        assert_eq!(mesh_nat(sv_net::NatMapping::Unknown), MeshNat::Cone);
        assert_eq!(mesh_nat(sv_net::NatMapping::Symmetric), MeshNat::Symmetric);
    }

    #[test]
    fn quem_nao_cabe_no_plano_recebe_as_faixas_de_dados_direto() {
        let mut c = coordinator();
        let viewers: Vec<_> = (1..=12).map(|n| viewer(n, 0, MeshNat::Cone)).collect();
        let masks: Vec<_> = viewers.iter().map(|v| v.mask.clone()).collect();
        for v in viewers {
            c.join(v);
        }
        // Ninguém doa: quem transmite serve quem cabe, e os outros recebem como sem a malha.
        let fallback = masks
            .iter()
            .filter(|m| m.load(Ordering::Relaxed) == DATA_STRIPES_ONLY)
            .count();
        assert!(fallback > 0);
        for parents in routes(&c).values() {
            assert!(parents.len() >= DATA_STRIPES, "{parents:?}");
        }
    }

    #[test]
    fn com_dois_assistindo_tudo_vai_direto() {
        let mut c = coordinator();
        let ana = viewer(1, VIEWER_UPLOAD_KBPS, MeshNat::Cone);
        let beto = viewer(2, VIEWER_UPLOAD_KBPS, MeshNat::Cone);
        let (ana_mask, beto_mask) = (ana.mask.clone(), beto.mask.clone());
        c.join(ana);
        let out = c.join(beto);
        assert!(!out.is_empty());
        // Tudo direto: só as faixas de dados, sem pagar a paridade (o mesmo que sem a malha).
        assert_eq!(ana_mask.load(Ordering::Relaxed), DATA_STRIPES_ONLY);
        assert_eq!(beto_mask.load(Ordering::Relaxed), DATA_STRIPES_ONLY);
        assert!(
            routes(&c)
                .values()
                .all(|p| p.len() == DATA_STRIPES && p.iter().all(|(_, via)| via.is_none()))
        );
    }

    #[test]
    fn sala_maior_repassa_e_quem_transmite_manda_no_maximo_duas_copias() {
        let mut c = coordinator();
        let viewers: Vec<_> = (1..=8).map(|n| viewer(n, 20_000, MeshNat::Cone)).collect();
        let masks: Vec<_> = viewers.iter().map(|v| v.mask.clone()).collect();
        for v in viewers {
            c.join(v);
        }
        let direct: u32 = masks.iter().map(|m| m.load(Ordering::Relaxed).count_ones()).sum();
        assert!(direct <= 2 * STRIPES as u32, "quem transmite manda {direct} faixas");
        let r = routes(&c);
        assert!(r.values().flatten().any(|(_, via)| via.is_some()), "ninguém repassou");
        for (id, parents) in &r {
            assert!(parents.len() >= STRIPES - 1, "{id} recebe só {} faixas", parents.len());
        }
        // Todo filho anunciado a um pai é quem recebe dele.
        for (parent_id, a) in &c.sent {
            for kid in &a.children {
                let home = a.home.expect("tem filhos, tem casa");
                assert!(r[&kid.id].contains(&(home, Some(parent_id.clone()))));
            }
        }
    }

    #[test]
    fn quem_esta_atras_de_cgnat_so_recebe() {
        let mut c = coordinator();
        for n in 1..=8 {
            let nat = if n == 3 { MeshNat::Symmetric } else { MeshNat::Cone };
            c.join(viewer(n, 20_000, nat));
        }
        let cgnat = format!("{:016x}", 3);
        assert!(c.sent[&cgnat].children.is_empty());
        assert_eq!(c.sent[&cgnat].home, None);
    }

    #[test]
    fn saida_de_um_pai_avisa_so_quem_muda() {
        let mut c = coordinator();
        // Em sala pequena, quem sai é pai de todos na árvore dele: com 30, nem todos dependem dele.
        for n in 1..=30 {
            c.join(viewer(n, 20_000, MeshNat::Cone));
        }
        let parent = c
            .sent
            .iter()
            .find(|(_, a)| !a.children.is_empty())
            .map(|(id, a)| (id.clone(), a.children.iter().map(|k| k.id.clone()).collect::<Vec<_>>()))
            .expect("alguém repassa");
        let out = c.leave(&parent.0);
        let told: HashSet<String> = out.iter().map(|(id, _)| id.clone()).collect();
        for kid in &parent.1 {
            assert!(told.contains(kid), "{kid} perdeu o pai e não foi avisado");
        }
        assert!(!told.contains(&parent.0));
        // Quem não dependia do que saiu e não ganhou filho novo não recebe aviso.
        assert!(told.len() < c.len(), "avisou todo mundo: {told:?}");
        // Sair de novo não faz nada.
        assert!(c.leave(&parent.0).is_empty());
    }

    #[test]
    fn faixa_perdida_troca_o_pai_sem_voltar_para_ele() {
        let mut c = coordinator();
        for n in 1..=8 {
            c.join(viewer(n, 20_000, MeshNat::Cone));
        }
        let (child, stripe, old_parent) = routes(&c)
            .into_iter()
            .find_map(|(id, ps)| ps.into_iter().find_map(|(s, via)| via.map(|v| (id.clone(), s, v))))
            .expect("alguém recebe de um membro");
        let out = c.lost(&child, stripe);
        assert!(out.iter().any(|(id, _)| *id == child), "{child} não ganhou pai novo");
        let now = routes(&c)[&child].iter().find(|(s, _)| *s == stripe).cloned();
        assert_ne!(now, Some((stripe, Some(old_parent))), "voltou para o pai que falhou");
        // Faixa ou membro inválidos: nada muda.
        assert!(c.lost(&child, 9).is_empty());
        assert!(c.lost("ninguem", 0).is_empty());
    }
}
