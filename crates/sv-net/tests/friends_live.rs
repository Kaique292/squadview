//! Amigos e presença contra o Worker de sinalização de verdade (ADR 0011), com os clientes do app:
//! sessão, recentes, pedido de amizade, presença, pedido para entrar na sala, convite.
//!
//! Roda pelo `yarn test:rs` (o `scripts/with-worker.mjs` sobe o Worker local e define
//! `SQUADVIEW_SIGNALING_URL` e `SQUADVIEW_TEST_SESSION_SECRET`). Sem essas variáveis, é pulado.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use sv_net::friends::{FriendAction, FriendsClient, FriendsConnectError, FriendsEvent, RoomStatus};
use sv_net::signaling::new_room_code;
use sv_net::{DiscordUser, RoomClient, RoomEvent};
use tokio::sync::mpsc::UnboundedReceiver;

/// Worker local e o segredo das sessões dele. `None`: teste pulado.
fn worker() -> Option<(String, String)> {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").ok();
    let secret = std::env::var("SQUADVIEW_TEST_SESSION_SECRET").ok();
    if url.is_none() || secret.is_none() {
        eprintln!("pulado: rode pelo `yarn test:rs` (sobe o Worker local)");
    }
    Some((url?, secret?))
}

/// Sessão como a do Worker: `{id, exp}` em base64url, ponto, HMAC-SHA256.
fn session(secret: &str, id: &str) -> String {
    let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 3_600_000;
    let payload = URL_SAFE_NO_PAD.encode(json!({ "id": id, "exp": exp as u64 }).to_string());
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let sig = ring::hmac::sign(&key, payload.as_bytes());
    format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

/// Ids do Discord novos a cada teste. O relógio do Windows anda de 100 em 100 ns: dois testes em
/// paralelo podiam pegar o mesmo horário, ficar com a mesma Ana e o mesmo Beto e um atrapalhar o
/// outro (na esteira os dois falhavam juntos esperando a amizade). O contador separa os testes do
/// mesmo processo; o horário separa as execuções (o Worker local guarda o estado entre elas).
fn fresh_ids() -> (String, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed) % 1000;
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() % 100_000_000_000_000;
    (format!("1{t:014}{n:03}"), format!("2{t:014}{n:03}"))
}

/// Espera uma mensagem do registro que satisfaça `pred`.
async fn wait(rx: &mut UnboundedReceiver<FriendsEvent>, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("não chegou: {what}"))
            .expect("conexão fechou");
        if let FriendsEvent::Message(v) = event
            && pred(&v)
        {
            return v;
        }
    }
}

async fn room_event(rx: &mut UnboundedReceiver<RoomEvent>, what: &str, pred: impl Fn(&RoomEvent) -> bool) -> RoomEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("não chegou na sala: {what}"))
            .expect("sala fechou");
        if pred(&event) {
            return event;
        }
    }
}

