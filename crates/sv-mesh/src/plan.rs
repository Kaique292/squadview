//! Planejador da malha em enxame (ver `docs/adr/0012-malha-em-enxame.md`).
//!
//! O vídeo de quem transmite é cortado em `K` faixas, mais `P` faixas de paridade (FEC): com
//! quaisquer `K` das `K + P` faixas, dá para remontar a imagem. Cada faixa desce por uma árvore
//! própria, e cada membro repassa **só na sua faixa** (a "faixa da casa"): ele é galho numa árvore
//! e folha nas outras. Assim:
//!
//! - o esforço de repasse se espalha por todo mundo, como num torrent;
//! - a saída de um membro corta no máximo uma faixa de quem estava abaixo dele, e a paridade cobre
//!   o buraco sem congelar a imagem;
//! - quem chega primeiro na árvore é quem tem mais upload, então as árvores ficam rasas.
//!
//! É lógica pura (sem rede), para ser testada e simulada com milhares de membros.

use std::collections::HashMap;

use sv_core::{PeerId, QualityLayer};

/// Tipo de NAT, que decide quem consegue abrir conexão direta com quem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nat {
    /// Aceita conexão de qualquer um (porta aberta ou NAT "full cone").
    Open,
    /// NAT comum de roteador doméstico: fura com quem é aberto ou também é cone.
    Cone,
    /// CGNAT ou NAT simétrico: só fala direto com quem é aberto.
    Symmetric,
}

impl Nat {
    /// Os dois conseguem abrir uma conexão direta (com hole punching)?
    pub fn connects(self, other: Nat) -> bool {
        matches!((self, other), (Nat::Open, _) | (_, Nat::Open) | (Nat::Cone, Nat::Cone))
    }
}

/// O que o planejador sabe de cada membro.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub id: PeerId,
    pub upload_kbps: u32,
    pub download_kbps: u32,
    pub nat: Nat,
    /// Jogando agora: repassa só um pouco, para não dar lag na partida.
    pub in_game: bool,
}

/// Parâmetros do planejador.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Faixas de dados (`K`).
    pub data_stripes: u8,
    /// Faixas de paridade (`P`): quantas faixas podem faltar sem a imagem congelar.
    pub parity_stripes: u8,
    /// Fração do upload que pode ser usada para repassar (o resto fica de folga).
    pub headroom: f32,
    /// Máximo que quem está jogando doa para repassar, em kbit/s.
    pub in_game_kbps: u32,
    /// Quantas cópias completas do stream quem transmite manda, no máximo.
    pub source_copies: u32,
    /// Tempo de cada salto além da rede (receber, conferir, repassar), em ms.
    pub hop_ms: u32,
    /// Repassador na nuvem (TURN ou MoQ), só para quem não alcança ninguém direto (CGNAT).
    /// Upload dele em kbit/s; 0 = sem repassador.
    pub relay_kbps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_stripes: 4,
            parity_stripes: 1,
            headroom: 0.75,
            in_game_kbps: 1_500,
            source_copies: 2,
            hop_ms: 2,
            relay_kbps: 0,
        }
    }
}

impl Config {
    pub fn total_stripes(&self) -> u8 {
        self.data_stripes + self.parity_stripes
    }

    /// Bitrate de uma faixa da camada, em kbit/s.
    pub fn stripe_kbps(&self, layer: QualityLayer) -> u32 {
        layer.target_kbps().div_ceil(u32::from(self.data_stripes))
    }

    /// O que um membro baixa ao receber todas as faixas da camada.
    pub fn download_needed(&self, layer: QualityLayer) -> u32 {
        self.stripe_kbps(layer) * u32::from(self.total_stripes())
    }
}

/// Id do repassador na nuvem nos planos (não é um membro da sala).
pub const RELAY: PeerId = PeerId(u64::MAX);

/// Uma árvore: a faixa `stripe` da camada `layer`.
pub type TreeKey = (QualityLayer, u8);

