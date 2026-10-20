//! Simulador de sala (ADR 0012): gera uma população parecida com a internet brasileira, roda o
//! planejador e mede latência, qualidade, uso de upload e o efeito de gente saindo de repente.
//!
//! Determinístico (mesma semente, mesmo resultado), sem rede de verdade. Rode os cenários com
//! `cargo run -p sv-mesh --release --example simular`.

use std::collections::HashMap;
use std::fmt;

use sv_core::{PeerId, QualityLayer};

use crate::plan::{Config, Member, Nat, Plan, RELAY, impact, plan};

/// Gerador pseudoaleatório pequeno (SplitMix64): reprodutível e sem dependência.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Número em `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Verdadeiro com probabilidade `p` (0 a 1).
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }

    /// Escolhe um item pelos pesos.
    pub fn weighted<T: Copy>(&mut self, items: &[(T, u32)]) -> T {
        let total: u32 = items.iter().map(|(_, w)| w).sum();
        let mut roll = self.below(u64::from(total)) as u32;
        for &(item, w) in items {
            if roll < w {
                return item;
            }
            roll -= w;
        }
        items[items.len() - 1].0
    }
}

/// Cidades (para a latência entre membros): SP, RJ, BH, POA, Recife.
const CITIES: usize = 5;
/// RTT entre cidades, em ms (aproximado, rotas típicas de backbone no Brasil).
const CITY_RTT: [[u32; CITIES]; CITIES] = [
    [6, 12, 15, 25, 45],
    [12, 6, 12, 30, 40],
    [15, 12, 6, 32, 38],
    [25, 30, 32, 6, 60],
    [45, 40, 38, 60, 6],
];

/// Uma pessoa da sala simulada: o que o planejador vê mais onde ela está.
#[derive(Debug, Clone)]
pub struct Person {
    pub member: Member,
    pub city: usize,
    /// Latência da "última milha" (Wi-Fi, 4G, cabo), em ms de ida e volta.
    pub access_ms: u32,
}

/// Perfis de conexão (upload, download em kbit/s), com o peso de cada um na população.
const PROFILES: [((u32, u32), u32); 6] = [
    ((3_000, 20_000), 10),      // 4G
    ((5_000, 50_000), 10),      // cabo ou ADSL antigos
    ((20_000, 100_000), 20),    // fibra de entrada
    ((50_000, 300_000), 30),    // fibra comum (upload metade do download ou menos)
    ((150_000, 500_000), 20),   // fibra boa
    ((400_000, 1_000_000), 10), // fibra de 1 Gbps
];

/// Gera `n` pessoas com a mistura de conexões, NAT e cidades do Brasil.
pub fn population(n: usize, rng: &mut Rng) -> Vec<Person> {
    (0..n)
        .map(|i| {
            let (upload_kbps, download_kbps) = rng.weighted(&PROFILES);
            // Em torno de 1 em cada 4 está atrás de CGNAT (comum em 4G e operadoras menores).
            let nat = rng.weighted(&[(Nat::Open, 15), (Nat::Cone, 60), (Nat::Symmetric, 25)]);
            Person {
                member: Member {
                    id: PeerId(i as u64 + 1),
                    upload_kbps,
                    download_kbps,
                    nat,
                    in_game: rng.chance(0.2),
                },
                city: rng.weighted(&[(0, 40), (1, 20), (2, 15), (3, 10), (4, 15)]),
                access_ms: 2 + rng.below(14) as u32,
            }
        })
        .collect()
}

/// Quem transmite: está jogando, em São Paulo, com fibra comum (ou boa, para 1440p).
pub fn streamer_for(top: QualityLayer) -> Person {
    let mut person = streamer();
    if top == QualityLayer::Ultra {
        person.member.upload_kbps = 150_000;
    }
    person
}

/// Quem transmite em 1080p: está jogando, com fibra comum, em São Paulo.
pub fn streamer() -> Person {
    Person {
        member: Member {
            id: PeerId(0),
            upload_kbps: 50_000,
            download_kbps: 300_000,
            nat: Nat::Cone,
            in_game: true,
        },
        city: 0,
        access_ms: 4,
    }
}

