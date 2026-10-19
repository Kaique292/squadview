//! Prévia local pelo terminal, sem a interface:
//!   cargo run -p sv-session --example preview --release            (monitor principal, 1080p60)
//!   cargo run -p sv-session --example preview --release -- 720      (720p30)
//!   cargo run -p sv-session --example preview --release -- 1440     (1440p60)
//!   cargo run -p sv-session --example preview --release -- janela "Counter-Strike 2"
//!   cargo run -p sv-session --example preview --release -- som 1234   (áudio do processo 1234)

use std::time::Duration;

use sv_capture::{CaptureSource, list_sources};
use sv_core::{Resolution, StreamQuality};
use sv_session::LocalPreview;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quality = if args.iter().any(|a| a == "720") {
        StreamQuality {
            resolution: Resolution::P720,
            fps: 30,
        }
    } else if args.iter().any(|a| a == "1440") {
        StreamQuality {
            resolution: Resolution::P1440,
            fps: 60,
        }
    } else {
        StreamQuality::DEFAULT
    };
    let sources = list_sources();
    let source = match args.iter().position(|a| a == "janela") {
        Some(i) => {
            let wanted = args.get(i + 1).map(String::as_str).unwrap_or_default().to_lowercase();
            sources
                .iter()
                .find(|s| matches!(s.id, CaptureSource::Window(_)) && s.name.to_lowercase().contains(&wanted))
        }
        None => sources.iter().find(|s| matches!(s.id, CaptureSource::Monitor(_))),
    }
    .expect("fonte não encontrada");
    println!("Capturando: {}  (feche a janela da prévia para sair)", source.name);

    let audio_pid = match args.iter().position(|a| a == "som") {
        Some(i) => args.get(i + 1).and_then(|p| p.parse().ok()),
        None => source.process_id,
    };
    let target = sv_session::WindowTarget::Standalone {
        title: "Prévia — SquadView".into(),
    };
    let preview = LocalPreview::start(source.id, quality, audio_pid, target, None, |s| {
        println!(
            "{}x{} | {:5.1} fps | {:6} kbps | encode {:4.1} ms | latência {:5.1} ms (p95 {:5.1}) | descartados {} | áudio {} kbps nível {:.2} | {}",
            s.width, s.height, s.fps, s.bitrate_kbps, s.encode_ms, s.latency_ms, s.latency_p95_ms, s.dropped, s.audio_kbps, s.audio_level, s.encoder
        );
    })
    .expect("não deu para iniciar a prévia");

    while !preview.is_closed() {
        std::thread::sleep(Duration::from_millis(200));
    }
}