/// Resultado do planejamento.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Camada que cada membro recebe; `None` = ficou sem vídeo.
    pub layer: HashMap<PeerId, Option<QualityLayer>>,
    /// Para cada árvore, o pai de cada membro nela.
    pub parents: HashMap<TreeKey, HashMap<PeerId, PeerId>>,
    /// Faixa da casa de cada membro (a única em que ele repassa).
    pub home: HashMap<PeerId, TreeKey>,
    /// Atraso de cada membro até poder montar a imagem (a `K`-ésima faixa mais rápida), em ms.
    pub latency_ms: HashMap<PeerId, u32>,
    /// Upload usado por cada membro (e por quem transmite), em kbit/s.
    pub used_kbps: HashMap<PeerId, u32>,
}

impl Plan {
    /// Quantos saltos a faixa leva de quem transmite até `id` na árvore `key`.
    pub fn depth(&self, key: TreeKey, id: PeerId) -> Option<u32> {
        let parents = self.parents.get(&key)?;
        let mut depth = 0;
        let mut at = id;
        while let Some(&p) = parents.get(&at) {
            depth += 1;
            at = p;
            if depth > 10_000 {
                return None; // Ciclo: nunca deveria acontecer.
            }
        }
        (depth > 0).then_some(depth)
    }
}

/// Quanto cada membro pode doar para repassar.
fn budget(m: &Member, cfg: &Config) -> u32 {
    let usable = (m.upload_kbps as f32 * cfg.headroom) as u32;
    if m.in_game {
        usable.min(cfg.in_game_kbps)
    } else {
        usable
    }
}

/// Planeja as árvores para `members` assistirem `source`. `rtt` dá a latência de ida e volta
/// entre dois membros, em ms.
/// Com `cfg.relay_kbps > 0`, `rtt` também precisa responder pelo [`RELAY`].
pub fn plan(source: &Member, members: &[Member], rtt: &dyn Fn(PeerId, PeerId) -> u32, cfg: &Config) -> Plan {
    // Começa com a camada que o download de cada um aguenta; quem não couber na alta (falta de
    // upload na sala) desce para a baixa, e o plano é refeito. Converge em poucas voltas.
    let mut wanted: HashMap<PeerId, Option<QualityLayer>> = members
        .iter()
        .map(|m| {
            let layer = if m.download_kbps >= cfg.download_needed(QualityLayer::High) {
                QualityLayer::High
            } else {
                QualityLayer::Low
            };
            (m.id, Some(layer))
        })
        .collect();
    for _ in 0..6 {
        let attempt = place(source, members, rtt, cfg, &wanted);
        let mut changed = false;
        for m in members {
            let got = attempt.layer.get(&m.id).copied().flatten();
            let want = wanted[&m.id];
            if got != want {
                // Não coube na alta: tenta a baixa. Não coube na baixa: fica sem vídeo.
                let next = match want {
                    Some(QualityLayer::High) => Some(QualityLayer::Low),
                    _ => None,
                };
                if next != want {
                    wanted.insert(m.id, next);
                    changed = true;
                }
            }
        }
        if !changed {
            return attempt;
        }
    }
    place(source, members, rtt, cfg, &wanted)
}