/// Resumo de um cenário.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub members: usize,
    /// Fração que recebe 1440p60, 1080p60, 720p30 e nada.
    pub ultra: f64,
    pub high: f64,
    pub low: f64,
    pub none: f64,
    /// Atraso até montar a imagem, em ms (sem contar captura, encode e decode).
    pub latency_p50: u32,
    pub latency_p95: u32,
    pub latency_max: u32,
    /// Maior número de saltos numa árvore.
    pub max_depth: u32,
    /// Upload usado por quem transmite, em Mbit/s.
    pub source_mbps: f64,
    /// Média do upload doado por quem assiste, em Mbit/s.
    pub viewer_mbps: f64,
    /// Upload usado pelo repassador na nuvem, em Mbit/s (0 sem repassador).
    pub relay_mbps: f64,
    /// Ao saírem `churn` dos membros de uma vez: fração que congela e que a paridade cobre.
    pub churn: f64,
    pub frozen: f64,
    pub covered: f64,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:>7} | {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}% | {:>4} {:>4} {:>4} ms | {:>6} | {:>19.1} | {:>12.2} | {:>6.1} | {:>6.1}% {:>4.1}%",
            self.members,
            self.ultra * 100.0,
            self.high * 100.0,
            self.low * 100.0,
            self.none * 100.0,
            self.latency_p50,
            self.latency_p95,
            self.latency_max,
            self.max_depth,
            self.source_mbps,
            self.viewer_mbps,
            self.relay_mbps,
            self.frozen * 100.0,
            self.covered * 100.0,
        )
    }
}

/// Cabeçalho da tabela do [`Report`].
pub const REPORT_HEADER: &str = "membros |  1440p  1080p   720p   nada |  p50  p95  máx    | saltos | Mbps quem transmite | Mbps cada um | nuvem  | congela cobre";

/// RTT entre duas pessoas: a rota entre as cidades mais a última milha de cada lado.
pub fn rtt(a: &Person, b: &Person) -> u32 {
    CITY_RTT[a.city][b.city] + a.access_ms + b.access_ms
}

/// Roda um cenário: `n` membros, o planejador com `cfg`, e `churn` (0 a 1) saindo de uma vez.
pub fn run(n: usize, cfg: &Config, churn: f64, seed: u64) -> Report {
    let mut rng = Rng::new(seed);
    let people = population(n, &mut rng);
    let source = streamer_for(cfg.top_layer);
    // O repassador na nuvem fica num datacenter em São Paulo, com rede aberta.
    let relay = Person {
        member: Member {
            id: RELAY,
            upload_kbps: cfg.relay_kbps,
            download_kbps: u32::MAX,
            nat: Nat::Open,
            in_game: false,
        },
        city: 0,
        access_ms: 1,
    };
    let by_id: HashMap<PeerId, &Person> = people
        .iter()
        .chain([&source, &relay])
        .map(|p| (p.member.id, p))
        .collect();
    let members: Vec<Member> = people.iter().map(|p| p.member.clone()).collect();
    let rtt_fn = |a: PeerId, b: PeerId| rtt(by_id[&a], by_id[&b]);
    let plan = plan(&source.member, &members, &rtt_fn, cfg);
    let gone: Vec<PeerId> = members.iter().filter(|_| rng.chance(churn)).map(|m| m.id).collect();
    summarize(&plan, cfg, &source.member, &members, churn, &gone)
}

