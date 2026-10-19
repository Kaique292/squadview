//! Contra o Worker de sinalização rodando de verdade (local: `yarn workspace @squadview/signaling dev`).
//! Rode com: `SQUADVIEW_SIGNALING_URL=ws://127.0.0.1:8787 cargo test -p sv-net --test signaling_live -- --ignored`

use std::time::Duration;

use sv_net::protocol::Signal;
use sv_net::signaling::new_room_code;
use sv_net::{RoomClient, RoomEvent};
use tokio::sync::mpsc::UnboundedReceiver;

async fn next(rx: &mut UnboundedReceiver<RoomEvent>) -> RoomEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("nada em 5 s")
        .expect("canal fechou")
}

#[tokio::test]
#[ignore = "precisa do Worker de sinalização rodando (SQUADVIEW_SIGNALING_URL)"]
async fn dois_amigos_se_encontram_na_sala_e_trocam_sinais() {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").unwrap_or_else(|_| "ws://127.0.0.1:8787".into());
    let code = new_room_code();

    let (ana, mut ana_rx) = RoomClient::join(&url, &code, "Ana", Some("Galera")).await.unwrap();
    let RoomEvent::Joined {
        you: ana_id, members, ..
    } = next(&mut ana_rx).await
    else {
        panic!("sem welcome")
    };
    assert!(members.is_empty());

    let (beto, mut beto_rx) = RoomClient::join(&url, &code, "Beto", None).await.unwrap();
    let RoomEvent::Joined {
        you: beto_id,
        members,
        room,
    } = next(&mut beto_rx).await
    else {
        panic!("sem welcome")
    };
    assert_eq!(members[0].name, "Ana");
    // Ana criou a sala com nome; Beto recebe o nome ao entrar.
    assert_eq!(room.as_deref(), Some("Galera"));
    assert!(matches!(next(&mut ana_rx).await, RoomEvent::MemberJoined(m) if m.name == "Beto"));

    ana.set_live(true);
    assert!(matches!(next(&mut beto_rx).await, RoomEvent::MemberUpdated(m) if m.live && m.id == ana_id));

    let request = Signal::WatchRequest {
        candidates: vec!["127.0.0.1:5000".parse().unwrap()],
        fingerprint: "ab".into(),
    };
    beto.signal(&ana_id, &request);
    assert_eq!(
        next(&mut ana_rx).await,
        RoomEvent::Signal {
            from: beto_id.clone(),
            signal: request
        }
    );

    drop(beto);
    assert_eq!(next(&mut ana_rx).await, RoomEvent::MemberLeft(beto_id));
}

/// O ping a cada 20 s mantém a sala viva: parado por mais que o limite de silêncio (45 s), o
/// cliente não pode achar que caiu.
#[tokio::test]
#[ignore = "precisa do Worker de sinalização rodando (SQUADVIEW_SIGNALING_URL); leva ~70 s"]
async fn fica_na_sala_parado_sem_cair() {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").unwrap_or_else(|_| "ws://127.0.0.1:8787".into());
    let code = new_room_code();
    let (_ana, mut ana_rx) = RoomClient::join(&url, &code, "Ana", None).await.unwrap();
    assert!(matches!(next(&mut ana_rx).await, RoomEvent::Joined { .. }));
    let quiet = tokio::time::timeout(std::time::Duration::from_secs(70), ana_rx.recv()).await;
    assert!(quiet.is_err(), "a sala mandou algo ou caiu: {quiet:?}");
}