/// Uma volta do planejamento, com a camada de cada membro já decidida.
///
/// 1. Cada membro que pode repassar ganha uma faixa da casa (equilibrando a capacidade).
/// 2. **Troncos:** em cada árvore, quem tem ela como casa entra primeiro, do maior upload para o
///    menor, pendurado em quem transmite (que divide as cópias entre as árvores) ou em outro do
///    tronco. Só o tronco repassa.
/// 3. **Folhas:** todo mundo pega as faixas que faltam de quem está no tronco de cada árvore.
fn place(
    source: &Member,
    members: &[Member],
    rtt: &dyn Fn(PeerId, PeerId) -> u32,
    cfg: &Config,
    wanted: &HashMap<PeerId, Option<QualityLayer>>,
) -> Plan {
    let total = cfg.total_stripes();
    let mut plan = Plan::default();
    let mut left: HashMap<PeerId, u32> = members.iter().map(|m| (m.id, budget(m, cfg))).collect();
    let source_budget = ((source.upload_kbps as f32 * cfg.headroom) as u32)
        .min(cfg.download_needed(QualityLayer::High) * cfg.source_copies);
    let nat: HashMap<PeerId, Nat> = members
        .iter()
        .map(|m| (m.id, m.nat))
        .chain([(source.id, source.nat)])
        .collect();

    // Quem tem mais para doar vem primeiro: fica perto da raiz e as árvores ficam rasas.
    let mut order: Vec<&Member> = members.iter().filter(|m| wanted[&m.id].is_some()).collect();
    order.sort_by(|a, b| left[&b.id].cmp(&left[&a.id]).then(a.id.0.cmp(&b.id.0)));

    // Árvores em uso (as camadas de quem vai assistir) e a cota de quem transmite em cada uma:
    // as cópias dele são divididas igualmente, para nenhuma faixa ficar sem semente.
    let mut layers: Vec<QualityLayer> = order.iter().filter_map(|m| wanted[&m.id]).collect();
    layers.sort();
    layers.dedup();
    let trees: Vec<TreeKey> = layers.iter().flat_map(|&l| (0..total).map(move |s| (l, s))).collect();
    let mut source_left: HashMap<TreeKey, u32> = trees
        .iter()
        .map(|&k| (k, source_budget / trees.len().max(1) as u32))
        .collect();

    // 1. Faixa da casa: só para quem consegue repassar ao menos uma faixa e não está atrás de
    //    CGNAT (só falaria com quem é aberto). Os outros só recebem.
    let mut home_capacity: HashMap<TreeKey, u64> = HashMap::new();
    for m in &order {
        let layer = wanted[&m.id].expect("filtrado acima");
        if m.nat == Nat::Symmetric || left[&m.id] < cfg.stripe_kbps(layer) {
            continue;
        }
        let key = (0..total)
            .map(|s| (layer, s))
            .min_by_key(|k| (home_capacity.get(k).copied().unwrap_or(0), k.1))
            .expect("pelo menos uma faixa");
        *home_capacity.entry(key).or_default() += u64::from(left[&m.id]);
        plan.home.insert(m.id, key);
    }

    // Chegada de cada faixa em quem está no tronco de cada árvore (ms desde quem transmite).
    let mut trunk: HashMap<TreeKey, Vec<(PeerId, u32)>> = HashMap::new();
    let mut received: HashMap<PeerId, Vec<u32>> = HashMap::new();
    // Melhor pai para `m` na árvore `key`: quem transmite (se ainda tem cota) ou alguém do tronco.
    let best_parent = |key: TreeKey,
                       m: &Member,
                       trunk: &HashMap<TreeKey, Vec<(PeerId, u32)>>,
                       left: &HashMap<PeerId, u32>,
                       source_left: &HashMap<TreeKey, u32>|
     -> Option<(u32, PeerId)> {
        let stripe_kbps = cfg.stripe_kbps(key.0);
        let from_source = (source_left[&key] >= stripe_kbps && source.nat.connects(m.nat))
            .then(|| (rtt(source.id, m.id) / 2 + cfg.hop_ms, source.id));
        let from_trunk = trunk
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|(p, _)| *p != m.id && left[p] >= stripe_kbps && nat[p].connects(m.nat))
            .map(|&(p, t)| (t + rtt(p, m.id) / 2 + cfg.hop_ms, p))
            .min_by_key(|&(t, p)| (t, p.0));
        match (from_source, from_trunk) {
            (Some(a), Some(b)) => Some(a.min_by_key_pair(b)),
            (a, b) => a.or(b),
        }
    };

    // Repassador na nuvem: recebe cada faixa direto de quem transmite (uma cópia a mais) e fica
    // de reserva no tronco de todas as árvores.
    let mut relay_left = 0u32;
    let relay_budget = cfg.relay_kbps;
    let mut relay_in: Vec<TreeKey> = Vec::new();
    if relay_budget > 0 {
        relay_left = relay_budget;
        for &key in &trees {
            let stripe_kbps = cfg.stripe_kbps(key.0);
            if source_left[&key] >= stripe_kbps {
                *source_left.get_mut(&key).expect("árvore em uso") -= stripe_kbps;
                plan.parents.entry(key).or_default().insert(RELAY, source.id);
                relay_in.push(key);
            }
        }
    }
    let relay_arrival = |m: PeerId| rtt(source.id, RELAY) / 2 + cfg.hop_ms + rtt(RELAY, m) / 2 + cfg.hop_ms;

    // 2. Troncos.
    for m in &order {
        let Some(&key) = plan.home.get(&m.id) else { continue };
        let stripe_kbps = cfg.stripe_kbps(key.0);
        if let Some((t, p)) = best_parent(key, m, &trunk, &left, &source_left) {
            if p == source.id {
                *source_left.get_mut(&key).expect("árvore em uso") -= stripe_kbps;
            } else {
                *left.get_mut(&p).expect("conhecido") -= stripe_kbps;
            }
            plan.parents.entry(key).or_default().insert(m.id, p);
            trunk.entry(key).or_default().push((m.id, t));
            received.entry(m.id).or_default().push(t);
        } else {
            // Não coube no tronco: vira só folha.
            plan.home.remove(&m.id);
        }
    }

    // 3. Folhas: as faixas que faltam, de quem está no tronco.
    for m in &order {
        let layer = wanted[&m.id].expect("filtrado acima");
        let stripe_kbps = cfg.stripe_kbps(layer);
        for s in 0..total {
            let key = (layer, s);
            if plan.home.get(&m.id) == Some(&key) {
                continue; // Já recebe pelo tronco.
            }
            if let Some((t, p)) = best_parent(key, m, &trunk, &left, &source_left) {
                if p == source.id {
                    *source_left.get_mut(&key).expect("árvore em uso") -= stripe_kbps;
                } else {
                    *left.get_mut(&p).expect("conhecido") -= stripe_kbps;
                }
                plan.parents.entry(key).or_default().insert(m.id, p);
                received.entry(m.id).or_default().push(t);
            } else if relay_in.contains(&key) && relay_left >= stripe_kbps {
                // Ninguém alcançável com vaga: o repassador na nuvem serve (só esta faixa).
                relay_left -= stripe_kbps;
                plan.parents.entry(key).or_default().insert(m.id, RELAY);
                received.entry(m.id).or_default().push(relay_arrival(m.id));
            }
        }
        let times = received.entry(m.id).or_default();
        times.sort_unstable();
        let ok = times.len() >= usize::from(cfg.data_stripes);
        plan.layer.insert(m.id, ok.then_some(layer));
        if ok {
            plan.latency_ms.insert(m.id, times[usize::from(cfg.data_stripes) - 1]);
        }
    }
    for m in members {
        plan.layer.entry(m.id).or_insert(None);
        plan.used_kbps.insert(m.id, budget(m, cfg) - left[&m.id]);
    }
    let source_unused: u32 = source_left.values().sum();
    plan.used_kbps
        .insert(source.id, source_budget.saturating_sub(source_unused));
    if relay_budget > 0 {
        plan.used_kbps.insert(RELAY, relay_budget - relay_left);
    }
    plan
}

