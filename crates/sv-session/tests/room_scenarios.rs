//! Cenários da sala contra o Worker de sinalização local, sem GPU nem dispositivos de áudio:
//! renomear, apelido, mudo e surdo, quem sai, pedidos para assistir recusados ou sem resposta,
//! sinais que ninguém pediu, sala cheia e a volta automática depois de uma queda.
//!
//! Cada cenário põe um `Room` de verdade numa sala com "amigos" feitos direto com o cliente da
//! sinalização (`RoomClient`) e, quando precisa de vídeo, com um `Peer` QUIC no mesmo PC. Assim
//! dá para mandar exatamente o sinal que o cenário pede, de forma determinística.
//!
//! Roda pelo `yarn test:rs`, que sobe o Worker local (`scripts/with-worker.mjs`) e define
//! `SQUADVIEW_SIGNALING_URL`; sem ela, cada teste é pulado.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use sv_encode::EncodedFrame;
use sv_gpu::Size;
use sv_net::protocol::{FrameHeader, Signal, StreamDescription};
use sv_net::signaling::{error_message, new_room_code};
use sv_net::{DiscordUser, Peer, Purpose, Receiver, ReceiverSink, RoomClient, RoomEvent};
use sv_session::{AudioPacket, JoinAs, KeyframeHandle, Member, Room, RoomUpdate, StreamInfo};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;

/// Endereço do Worker local. `None`: teste pulado (rode pelo `yarn test:rs`).
fn worker() -> Option<String> {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").ok();
    if url.is_none() {
        eprintln!("pulado: rode pelo `yarn test:rs` (sobe o Worker local)");
    }
    url
}

const WAIT: Duration = Duration::from_secs(10);

/// Um membro de verdade (`Room`) e o que chega para a interface dele.
struct Someone {
    room: Room,
    updates: mpsc::Receiver<RoomUpdate>,
}

fn join(url: &str, code: &str, name: &str, room_name: Option<&str>) -> Someone {
    let (tx, updates) = mpsc::channel();
    let room = Room::join(url, code, JoinAs::named(name), room_name, None, move |u| {
        let _ = tx.send(u);
    })
    .unwrap_or_else(|e| panic!("{name} não entrou: {e}"));
    Someone { room, updates }
}

impl Someone {
    /// Espera a primeira atualização para a qual `pick` devolve algo.
    fn wait<T>(&self, what: &str, mut pick: impl FnMut(RoomUpdate) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.updates.recv_timeout(left) {
                Ok(update) => {
                    if let Some(found) = pick(update) {
                        return found;
                    }
                }
                Err(_) => panic!("não chegou: {what}"),
            }
        }
    }

    /// Espera uma lista de membros que satisfaça `pred` e a devolve.
    fn members(&self, what: &str, pred: impl Fn(&[Member]) -> bool) -> Vec<Member> {
        self.wait(what, |u| match u {
            RoomUpdate::Members(m) if pred(&m) => Some(m),
            _ => None,
        })
    }

    /// O id de alguém que já está na sala, pelo nome.
    fn id_of(&self, name: &str) -> String {
        let list = self.members(&format!("{name} na lista"), |m| m.iter().any(|x| x.name == name));
        list.into_iter().find(|x| x.name == name).unwrap().id
    }
}

/// Um amigo "de mentira": só o cliente da sinalização (e, se precisar, um ponto QUIC).
struct Friend {
    client: RoomClient,
    events: UnboundedReceiver<RoomEvent>,
    id: String,
    room: Option<String>,
}

fn friend(rt: &Runtime, url: &str, code: &str, name: &str) -> Friend {
    rt.block_on(async {
        let (client, mut events) = RoomClient::join(url, code, name, None, None, None)
            .await
            .unwrap_or_else(|e| panic!("{name} não entrou: {e}"));
        match tokio::time::timeout(WAIT, events.recv()).await {
            Ok(Some(RoomEvent::Joined { you, room, .. })) => Friend {
                client,
                events,
                id: you,
                room,
            },
            other => panic!("{name} não entrou: {other:?}"),
        }
    })
}