fn friends_of(v: &Value) -> Vec<&Value> {
    v["friends"].as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

#[tokio::test]
async fn sessao_falsa_e_recusada() {
    let Some((url, _)) = worker() else { return };
    let result = FriendsClient::connect(&url, "abc.def").await;
    assert!(
        matches!(result, Err(FriendsConnectError::Unauthorized)),
        "{:?}",
        result.err()
    );
}

#[tokio::test]
async fn amigos_de_ponta_a_ponta() {
    let Some((url, secret)) = worker() else { return };
    let (ana_id, beto_id) = fresh_ids();
    let (ana_session, beto_session) = (session(&secret, &ana_id), session(&secret, &beto_id));

    // Os dois conectam ao próprio registro: chegam o estado e a sessão renovada.
    let (ana, mut ana_rx) = FriendsClient::connect(&url, &ana_session).await.expect("Ana conecta");
    let (beto, mut beto_rx) = FriendsClient::connect(&url, &beto_session).await.expect("Beto conecta");
    wait(&mut ana_rx, "estado da Ana", |v| v["type"] == "state").await;
    ana.send(&FriendAction::Profile {
        name: "Ana".into(),
        avatar: None,
    });
    beto.send(&FriendAction::Profile {
        name: "Beto".into(),
        avatar: None,
    });

    // Entram na mesma sala conferidos pela sessão: viram recentes um do outro.
    let code = new_room_code();
    let ana_discord = DiscordUser {
        id: ana_id.clone(),
        avatar: None,
    };
    let (ana_room, mut ana_room_rx) = RoomClient::join(
        &url,
        &code,
        "Ana",
        Some(&ana_discord),
        Some(&ana_session),
        None,
        None,
        true,
    )
    .await
    .unwrap();
    room_event(&mut ana_room_rx, "welcome da Ana", |e| {
        matches!(e, RoomEvent::Joined { .. })
    })
    .await;
    let (_beto_room, mut beto_room_rx) =
        RoomClient::join(&url, &code, "Beto", None, Some(&beto_session), None, None, true)
            .await
            .unwrap();
    let RoomEvent::Joined { members, .. } = room_event(&mut beto_room_rx, "welcome do Beto", |e| {
        matches!(e, RoomEvent::Joined { .. })
    })
    .await
    else {
        unreachable!()
    };
    // A conta do Discord da Ana na sala é a da sessão (conferida).
    assert_eq!(
        members[0].discord.as_ref().map(|d| d.id.as_str()),
        Some(ana_id.as_str())
    );
    wait(&mut ana_rx, "Beto nos recentes", |v| {
        v["type"] == "state"
            && v["recents"]
                .as_array()
                .is_some_and(|r| r.iter().any(|p| p["id"] == beto_id.as_str()))
    })
    .await;

    // Pedido de amizade e aceite.
    ana.send(&FriendAction::FriendRequest { id: beto_id.clone() });
    wait(&mut beto_rx, "pedido de amizade", |v| v["type"] == "friend-request").await;
    beto.send(&FriendAction::FriendAccept { id: ana_id.clone() });
    wait(&mut ana_rx, "amizade", |v| {
        v["type"] == "state" && friends_of(v).iter().any(|f| f["id"] == beto_id.as_str())
    })
    .await;

    // Presença: o Beto vê a sala da Ana pelo nome, nunca pelo código.
    ana.send(&FriendAction::Status {
        room: Some(RoomStatus {
            code: code.clone(),
            name: Some("Galera".into()),
        }),
        live: true,
    });
    let state = wait(&mut beto_rx, "presença da Ana", |v| {
        v["type"] == "state" && friends_of(v).iter().any(|f| f["presence"]["room"] == "Galera")
    })
    .await;
    assert_eq!(friends_of(&state)[0]["presence"]["live"], true);
    assert!(!state.to_string().contains(&code), "o código da sala vazou na presença");

    // Pedido para entrar: aparece na sala, a Ana aceita, o Beto recebe o código.
    beto.send(&FriendAction::Join { id: ana_id.clone() });
    wait(&mut beto_rx, "pedido pendente", |v| {
        v["type"] == "join-result" && v["result"]["kind"] == "pending"
    })
    .await;
    let RoomEvent::JoinRequest { id, from } = room_event(&mut ana_room_rx, "pedido na sala", |e| {
        matches!(e, RoomEvent::JoinRequest { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(from.id, beto_id);
    ana_room.answer_join(&id, true);
    let approved = wait(&mut beto_rx, "aceito", |v| {
        v["type"] == "join-result" && v["result"]["kind"] == "approved"
    })
    .await;
    assert_eq!(approved["result"]["code"], code.as_str());
    room_event(&mut ana_room_rx, "pedido fechado", |e| {
        matches!(e, RoomEvent::JoinRequestClosed(_))
    })
    .await;

    // Entra direto: com a marcação, o código chega sem pedir.
    ana.send(&FriendAction::SetDirect {
        id: beto_id.clone(),
        direct: true,
    });
    wait(&mut ana_rx, "entra direto marcado", |v| {
        v["type"] == "state" && friends_of(v).iter().any(|f| f["direct"] == true)
    })
    .await;
    beto.send(&FriendAction::Join { id: ana_id.clone() });
    wait(&mut beto_rx, "entrou direto", |v| {
        v["type"] == "join-result" && v["result"]["kind"] == "approved" && v["result"]["by"] == "Ana"
    })
    .await;

    // Convite.
    ana.send(&FriendAction::Invite { id: beto_id.clone() });
    let invited = wait(&mut beto_rx, "convite", |v| v["type"] == "invited").await;
    assert_eq!(invited["code"], code.as_str());

    // Recusar pedido de entrada: sem a marcação, a Ana recusa.
    ana.send(&FriendAction::SetDirect {
        id: beto_id.clone(),
        direct: false,
    });
    wait(&mut ana_rx, "entra direto desmarcado", |v| {
        v["type"] == "state" && friends_of(v).iter().all(|f| f["direct"] == false)
    })
    .await;
    beto.send(&FriendAction::Join { id: ana_id.clone() });
    let RoomEvent::JoinRequest { id, .. } = room_event(&mut ana_room_rx, "segundo pedido", |e| {
        matches!(e, RoomEvent::JoinRequest { .. })
    })
    .await
    else {
        unreachable!()
    };
    ana_room.answer_join(&id, false);
    wait(&mut beto_rx, "recusado", |v| {
        v["type"] == "join-result" && v["result"]["kind"] == "denied"
    })
    .await;

    // Ana fecha o app: aparece offline. Depois, desfazem a amizade.
    drop(ana);
    wait(&mut beto_rx, "Ana offline", |v| {
        v["type"] == "state" && friends_of(v).iter().any(|f| f["presence"]["online"] == false)
    })
    .await;
    beto.send(&FriendAction::Unfriend { id: ana_id.clone() });
    wait(&mut beto_rx, "amizade desfeita", |v| {
        v["type"] == "state" && friends_of(v).is_empty()
    })
    .await;
}

#[tokio::test]
async fn pedidos_cancelados_e_recusados() {
    let Some((url, secret)) = worker() else { return };
    let (ana_id, beto_id) = fresh_ids();
    let (ana, mut ana_rx) = FriendsClient::connect(&url, &session(&secret, &ana_id)).await.unwrap();
    let (beto, mut beto_rx) = FriendsClient::connect(&url, &session(&secret, &beto_id)).await.unwrap();

    // Sem estar nos recentes, não dá para pedir.
    ana.send(&FriendAction::FriendRequest { id: beto_id.clone() });
    wait(&mut ana_rx, "erro de pessoa desconhecida", |v| {
        v["type"] == "error" && v["code"] == "unknown-person"
    })
    .await;

    // Na mesma sala, conferidos.
    let code = new_room_code();
    let (_a, mut a_rx) = RoomClient::join(
        &url,
        &code,
        "Ana",
        None,
        Some(&session(&secret, &ana_id)),
        None,
        None,
        true,
    )
    .await
    .unwrap();
    room_event(&mut a_rx, "welcome", |e| matches!(e, RoomEvent::Joined { .. })).await;
    let (_b, _b_rx) = RoomClient::join(
        &url,
        &code,
        "Beto",
        None,
        Some(&session(&secret, &beto_id)),
        None,
        None,
        true,
    )
    .await
    .unwrap();
    wait(&mut ana_rx, "recentes", |v| {
        v["type"] == "state" && v["recents"].as_array().is_some_and(|r| !r.is_empty())
    })
    .await;

    // Pede e cancela.
    ana.send(&FriendAction::FriendRequest { id: beto_id.clone() });
    wait(&mut beto_rx, "pedido", |v| v["type"] == "friend-request").await;
    ana.send(&FriendAction::FriendCancel { id: beto_id.clone() });
    wait(&mut beto_rx, "pedido cancelado", |v| {
        v["type"] == "state" && v["incoming"].as_array().is_some_and(|i| i.is_empty())
    })
    .await;

    // Pede de novo e o outro recusa.
    ana.send(&FriendAction::FriendRequest { id: beto_id.clone() });
    wait(&mut beto_rx, "pedido de novo", |v| {
        v["type"] == "state" && v["incoming"].as_array().is_some_and(|i| !i.is_empty())
    })
    .await;
    beto.send(&FriendAction::FriendDecline { id: ana_id.clone() });
    wait(&mut ana_rx, "pedido recusado", |v| {
        v["type"] == "state" && v["outgoing"].as_array().is_some_and(|o| o.is_empty())
    })
    .await;

    // Convidar quem não é amigo dá erro.
    ana.send(&FriendAction::Invite { id: beto_id.clone() });
    wait(&mut ana_rx, "convite recusado", |v| v["type"] == "error").await;
}

/// O caso do DK: entrou na sala com o login antigo (sem sessão) e depois entrou com o Discord de
/// novo, já na sala. Vira conferido para os outros e entra nos recentes, sem sair e entrar.
#[tokio::test]
async fn quem_entra_com_o_discord_ja_na_sala_vira_conferido() {
    let Some((url, secret)) = worker() else { return };
    let (ana_id, beto_id) = fresh_ids();
    let (ana_session, beto_session) = (session(&secret, &ana_id), session(&secret, &beto_id));
    let (_ana, mut ana_rx) = FriendsClient::connect(&url, &ana_session).await.unwrap();
    let code = new_room_code();
    let (_ana_room, mut ana_room_rx) = RoomClient::join(&url, &code, "Ana", None, Some(&ana_session), None, None, true)
        .await
        .unwrap();
    room_event(&mut ana_room_rx, "welcome", |e| matches!(e, RoomEvent::Joined { .. })).await;
    // Beto entra sem sessão: não conferido.
    let (beto_room, _beto_room_rx) = RoomClient::join(&url, &code, "Beto", None, None, None, None, true)
        .await
        .unwrap();
    let RoomEvent::MemberJoined(beto) = room_event(&mut ana_room_rx, "Beto entrou", |e| {
        matches!(e, RoomEvent::MemberJoined(_))
    })
    .await
    else {
        unreachable!()
    };
    assert!(!beto.verified);
    // Beto entra com o Discord, já na sala.
    beto_room.verify(&beto_session);
    let RoomEvent::MemberUpdated(beto) = room_event(
        &mut ana_room_rx,
        "Beto conferido",
        |e| matches!(e, RoomEvent::MemberUpdated(m) if m.verified),
    )
    .await
    else {
        unreachable!()
    };
    assert_eq!(beto.discord.map(|d| d.id), Some(beto_id.clone()));
    wait(&mut ana_rx, "Beto nos recentes", |v| {
        v["type"] == "state"
            && v["recents"]
                .as_array()
                .is_some_and(|r| r.iter().any(|p| p["id"] == beto_id.as_str()))
    })
    .await;
}