/// Escolhe o menor `(tempo, id)` entre dois candidatos.
trait MinPair {
    fn min_by_key_pair(self, other: Self) -> Self;
}

impl MinPair for (u32, PeerId) {
    fn min_by_key_pair(self, other: Self) -> Self {
        if (other.0, other.1.0) < (self.0, self.1.0) {
            other
        } else {
            self
        }
    }
}

/// O que acontece com quem continua quando alguns membros saem de repente.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Impact {
    /// Perderam faixas, mas a paridade cobre: a imagem não congela.
    pub covered: usize,
    /// Perderam mais faixas do que a paridade cobre: congela até a malha se refazer.
    pub frozen: usize,
}

/// Quem fica sem quantas faixas se os membros `gone` saírem agora (antes de a malha se refazer).
pub fn impact(plan: &Plan, cfg: &Config, gone: &[PeerId]) -> Impact {
    let gone: std::collections::HashSet<PeerId> = gone.iter().copied().collect();
    let mut result = Impact::default();
    for (&id, layer) in &plan.layer {
        let Some(layer) = layer else { continue };
        if gone.contains(&id) {
            continue;
        }
        let mut lost = 0u8;
        let mut received = 0u8;
        for s in 0..cfg.total_stripes() {
            let Some(parents) = plan.parents.get(&(*layer, s)) else {
                continue;
            };
            if !parents.contains_key(&id) {
                continue;
            }
            received += 1;
            // Sobe até quem transmite: se algum ancestral saiu, a faixa parou de chegar.
            let mut at = id;
            while let Some(&p) = parents.get(&at) {
                if gone.contains(&p) {
                    lost += 1;
                    break;
                }
                at = p;
            }
        }
        if lost == 0 {
            continue;
        }
        if received - lost >= cfg.data_stripes {
            result.covered += 1;
        } else {
            result.frozen += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: u64, upload: u32, nat: Nat) -> Member {
        Member {
            id: PeerId(id),
            upload_kbps: upload,
            download_kbps: 100_000,
            nat,
            in_game: false,
        }
    }

    fn flat(_: PeerId, _: PeerId) -> u32 {
        20
    }

    fn source() -> Member {
        Member {
            in_game: true,
            ..member(0, 50_000, Nat::Cone)
        }
    }

    #[test]
    fn nat_simetrico_so_fala_com_quem_e_aberto() {
        assert!(Nat::Symmetric.connects(Nat::Open));
        assert!(!Nat::Symmetric.connects(Nat::Cone));
        assert!(!Nat::Symmetric.connects(Nat::Symmetric));
        assert!(Nat::Cone.connects(Nat::Cone));
    }

    #[test]
    fn sala_pequena_recebe_direto_de_quem_transmite_em_1080p() {
        let cfg = Config::default();
        let members: Vec<_> = (1..=2).map(|i| member(i, 50_000, Nat::Cone)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        for m in &members {
            assert_eq!(plan.layer[&m.id], Some(QualityLayer::High));
            assert_eq!(plan.latency_ms[&m.id], 10 + cfg.hop_ms);
        }
    }

    #[test]
    fn ninguem_passa_do_proprio_upload() {
        let cfg = Config::default();
        let members: Vec<_> = (1..=60)
            .map(|i| member(i, [3_000, 10_000, 50_000][i as usize % 3], Nat::Cone))
            .collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        for m in &members {
            assert!(plan.used_kbps[&m.id] <= budget(m, &cfg), "{:?}", m.id);
        }
        assert!(plan.used_kbps[&PeerId(0)] <= cfg.download_needed(QualityLayer::High) * cfg.source_copies);
    }

    #[test]
    fn cada_um_repassa_so_na_propria_faixa_e_respeita_o_nat() {
        let cfg = Config::default();
        let members: Vec<_> = (1..=80)
            .map(|i| {
                let nat = [Nat::Open, Nat::Cone, Nat::Symmetric][i as usize % 3];
                member(i, 30_000, nat)
            })
            .collect();
        let nats: HashMap<PeerId, Nat> = members.iter().map(|m| (m.id, m.nat)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        for (key, parents) in &plan.parents {
            for (child, parent) in parents {
                if *parent != PeerId(0) {
                    assert_eq!(plan.home[parent], *key, "{parent:?} repassou fora da casa");
                    assert!(nats[parent].connects(nats[child]));
                }
                assert!(plan.depth(*key, *child).is_some(), "ciclo na árvore");
            }
        }
    }

    #[test]
    fn upload_curto_na_sala_desce_para_720p_em_vez_de_travar() {
        let cfg = Config::default();
        // Quem transmite manda só 2 cópias; os outros quase não têm upload.
        let members: Vec<_> = (1..=20).map(|i| member(i, 1_000, Nat::Cone)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        let low = plan.layer.values().filter(|l| **l == Some(QualityLayer::Low)).count();
        let high = plan.layer.values().filter(|l| **l == Some(QualityLayer::High)).count();
        assert!(low > 0, "ninguém desceu para 720p");
        assert!(high >= 1);
    }

    /// A nuvem é o último recurso: entra quando ninguém alcançável tem vaga (CGNAT ou falta de
    /// upload na sala), nunca no lugar de um caminho direto com vaga.
    #[test]
    fn repassador_na_nuvem_atende_quem_nao_alcanca_ninguem() {
        let cfg = Config {
            relay_kbps: 100_000,
            source_copies: 4,
            ..Config::default()
        };
        // Dois amigos de rede comum e um atrás de CGNAT, sem ninguém de rede aberta na sala.
        let members = vec![
            member(1, 50_000, Nat::Cone),
            member(2, 50_000, Nat::Cone),
            member(3, 50_000, Nat::Symmetric),
        ];
        let plan = plan(&source(), &members, &flat, &cfg);
        assert!(members.iter().all(|m| plan.layer[&m.id] == Some(QualityLayer::High)));
        for parents in plan.parents.values() {
            // Só o do CGNAT recebe do repassador; os outros, direto.
            assert_ne!(parents.get(&PeerId(1)), Some(&RELAY));
            assert_ne!(parents.get(&PeerId(2)), Some(&RELAY));
            assert_eq!(parents.get(&PeerId(3)), Some(&RELAY));
        }
        // Uma cópia do stream só para o do CGNAT (as 5 faixas).
        assert_eq!(plan.used_kbps[&RELAY], cfg.download_needed(QualityLayer::High));
    }

    #[test]
    fn repassador_tambem_cobre_falta_de_upload_na_sala() {
        let cfg = Config {
            relay_kbps: 100_000,
            ..Config::default()
        };
        // Quem transmite manda 2 cópias (uma vai para a nuvem) e os amigos quase não têm upload.
        let members: Vec<_> = (1..=4).map(|i| member(i, 1_000, Nat::Cone)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        assert!(members.iter().all(|m| plan.layer[&m.id].is_some()));
        assert!(plan.used_kbps[&RELAY] > 0);
    }

    #[test]
    fn sem_repassador_o_do_cgnat_fica_sem_video() {
        let members = vec![member(1, 50_000, Nat::Cone), member(2, 50_000, Nat::Symmetric)];
        let plan = plan(&source(), &members, &flat, &Config::default());
        assert_eq!(plan.layer[&PeerId(1)], Some(QualityLayer::High));
        assert_eq!(plan.layer[&PeerId(2)], None);
        assert!(!plan.used_kbps.contains_key(&RELAY));
    }

    #[test]
    fn saida_de_qualquer_um_nao_congela_ninguem_com_paridade() {
        let cfg = Config::default();
        let members: Vec<_> = (1..=150).map(|i| member(i, 20_000, Nat::Cone)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        for m in &members {
            let hit = impact(&plan, &cfg, &[m.id]);
            assert_eq!(hit.frozen, 0, "a saída de {:?} congelou alguém", m.id);
        }
    }

    #[test]
    fn sem_paridade_a_saida_de_um_galho_congela_os_de_baixo() {
        let cfg = Config {
            parity_stripes: 0,
            ..Config::default()
        };
        let members: Vec<_> = (1..=150).map(|i| member(i, 20_000, Nat::Cone)).collect();
        let plan = plan(&source(), &members, &flat, &cfg);
        let worst = members
            .iter()
            .map(|m| impact(&plan, &cfg, &[m.id]).frozen)
            .max()
            .unwrap();
        assert!(worst > 0);
    }
}