impl Friend {
    fn wait<T>(&mut self, rt: &Runtime, what: &str, mut pick: impl FnMut(RoomEvent) -> Option<T>) -> T {
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + WAIT;
            loop {
                match tokio::time::timeout_at(deadline, self.events.recv()).await {
                    Ok(Some(event)) => {
                        if let Some(found) = pick(event) {
                            return found;
                        }
                    }
                    other => panic!("não chegou: {what} ({other:?})"),
                }
            }
        })
    }

    /// Espera um sinal vindo de `from`.
    fn signal_from(&mut self, rt: &Runtime, from: &str) -> Signal {
        self.wait(rt, "sinal", |e| match e {
            RoomEvent::Signal { from: f, signal } if f == from => Some(signal),
            _ => None,
        })
    }
}

fn desc() -> StreamDescription {
    StreamDescription {
        width: 1280,
        height: 720,
        fps: 30,
        audio: true,
    }
}

#[test]
fn renomear_a_sala_avisa_todos_e_quem_chega_ja_recebe_o_nome() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", Some("Galera"));
    ana.wait("nome da sala", |u| match u {
        RoomUpdate::RoomName(Some(n)) if n == "Galera" => Some(()),
        _ => None,
    });
    assert_eq!(ana.room.room_name().as_deref(), Some("Galera"));
    assert_eq!(ana.room.code(), code);
    assert!(format!("{:?}", ana.room).contains(&code));
    assert!(ana.room.voice().is_none(), "entrou sem voz");
    assert_eq!(ana.room.viewer_count(), 0);
    let _ = ana.room.nat();

    let mut beto = friend(&rt, &url, &code, "Beto");
    assert_eq!(beto.room.as_deref(), Some("Galera"));

    ana.room.rename_room("Jogatina");
    ana.wait("o novo nome", |u| match u {
        RoomUpdate::RoomName(Some(n)) if n == "Jogatina" => Some(()),
        _ => None,
    });
    assert_eq!(ana.room.room_name().as_deref(), Some("Jogatina"));
    beto.wait(&rt, "o novo nome no Beto", |e| {
        matches!(e, RoomEvent::RoomRenamed(ref n) if n == "Jogatina").then_some(())
    });

    // Qualquer um renomeia: a Ana fica sabendo quando o Beto troca.
    beto.client.set_room_name("Sala do Beto");
    ana.wait("o nome dado pelo Beto", |u| match u {
        RoomUpdate::RoomName(Some(n)) if n == "Sala do Beto" => Some(()),
        _ => None,
    });
    assert_eq!(ana.room.room_name().as_deref(), Some("Sala do Beto"));
}

