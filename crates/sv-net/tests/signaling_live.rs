//! Contra o Worker de sinalização rodando de verdade. Roda pelo `yarn test:rs`, que sobe o Worker
//! local (`scripts/with-worker.mjs`) e define `SQUADVIEW_SIGNALING_URL`; sem ela, é pulado.

use std::time::Duration;

use sv_net::protocol::Signal;
use sv_net::signaling::new_room_code;
use sv_net::{RoomClient, RoomEvent};
use tokio::sync::mpsc::UnboundedReceiver;

/// Endereço do Worker local. `None`: teste pulado (rode pelo `yarn test:rs`).
fn worker() -> Option<String> {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").ok();
    if url.is_none() {
        eprintln!("pulado: rode pelo `yarn test:rs` (sobe o Worker local)");
    }
    url
}

async fn next(rx: &mut UnboundedReceiver<RoomEvent>) -> RoomEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("nada em 5 s")
        .expect("canal fechou")
}

#[tokio::test]
async fn dois_amigos_se_encontram_na_sala_e_trocam_sinais() {
    let Some(url) = worker() else { return };
    let code = new_room_code();

    let (ana, mut ana_rx) = RoomClient::join(&url, &code, "Ana", None, None, Some("Galera"), None, true)
        .await
        .unwrap();
    let RoomEvent::Joined {
        you: ana_id, members, ..
    } = next(&mut ana_rx).await
    else {
        panic!("sem welcome")
    };
    assert!(members.is_empty());

    let (beto, mut beto_rx) = RoomClient::join(&url, &code, "Beto", None, None, None, None, true)
        .await
        .unwrap();
    let RoomEvent::Joined {
        you: beto_id,
        members,
        room,
        ..
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
        simulcast: false,
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
    let (_ana, mut ana_rx) = RoomClient::join(&url, &code, "Ana", None, None, None, None, true)
        .await
        .unwrap();
    assert!(matches!(next(&mut ana_rx).await, RoomEvent::Joined { .. }));
    let quiet = tokio::time::timeout(std::time::Duration::from_secs(70), ana_rx.recv()).await;
    assert!(quiet.is_err(), "a sala mandou algo ou caiu: {quiet:?}");
}

/// Regressão (ADR 0022): a conexão da Ana cai sem `leave` e ela volta com o token do `welcome`.
/// Volta com o mesmo id, e o Beto não vê ninguém sair nem entrar (antes, via a Ana sair e entrar
/// de novo com outro id, e a voz entre os dois caía junto).
#[tokio::test]
async fn quem_cai_volta_com_o_mesmo_id_sem_os_outros_verem() {
    use futures_util::{SinkExt, StreamExt};
    use sv_net::Resume;
    use tokio_tungstenite::tungstenite::Message;

    let Some(url) = worker() else { return };
    let code = new_room_code();

    // A Ana entra "na mão" para poder derrubar a conexão sem mandar `leave`.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{url}/rooms/{code}"))
        .await
        .unwrap();
    ws.send(Message::Text(
        r#"{"type":"join","name":"Ana","protocol":1,"resumable":true}"#.into(),
    ))
    .await
    .unwrap();
    let welcome: serde_json::Value = loop {
        if let Some(Ok(Message::Text(t))) = ws.next().await {
            break serde_json::from_str(&t).unwrap();
        }
    };
    let back = Resume {
        id: welcome["you"].as_str().unwrap().to_owned(),
        token: welcome["resume"].as_str().expect("welcome com token").to_owned(),
    };

    let (_beto, mut beto_rx) = RoomClient::join(&url, &code, "Beto", None, None, None, None, true)
        .await
        .unwrap();
    assert!(matches!(next(&mut beto_rx).await, RoomEvent::Joined { .. }));

    drop(ws);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (_ana, mut ana_rx) = RoomClient::join(&url, &code, "Ana", None, None, None, Some(&back), true)
        .await
        .unwrap();
    let RoomEvent::Joined { you, members, .. } = next(&mut ana_rx).await else {
        panic!("sem welcome")
    };
    assert_eq!(you, back.id, "voltou com o mesmo id");
    assert_eq!(members.len(), 1);

    let seen = next(&mut beto_rx).await;
    assert!(
        matches!(&seen, RoomEvent::MemberUpdated(m) if m.id == back.id),
        "o Beto só vê uma atualização da Ana, não a saída: {seen:?}"
    );
}
