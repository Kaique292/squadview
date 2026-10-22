//! Prova de conceito do fliperama (Etapa 0 de `docs/fliperama.md`), sem rede: lê um controle
//! físico pelo XInput e repete o estado num controle virtual de Xbox 360 do ViGEmBus. Com um jogo
//! de vários controles aberto, o controle físico passa a mexer o P1 (ele mesmo) e o P2 (o virtual).
//!
//! ```text
//! cargo run -p sv-input --example arcade_loopback -- [--from 0] [--seconds 60]
//! ```
//!
//! Para antes do tempo segurando Back + Start por 2 s. A cada 5 s mostra quantas atualizações
//! foram ao driver e quanto cada uma levou.

use std::time::{Duration, Instant};

use sv_input::pad::buttons;
use sv_input::{DriverStatus, PadState, VirtualPads, driver_status, find_uninstaller, xinput};

/// 250 leituras por segundo, como o jogador vai ler (ADR 0023).
const READ_EVERY: Duration = Duration::from_millis(4);
const REPORT_EVERY: Duration = Duration::from_secs(5);
const QUIT_HOLD: Duration = Duration::from_secs(2);

fn main() {
    let (from, seconds) = args();

    match driver_status() {
        DriverStatus::Installed => println!("ViGEmBus: instalado."),
        DriverStatus::Missing => {
            println!(
                "ViGEmBus: não instalado. Instale pelo instalador oficial:\n  {}",
                sv_input::driver::INSTALLER_URL
            );
            return;
        }
        other => {
            println!("ViGEmBus: com problema: {other:?}");
            return;
        }
    }
    match find_uninstaller() {
        Some(u) => println!(
            "Desinstalador: {} {} → {}",
            u.name,
            u.version.as_deref().unwrap_or("(sem versão)"),
            u.command
        ),
        None => println!("Desinstalador: não achado no registro (instalação fora do instalador oficial?)"),
    }

    let before = xinput::connected();
    println!("Vagas do XInput antes: {}", describe(before));
    let from = from.or_else(|| before.iter().position(|c| *c).map(|s| s as u32));
    let Some(from) = from else {
        println!("Nenhum controle físico ligado. Ligue um controle de Xbox e rode de novo.");
        return;
    };

    let pads = VirtualPads::connect().expect("conectar ao ViGEmBus");
    let pad = pads.plug().expect("plugar o controle virtual");
    let virtual_slot = pad.user_index().expect("o Windows não deu vaga ao controle virtual");
    println!(
        "Controle físico na vaga {from} (P{}) → controle virtual na vaga {virtual_slot} (P{}).",
        from + 1,
        virtual_slot + 1
    );
    println!("Vagas do XInput agora: {}", describe(xinput::connected()));
    println!("Repetindo por {seconds} s. Segure Back + Start por 2 s para parar antes.");

    let started = Instant::now();
    let mut last = PadState::default();
    let mut quit_since: Option<Instant> = None;
    let mut window = Stats::default();
    let mut next_report = started + REPORT_EVERY;

    while started.elapsed() < Duration::from_secs(seconds) {
        let tick = Instant::now();
        let state = xinput::read(from).unwrap_or_default();
        if state != last {
            let t = Instant::now();
            if let Err(e) = pad.update(&state) {
                println!("O driver recusou a atualização: {e}");
                break;
            }
            window.record(t.elapsed());
            last = state;
        }

        if state.pressed(buttons::BACK | buttons::START) {
            let since = *quit_since.get_or_insert(tick);
            if since.elapsed() >= QUIT_HOLD {
                println!("Back + Start: parando.");
                break;
            }
        } else {
            quit_since = None;
        }

        if tick >= next_report {
            window.print();
            window = Stats::default();
            next_report += REPORT_EVERY;
        }
        if let Some(rest) = READ_EVERY.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    window.print();
    drop(pad);
    println!(
        "Controle virtual removido. Vagas do XInput: {}",
        describe(xinput::connected())
    );
}

/// `--from N` (vaga do controle físico) e `--seconds N` (duração).
fn args() -> (Option<u32>, u64) {
    let mut from = None;
    let mut seconds = 60;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let value = it.next().and_then(|v| v.parse::<u64>().ok());
        match (arg.as_str(), value) {
            ("--from", Some(v)) if v < u64::from(xinput::SLOTS) => from = Some(v as u32),
            ("--seconds", Some(v)) => seconds = v,
            _ => {
                eprintln!("uso: arcade_loopback [--from 0..3] [--seconds N]");
                std::process::exit(2);
            }
        }
    }
    (from, seconds)
}

fn describe(slots: [bool; xinput::SLOTS as usize]) -> String {
    slots
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}:{}", i, if *c { "ocupada" } else { "livre" }))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Atualizações mandadas ao driver numa janela de tempo.
#[derive(Default)]
struct Stats {
    updates: u32,
    total: Duration,
    worst: Duration,
}

impl Stats {
    fn record(&mut self, took: Duration) {
        self.updates += 1;
        self.total += took;
        self.worst = self.worst.max(took);
    }

    fn print(&self) {
        if self.updates == 0 {
            println!("  sem mudanças no controle");
            return;
        }
        println!(
            "  {} atualizações; média {} µs, pior {} µs",
            self.updates,
            self.total.as_micros() / u128::from(self.updates),
            self.worst.as_micros()
        );
    }
}