#[test]
fn apelido_perfil_mudo_e_surdo_chegam_aos_outros() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    ana.members("o Beto chegar", |m| m.iter().any(|x| x.name == "Beto"));

    let updated = |beto: &mut Friend, what: &str, pred: &dyn Fn(&Member) -> bool| {
        beto.wait(&rt, what, |e| match e {
            RoomEvent::MemberUpdated(m) if pred(&m) => Some(m),
            _ => None,
        })
    };

    ana.room.set_name("Aninha");
    let me = ana.members("a lista com o apelido novo", |m| m[0].name == "Aninha");
    assert_eq!(me[0].name, "Aninha", "nós primeiro na lista");
    updated(&mut beto, "o apelido novo", &|m| m.name == "Aninha");

    let discord = DiscordUser {
        id: "123456789012345678".into(),
        avatar: Some("0123456789abcdef0123456789abcdef".into()),
    };
    ana.room.set_profile("Ana", Some(discord.clone()));
    ana.members("o perfil novo", |m| {
        m[0].name == "Ana" && m[0].discord.as_ref() == Some(&discord)
    });
    updated(&mut beto, "a foto do Discord", &|m| {
        m.discord.as_ref() == Some(&discord)
    });

    ana.room.set_muted(true);
    ana.members("mudo na lista", |m| m[0].muted);
    updated(&mut beto, "a Ana muda", &|m| m.muted && !m.deafened);
    ana.room.set_deafened(true);
    ana.members("surda na lista", |m| m[0].deafened);
    updated(&mut beto, "a Ana surda", &|m| m.muted && m.deafened);

    // A Ana conta para a sala quem está assistindo (quem transmite vê quantos e quem assistem).
    ana.room.set_watching(Some("0123456789abcdef"));
    ana.members("assistindo na lista", |m| {
        m[0].watching.as_deref() == Some("0123456789abcdef")
    });
    updated(&mut beto, "quem a Ana assiste", &|m| {
        m.watching.as_deref() == Some("0123456789abcdef")
    });
    ana.room.set_watching(None);
    updated(&mut beto, "a Ana parar de assistir", &|m| m.watching.is_none());

    // O Beto fica ao vivo: a Ana vê.
    beto.client.set_status(true, false, false, None);
    ana.members("o Beto ao vivo", |m| m.iter().any(|x| x.name == "Beto" && x.live));

    // O Beto sai: some da lista e a voz é atualizada.
    drop(beto);
    ana.members("o Beto sair", |m| m.iter().all(|x| x.name != "Beto"));
    let voice = ana.wait("a voz atualizada", |u| match u {
        RoomUpdate::VoicePeers(p) => Some(p),
        _ => None,
    });
    assert!(voice.is_empty());
}

#[test]
fn pedir_para_assistir_quem_nao_esta_ao_vivo_e_recusado() {
    let Some(url) = worker() else { return };
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let beto = join(&url, &code, "Beto", None);
    let beto_id = ana.id_of("Beto");

    let refused = |ana: &Someone| {
        ana.wait("a recusa", |u| match u {
            RoomUpdate::WatchFailed { member_id, reason } => Some((member_id, reason)),
            RoomUpdate::WatchReady { .. } => panic!("não devia conectar"),
            _ => None,
        })
    };
    ana.room.watch(&beto_id);
    assert_eq!(
        refused(&ana),
        (beto_id.clone(), "Não está transmitindo agora.".to_owned())
    );

    // Ao vivo, mas o encoder ainda não começou (sem formato do vídeo): também recusa.
    beto.room.stream_started(KeyframeHandle::detached());
    ana.members("o Beto ao vivo", |m| m.iter().any(|x| x.id == beto_id && x.live));
    ana.room.watch(&beto_id);
    assert_eq!(refused(&ana).1, "Não está transmitindo agora.");
    beto.room.stream_stopped();
    ana.members("o Beto fora do ar", |m| m.iter().any(|x| x.id == beto_id && !x.live));
}

#[test]
fn sinais_que_ninguem_pediu_sao_ignorados() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    let candidates = vec!["127.0.0.1:9".parse().unwrap()];
    for signal in [
        Signal::WatchAccept {
            candidates: candidates.clone(),
            fingerprint: "ab".into(),
            stream: desc(),
        },
        Signal::WatchDeny { reason: "não".into() },
        // A Ana entrou sem voz: convite e resposta de voz não fazem nada.
        Signal::VoiceOffer {
            candidates: candidates.clone(),
            fingerprint: "ab".into(),
        },
        Signal::VoiceAnswer {
            candidates,
            fingerprint: "ab".into(),
        },
    ] {
        beto.client.signal(&ana_id, &signal);
    }
    // Um marcador depois dos sinais: até ele chegar, nada de assistir nem de voz.
    beto.client.set_status(true, false, false, None);
    ana.wait("o marcador", |u| match u {
        RoomUpdate::Members(m) if m.iter().any(|x| x.name == "Beto" && x.live) => Some(()),
        RoomUpdate::WatchReady { .. } | RoomUpdate::WatchFailed { .. } | RoomUpdate::VoiceFailed { .. } => {
            panic!("sinal não pedido teve efeito: {u:?}")
        }
        _ => None,
    });
    // Sem voz, a Ana não responde ao convite de voz: o que ela manda depois chega sem sinal antes.
    ana.room.set_name("Ana de novo");
    beto.wait(&rt, "o apelido novo da Ana", |e| match e {
        RoomEvent::Signal { signal, .. } => panic!("a Ana respondeu: {signal:?}"),
        RoomEvent::MemberUpdated(m) if m.name == "Ana de novo" => Some(()),
        _ => None,
    });
}

