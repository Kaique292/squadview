//! Dois membros numa sala de verdade (Worker local), sem GPU: um fica ao vivo e o outro assiste.
//! Rode com o Worker local ligado (`yarn workspace @squadview/signaling dev`):
//! `cargo test -p sv-session --test room_live -- --ignored`

use std::sync::mpsc;
use std::time::Duration;

use sv_encode::EncodedFrame;
use sv_gpu::Size;
use sv_net::protocol::FrameHeader;
use sv_net::signaling::new_room_code;
use sv_net::{Receiver, ReceiverSink};
use sv_session::{JoinAs, KeyframeHandle, Room, RoomUpdate, StreamInfo, VoiceSettings};

struct Frames(mpsc::Sender<FrameHeader>);

impl ReceiverSink for Frames {
    fn video(&mut self, frame: FrameHeader, _data: Vec<u8>) {
        let _ = self.0.send(frame);
    }
    fn audio(&mut self, _: u32, _: u64, _: f32, _: &[u8]) {}
    fn closed(&mut self, _: String) {}
}

fn url() -> String {
    std::env::var("SQUADVIEW_SIGNALING_URL").unwrap_or_else(|_| "ws://127.0.0.1:8787".into())
}

#[test]
#[ignore = "precisa do Worker de sinalização rodando"]
fn um_amigo_assiste_o_outro_pela_sala() {
    let code = new_room_code();
    let (ana_tx, ana_rx) = mpsc::channel::<RoomUpdate>();
    let ana = Room::join(&url(), &code, JoinAs::named("Ana"), None, None, move |u| {
        let _ = ana_tx.send(u);
    })
    .expect("Ana entra");
    let (beto_tx, beto_rx) = mpsc::channel::<RoomUpdate>();
    let beto = Room::join(&url(), &code, JoinAs::named("Beto"), None, None, move |u| {
        let _ = beto_tx.send(u);
    })
    .expect("Beto entra");

    // Ana vê o Beto chegar.
    let saw_beto = (0..5).any(|_| {
        matches!(ana_rx.recv_timeout(Duration::from_secs(3)), Ok(RoomUpdate::Members(m)) if m.iter().any(|x| x.name == "Beto"))
    });
    assert!(saw_beto);

    // Ana fica ao vivo (sem GPU: entregamos os frames na mão pelo tap).
    let tap = ana.tap();
    tap.started(&StreamInfo {
        size: Size::new(1280, 720),
        fps: 30,
        encoder: "teste".into(),
    });
    ana.stream_started(KeyframeHandle::detached());

    // Beto vê a Ana ao vivo e pede para assistir.
    let ana_id = loop {
        match beto_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Beto não viu a Ana ao vivo")
        {
            RoomUpdate::Members(m) => {
                if let Some(a) = m.iter().find(|x| x.name == "Ana" && x.live) {
                    break a.id.clone();
                }
            }
            other => eprintln!("Beto: {other:?}"),
        }
    };
    beto.watch(&ana_id);
    let (conn, desc) = loop {
        match beto_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("sem resposta ao pedido")
        {
            RoomUpdate::WatchReady { conn, desc, member } => {
                assert_eq!(member.name, "Ana");
                break (conn, desc);
            }
            RoomUpdate::WatchFailed { reason, .. } => panic!("recusado: {reason}"),
            _ => {}
        }
    };
    assert_eq!((desc.width, desc.height, desc.fps), (1280, 720, 30));

    let (frames_tx, frames_rx) = mpsc::channel();
    let _guard = beto.runtime().enter();
    let _receiver = Receiver::start(conn, Frames(frames_tx));
    // Espera a Ana registrar o espectador antes de mandar.
    for _ in 0..50 {
        if ana.viewer_count() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(ana.viewer_count(), 1);
    for seq in 0..3u32 {
        tap.video(
            seq,
            &EncodedFrame {
                data: vec![1; 5000],
                keyframe: seq == 0,
                timestamp_us: u64::from(seq),
                encode_us: 1,
            },
        );
    }
    let got: Vec<u32> = (0..3)
        .map(|_| frames_rx.recv_timeout(Duration::from_secs(5)).expect("frame").seq)
        .collect();
    assert_eq!(got, vec![0, 1, 2]);

    // Ana para: o Beto é desconectado.
    ana.stream_stopped();
    assert_eq!(ana.viewer_count(), 0);
}

/// Voz: dois membros entram com voz e a malha liga os dois (quem tem o id menor chama).
/// Usa o microfone e a saída padrão deste PC.
#[test]
#[ignore = "precisa do Worker de sinalização, de microfone e de saída de áudio"]
fn dois_amigos_ligam_a_voz_ao_entrar() {
    let code = new_room_code();
    let (ana_tx, ana_rx) = mpsc::channel::<RoomUpdate>();
    let _ana = Room::join(
        &url(),
        &code,
        JoinAs::named("Ana"),
        None,
        Some(VoiceSettings::default()),
        move |u| {
            let _ = ana_tx.send(u);
        },
    )
    .expect("Ana entra");
    let (beto_tx, beto_rx) = mpsc::channel::<RoomUpdate>();
    let _beto = Room::join(
        &url(),
        &code,
        JoinAs::named("Beto"),
        None,
        Some(VoiceSettings::default()),
        move |u| {
            let _ = beto_tx.send(u);
        },
    )
    .expect("Beto entra");

    let linked = |rx: &mpsc::Receiver<RoomUpdate>| {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(RoomUpdate::VoicePeers(p)) if p.len() == 1 => return true,
                Ok(RoomUpdate::VoiceFailed { reason, .. }) => panic!("voz falhou: {reason}"),
                _ => {}
            }
        }
        false
    };
    assert!(linked(&ana_rx), "a voz da Ana não ligou");
    assert!(linked(&beto_rx), "a voz do Beto não ligou");
}