fn summarize(plan: &Plan, cfg: &Config, source: &Member, members: &[Member], churn: f64, gone: &[PeerId]) -> Report {
    let n = members.len().max(1) as f64;
    let count = |l: Option<QualityLayer>| plan.layer.values().filter(|v| **v == l).count() as f64 / n;
    let mut latencies: Vec<u32> = plan.latency_ms.values().copied().collect();
    latencies.sort_unstable();
    let pct = |p: f64| {
        latencies
            .get(((latencies.len() as f64 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0)
    };
    let max_depth = plan
        .parents
        .iter()
        .flat_map(|(key, parents)| parents.keys().filter_map(|id| plan.depth(*key, *id)))
        .max()
        .unwrap_or(0);
    let viewers_used: u64 = members.iter().map(|m| u64::from(plan.used_kbps[&m.id])).sum();
    let hit = impact(plan, cfg, gone);
    let staying = (members.len() - gone.len()).max(1) as f64;
    Report {
        members: members.len(),
        ultra: count(Some(QualityLayer::Ultra)),
        high: count(Some(QualityLayer::High)),
        low: count(Some(QualityLayer::Low)),
        none: count(None),
        latency_p50: pct(0.5),
        latency_p95: pct(0.95),
        latency_max: latencies.last().copied().unwrap_or(0),
        max_depth,
        source_mbps: f64::from(plan.used_kbps[&source.id]) / 1000.0,
        viewer_mbps: viewers_used as f64 / n / 1000.0,
        relay_mbps: f64::from(plan.used_kbps.get(&RELAY).copied().unwrap_or(0)) / 1000.0,
        churn,
        frozen: hit.frozen as f64 / staying,
        covered: hit.covered as f64 / staying,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gerador_e_reprodutivel() {
        let a: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..5).map(|_| r.next_u64()).collect()
        };
        let mut r = Rng::new(7);
        assert_eq!(a, (0..5).map(|_| r.next_u64()).collect::<Vec<_>>());
        assert!((0..1000).all(|_| r.below(10) < 10));
        assert_eq!(r.weighted(&[("só", 1)]), "só");
    }

    #[test]
    fn populacao_tem_a_mistura_do_brasil() {
        let people = population(2_000, &mut Rng::new(1));
        let cgnat = people.iter().filter(|p| p.member.nat == Nat::Symmetric).count();
        assert!((350..650).contains(&cgnat), "{cgnat}");
        assert!(people.iter().all(|p| p.city < CITIES && p.access_ms >= 2));
        assert!(people.iter().any(|p| p.member.in_game));
    }

    #[test]
    fn mesma_semente_mesmo_resultado() {
        let cfg = Config::default();
        assert_eq!(run(80, &cfg, 0.05, 3), run(80, &cfg, 0.05, 3));
    }

    /// O desenho proposto: malha com o repassador na nuvem só de reserva.
    fn proposed() -> Config {
        Config {
            relay_kbps: 1_000_000,
            ..Config::default()
        }
    }

    #[test]
    fn sem_repassador_quem_esta_atras_de_cgnat_fica_sem_video_em_sala_pequena() {
        let r = run(8, &Config::default(), 0.0, 42);
        assert!(r.none > 0.0, "{r}");
        assert_eq!(r.relay_mbps, 0.0);
    }

    #[test]
    fn sala_de_amigos_e_rapida_e_em_1080p() {
        let r = run(8, &proposed(), 0.0, 1);
        assert!(r.high >= 0.75, "{r}");
        assert_eq!(r.none, 0.0, "{r}");
        assert!(r.latency_p95 <= 60, "{r}");
    }

    #[test]
    fn em_1440p_a_sala_grande_divide_entre_1440p_e_1080p_sem_deixar_ninguem_sem_video() {
        let cfg = Config {
            top_layer: QualityLayer::Ultra,
            ..proposed()
        };
        let r = run(1_000, &cfg, 0.0, 5);
        assert!(r.ultra > 0.0, "{r}");
        assert!(r.none <= 0.02, "{r}");
        assert!(r.latency_p95 <= 250, "{r}");
    }

    #[test]
    fn mil_pessoas_ainda_assistem_com_atraso_de_call() {
        let r = run(1_000, &proposed(), 0.05, 2);
        assert!(r.none <= 0.02, "{r}");
        assert!(r.high >= 0.8, "{r}");
        assert!(r.latency_p95 <= 250, "{r}");
        // Quem transmite não manda mais que as cópias combinadas, não importa o tamanho da sala.
        assert!(r.source_mbps <= 25.0, "{r}");
        // 5% saindo de uma vez: a paridade cobre quase todo mundo.
        assert!(r.frozen <= 0.03, "{r}");
        // Em sala grande sempre há gente de rede aberta: o repassador na nuvem fica parado.
        assert!(r.relay_mbps <= 1.0, "{r}");
    }
}