#[test]
fn assiste_um_amigo_que_aceita_pela_conexao_direta() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let beto_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    ana.room.watch(&beto.id);
    let Signal::WatchRequest { fingerprint, .. } = beto.signal_from(&rt, &ana_id) else {
        panic!("esperava o pedido para assistir")
    };
    beto_peer.allowed().allow(&fingerprint);
    beto.client.signal(
        &ana_id,
        &Signal::WatchAccept {
            candidates: beto_peer.candidates().to_vec(),
            fingerprint: beto_peer.fingerprint().to_owned(),
            stream: desc(),
        },
    );
    let accepted = rt.spawn(async move {
        let accepted = beto_peer.accept().await;
        (beto_peer, accepted)
    });
    let (member, got) = ana.wait("a conexão", |u| match u {
        RoomUpdate::WatchReady { member, desc, .. } => Some((member, desc)),
        RoomUpdate::WatchFailed { reason, .. } => panic!("falhou: {reason}"),
        _ => None,
    });
    assert_eq!(member.name, "Beto");
    assert_eq!(got, desc());
    let (_peer, accepted) = rt.block_on(accepted).unwrap();
    assert_eq!(accepted.expect("o Beto aceitou").purpose, Purpose::Stream);
}

#[test]
fn sem_conexao_direta_quem_pediu_para_assistir_fica_sabendo() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    ana.room.watch(&beto.id);
    beto.signal_from(&rt, &ana_id);
    // Nenhum endereço para tentar: a conexão direta falha na hora.
    beto.client.signal(
        &ana_id,
        &Signal::WatchAccept {
            candidates: Vec::new(),
            fingerprint: "ab".into(),
            stream: desc(),
        },
    );
    let (who, reason) = ana.wait("a falha", |u| match u {
        RoomUpdate::WatchFailed { member_id, reason } => Some((member_id, reason)),
        RoomUpdate::WatchReady { .. } => panic!("não devia conectar"),
        _ => None,
    });
    assert_eq!(who, beto.id);
    assert!(reason.contains("conectar direto"), "{reason}");
}

/// O que chegou para o amigo que assiste.
enum Got {
    Video(u32),
    Audio(u32),
}

struct Sink(mpsc::Sender<Got>);

impl ReceiverSink for Sink {
    fn video(&mut self, frame: FrameHeader, _data: Vec<u8>) {
        let _ = self.0.send(Got::Video(frame.seq));
    }
    fn audio(&mut self, seq: u32, _: u64, _: f32, _: &[u8]) {
        let _ = self.0.send(Got::Audio(seq));
    }
    fn closed(&mut self, _: String) {}
}

