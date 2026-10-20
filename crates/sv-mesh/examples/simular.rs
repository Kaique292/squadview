//! Cenários da malha em enxame (ADR 0012): `cargo run -p sv-mesh --release --example simular`.

use sv_core::QualityLayer;
use sv_mesh::plan::Config;
use sv_mesh::sim::{REPORT_HEADER, run};

fn main() {
    let sizes = [2, 8, 30, 100, 1_000, 5_000];
    let scenarios = [
        (
            "Malha em enxame: 4 faixas + 1 de paridade, com repassador na nuvem de reserva (proposta)",
            Config {
                relay_kbps: 1_000_000,
                ..Config::default()
            },
        ),
        (
            "1440p60 escolhido por quem transmite (fibra de 150 Mbps), com repassador na nuvem",
            Config {
                relay_kbps: 1_000_000,
                top_layer: QualityLayer::Ultra,
                ..Config::default()
            },
        ),
        ("Malha em enxame sem repassador na nuvem", Config::default()),
        (
            "Sem paridade: 4 faixas",
            Config {
                parity_stripes: 0,
                ..Config::default()
            },
        ),
        (
            "Árvore única (desenho antigo da Fase 3)",
            Config {
                data_stripes: 1,
                parity_stripes: 0,
                ..Config::default()
            },
        ),
    ];
    for (title, cfg) in scenarios {
        println!("\n{title} — 5% saindo de uma vez");
        println!("{REPORT_HEADER}");
        for n in sizes {
            println!("{}", run(n, &cfg, 0.05, 42));
        }
    }
}