#[test]
fn amigo_conectado_recebe_video_e_som_e_cai_quando_a_transmissao_para() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let beto_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    let tap = ana.room.tap();
    tap.started(&StreamInfo {
        size: Size::new(1280, 720),
        fps: 30,
        encoder: "teste".into(),
    });
    ana.room.stream_started(KeyframeHandle::detached());
    // Ninguém assistindo: os frames nem saem.
    tap.video(
        99,
        &EncodedFrame {
            data: vec![1; 10],
            keyframe: true,
            timestamp_us: 0,
            encode_us: 1,
        },
    );

    beto.client.signal(
        &ana_id,
        &Signal::WatchRequest {
            candidates: beto_peer.candidates().to_vec(),
            fingerprint: beto_peer.fingerprint().to_owned(),
        },
    );
    let Signal::WatchAccept {
        candidates,
        fingerprint,
        stream,
    } = beto.signal_from(&rt, &ana_id)
    else {
        panic!("a Ana devia aceitar")
    };
    assert_eq!((stream.width, stream.height, stream.fps), (1280, 720, 30));
    let conn = rt
        .block_on(beto_peer.connect(&candidates, &fingerprint))
        .expect("conectar na Ana");
    let (tx, got) = mpsc::channel();
    let _receiver = {
        let _guard = rt.enter();
        Receiver::start(conn, Sink(tx))
    };
    for _ in 0..100 {
        if ana.room.viewer_count() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(ana.room.viewer_count(), 1);

    tap.video(
        0,
        &EncodedFrame {
            data: vec![1; 3000],
            keyframe: true,
            timestamp_us: 1,
            encode_us: 1,
        },
    );
    tap.audio(&AudioPacket {
        seq: 7,
        data: vec![0xF8, 0xFF, 0xFE],
        peak: 0.2,
        timestamp_us: 2,
    });
    let (mut video, mut audio) = (None, None);
    while video.is_none() || audio.is_none() {
        match got.recv_timeout(WAIT).expect("nada chegou ao Beto") {
            Got::Video(seq) => video = Some(seq),
            Got::Audio(seq) => audio = Some(seq),
        }
    }
    assert_eq!((video, audio), (Some(0), Some(7)));

    // Uma conexão de voz de quem não chamou para a voz é recusada.
    let voice = rt
        .block_on(beto_peer.connect_for(&candidates, &fingerprint, Purpose::Voice))
        .expect("o aperto de mão passa: a fingerprint do Beto foi autorizada");
    let reason = rt.block_on(voice.closed());
    assert!(reason.to_string().contains("desconhecido"), "{reason}");

    // A Ana para: quem assistia sai, e uma conexão nova é recusada.
    ana.room.stream_stopped();
    assert_eq!(ana.room.viewer_count(), 0);
    let late = rt
        .block_on(beto_peer.connect(&candidates, &fingerprint))
        .expect("o aperto de mão passa");
    let reason = rt.block_on(late.closed());
    assert!(reason.to_string().contains("nao esta transmitindo"), "{reason}");
}

#[test]
fn sala_cheia_nao_deixa_entrar() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let _full: Vec<Friend> = (0..8)
        .map(|i| friend(&rt, &url, &code, &format!("Amigo {i}")))
        .collect();
    let err = Room::join(&url, &code, JoinAs::named("Ana"), None, None, |_| {}).unwrap_err();
    assert_eq!(err, error_message("room-full"));
}

/// Quem transmite não responde (saiu, caiu ou tem um app antigo): depois do prazo de 12 s,
/// o pedido é desfeito e a interface fica sabendo.
#[test]
fn pedido_para_assistir_sem_resposta_desiste_sozinho() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    // Alguém que já saiu: a sala avisa na hora.
    ana.room.watch("00000000000000aa");
    let error = ana.wait("o erro da sala", |u| match u {
        RoomUpdate::Error(e) => Some(e),
        _ => None,
    });
    assert_eq!(error, error_message("unknown-peer"));

    // O Beto recebe o pedido e fica calado.
    ana.room.watch(&beto.id);
    beto.signal_from(&rt, &ana_id);
    let started = Instant::now();
    let (who, reason) = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match ana
                .updates
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(RoomUpdate::WatchFailed { member_id, reason }) => break (member_id, reason),
                Ok(_) => {}
                Err(_) => panic!("o pedido não venceu"),
            }
        }
    };
    assert_eq!(
        who, beto.id,
        "só o pedido mais recente vence; o do fantasma foi trocado"
    );
    assert!(reason.contains("não respondeu"), "{reason}");
    assert!(started.elapsed() >= Duration::from_secs(10));

    // Resposta atrasada, depois de desistir: ignorada.
    beto.client
        .signal(&ana_id, &Signal::WatchDeny { reason: "tarde".into() });
    beto.client.set_status(true, false, false, None);
    ana.wait("o marcador", |u| match u {
        RoomUpdate::Members(m) if m.iter().any(|x| x.name == "Beto" && x.live) => Some(()),
        RoomUpdate::WatchFailed { .. } => panic!("resposta atrasada não pode falhar de novo"),
        _ => None,
    });
}

/// A sinalização derruba quem manda mensagens demais (limite de taxa). A sala volta sozinha,
/// com o mesmo nome e o mesmo estado, e quem estava nela vê a pessoa sair e voltar.
#[test]
fn sinalizacao_cai_e_a_sala_volta_sozinha() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let old_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    // Mais de 100 mensagens em 10 s: o Worker fecha a conexão.
    for i in 0..110 {
        ana.room.set_muted(i % 2 == 0);
    }
    let error = ana.wait("o aviso do limite", |u| match u {
        RoomUpdate::Error(e) => Some(e),
        _ => None,
    });
    assert_eq!(error, error_message("rate-limited"));
    ana.wait("tentando voltar", |u| {
        matches!(u, RoomUpdate::Reconnecting).then_some(())
    });
    let mut seen: Option<Vec<Member>> = None;
    let mut renamed = false;
    ana.wait("de volta", |u| match u {
        RoomUpdate::Members(m) => {
            seen = Some(m);
            None
        }
        RoomUpdate::RoomName(_) => {
            renamed = true;
            None
        }
        RoomUpdate::Reconnected => Some(()),
        RoomUpdate::Disconnected => panic!("desistiu de voltar"),
        _ => None,
    });
    assert!(renamed, "o nome da sala é publicado de novo ao voltar");
    let members = seen.expect("a lista é publicada de novo ao voltar");
    assert_ne!(members[0].id, old_id, "voltou com outro id");
    assert!(members.iter().any(|m| m.name == "Beto"), "o Beto continua na lista");

    beto.wait(&rt, "a Ana sair", |e| {
        matches!(e, RoomEvent::MemberLeft(ref id) if *id == old_id).then_some(())
    });
    let back = beto.wait(&rt, "a Ana voltar", |e| match e {
        RoomEvent::MemberJoined(m) if m.name == "Ana" => Some(m),
        _ => None,
    });

    assert_eq!(back.id, members[0].id);

    // A sala segue funcionando pela conexão nova.
    ana.room.rename_room("De volta");
    beto.wait(&rt, "o nome dado depois da volta", |e| {
        matches!(e, RoomEvent::RoomRenamed(ref n) if n == "De volta").then_some(())
    });
}

/// Caiu e, na volta, a sala encheu: não adianta insistir, a interface fica sabendo.
#[test]
fn sala_cheia_na_volta_desiste_e_avisa() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut friends: Vec<Friend> = (0..7)
        .map(|i| friend(&rt, &url, &code, &format!("Amigo {i}")))
        .collect();
    ana.members("a sala com 8", |m| m.len() == 8);

    for i in 0..110 {
        ana.room.set_deafened(i % 2 == 0);
    }
    ana.wait("o aviso do limite", |u| matches!(u, RoomUpdate::Error(_)).then_some(()));
    // A vaga da Ana é ocupada antes de ela tentar voltar (a primeira tentativa é depois de 1 s).
    friends.push(friend(&rt, &url, &code, "Caio"));
    ana.wait("a desistência", |u| match u {
        RoomUpdate::Disconnected => Some(()),
        RoomUpdate::Reconnected => panic!("voltou para uma sala cheia"),
        _ => None,
    });
}
