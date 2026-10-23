//! Cenários da sala contra o Worker de sinalização local, sem GPU nem dispositivos de áudio:
//! renomear, apelido, mudo e surdo, quem sai, pedidos para assistir recusados ou sem resposta,
//! sinais que ninguém pediu, sala sem limite de pessoas e a volta automática depois de uma queda.
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
use sv_net::protocol::{
    FrameHeader, RelayKey, Relayed, Signal, StreamDescription, decode_relayed, encode_audio, encode_relayed, relay_peer,
};
use sv_net::signaling::{error_message, new_room_code};
use sv_net::{Broadcaster, DiscordUser, Peer, Purpose, Receiver, ReceiverSink, RoomClient, RoomEvent, VideoFrame};
use sv_session::{
    AudioPacket, JoinAs, KeyframeHandle, Member, RelaySource, Room, RoomUpdate, StreamInfo, VoiceSettings, WatchPath,
};
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
        let (client, mut events) = RoomClient::join(url, code, name, None, None, None, None, true)
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
    ana.room.set_watching(&["0123456789abcdef".to_owned()]);
    ana.members("assistindo na lista", |m| {
        m[0].watching.as_deref() == Some("0123456789abcdef")
    });
    updated(&mut beto, "quem a Ana assiste", &|m| {
        m.watching.as_deref() == Some("0123456789abcdef")
    });
    // Duas lives ao mesmo tempo (ADR 0019): a primeira continua no campo antigo.
    let duas = ["0123456789abcdef".to_owned(), "fedcba9876543210".to_owned()];
    ana.room.set_watching(&duas);
    updated(&mut beto, "as duas lives da Ana", &|m| {
        m.watching_all == duas && m.watching.as_deref() == Some("0123456789abcdef")
    });
    ana.room.set_watching(&[]);
    updated(&mut beto, "a Ana parar de assistir", &|m| {
        m.watching.is_none() && m.watching_all.is_empty()
    });

    // O Beto fica ao vivo: a Ana vê.
    beto.client.set_status(true, false, false, &[], None, None);
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

    // Ao vivo, mas o encoder nunca mandou o primeiro quadro: espera um pouco e recusa dizendo que
    // a transmissão ainda está começando (não que ela não existe).
    beto.room.stream_started(KeyframeHandle::detached());
    ana.members("o Beto ao vivo", |m| m.iter().any(|x| x.id == beto_id && x.live));
    ana.room.watch(&beto_id);
    assert_eq!(
        refused(&ana).1,
        "A transmissão ainda está começando. Tente de novo em instantes."
    );
    beto.room.stream_stopped();
    ana.members("o Beto fora do ar", |m| m.iter().any(|x| x.id == beto_id && !x.live));
}

/// Regressão: o DK aparecia "AO VIVO" e quem clicava para assistir logo depois recebia "Não está
/// transmitindo agora.", porque o encoder ainda não tinha mandado o primeiro quadro (uma janela
/// parada demora a mandar um). Agora o pedido espera esse quadro e entra quando ele sai.
#[test]
fn pedido_antes_do_primeiro_quadro_espera_e_entra_quando_ele_sai() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let beto_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();

    ana.room.stream_started(KeyframeHandle::detached());
    beto.client.signal(
        &ana_id,
        &Signal::WatchRequest {
            candidates: beto_peer.candidates().to_vec(),
            fingerprint: beto_peer.fingerprint().to_owned(),
        },
    );
    // Sem o primeiro quadro, a Ana não responde nada (nem recusa).
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, beto.events.recv()).await {
            if let RoomEvent::Signal { from, signal } = event
                && from == ana_id
            {
                panic!("a Ana respondeu antes do primeiro quadro: {signal:?}");
            }
        }
    });

    ana.room.tap().started(&StreamInfo {
        size: Size::new(1280, 720),
        fps: 30,
        encoder: "teste".into(),
    });
    let Signal::WatchAccept { stream, .. } = beto.signal_from(&rt, &ana_id) else {
        panic!("a Ana devia aceitar quando o primeiro quadro saísse")
    };
    assert_eq!((stream.width, stream.height), (1280, 720));
    ana.room.stream_stopped();
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
            relay: None,
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
            relay: None,
        },
    ] {
        beto.client.signal(&ana_id, &signal);
    }
    // Um marcador depois dos sinais: até ele chegar, nada de assistir nem de voz.
    beto.client.set_status(true, false, false, &[], None, None);
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
            relay: None,
        },
    );
    let accepted = rt.spawn(async move {
        let accepted = beto_peer.accept().await;
        (beto_peer, accepted)
    });
    let (member, got, _conn) = ana.wait("a conexão", |u| match u {
        // A conexão fica viva até o fim: fechar na hora faz quem aceita descartá-la (tentativa perdedora).
        RoomUpdate::WatchReady { member, desc, conn, .. } => Some((member, desc, conn)),
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
            relay: None,
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

/// Prepara a sala da camada do meio (ADR 0014): a Ana quer assistir o Beto mas não alcança ele;
/// o Caio já assiste o Beto. Devolve o pedido de repasse que a Ana mandou para o Caio.
fn ana_pede_repasse_ao_caio(rt: &Runtime, url: &str) -> (Someone, Friend, Friend, String, Signal) {
    let code = new_room_code();
    let ana = join(url, &code, "Ana", None);
    let mut beto = friend(rt, url, &code, "Beto");
    let mut caio = friend(rt, url, &code, "Caio");
    let ana_id = ana.id_of("Ana");
    // O Caio conta para a sala que assiste o Beto.
    caio.client
        .set_status(false, false, false, std::slice::from_ref(&beto.id), None, None);
    let beto_id = beto.id.clone();
    ana.members("o Caio assistindo o Beto", |m| {
        m.iter()
            .any(|x| x.name == "Caio" && x.watching.as_deref() == Some(beto_id.as_str()))
    });

    ana.room.watch(&beto.id);
    beto.signal_from(rt, &ana_id);
    // O Beto aceita, mas nenhum endereço dele serve: o direto falha.
    beto.client.signal(
        &ana_id,
        &Signal::WatchAccept {
            candidates: Vec::new(),
            fingerprint: "ab".into(),
            stream: desc(),
            relay: None,
        },
    );
    let request = caio.signal_from(rt, &ana_id);
    (ana, beto, caio, ana_id, request)
}

#[test]
fn sem_caminho_ate_quem_transmite_recebe_por_quem_ja_assiste() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let (ana, beto, caio, ana_id, request) = ana_pede_repasse_ao_caio(&rt, &url);
    let Signal::RelayRequest {
        streamer, fingerprint, ..
    } = request
    else {
        panic!("esperava o pedido de repasse: {request:?}")
    };
    assert_eq!(streamer, beto.id);

    // O Caio aceita repassar: responde como quem transmite, com os endereços dele.
    let caio_peer = rt.block_on(Peer::bind(&[])).unwrap();
    caio_peer.allowed().allow(&fingerprint);
    caio.client.signal(
        &ana_id,
        &Signal::WatchAccept {
            candidates: caio_peer.candidates().to_vec(),
            fingerprint: caio_peer.fingerprint().to_owned(),
            stream: desc(),
            relay: None,
        },
    );
    let accepted = rt.spawn(async move {
        let accepted = caio_peer.accept().await;
        (caio_peer, accepted)
    });
    let (member, path, _conn) = ana.wait("a conexão pelo Caio", |u| match u {
        RoomUpdate::WatchReady { member, path, conn, .. } => Some((member, path, conn)),
        RoomUpdate::WatchFailed { reason, .. } => panic!("falhou: {reason}"),
        _ => None,
    });
    // Para quem assiste, é o stream do Beto, que chega pelo Caio.
    assert_eq!(member.name, "Beto");
    assert_eq!(path, WatchPath::Member(caio.id.clone()));
    let (_peer, accepted) = rt.block_on(accepted).unwrap();
    assert_eq!(accepted.expect("o Caio aceitou").purpose, Purpose::Stream);
}

#[test]
fn quem_ja_assiste_recusa_repassar_e_a_falha_chega_com_o_motivo_do_direto() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let (ana, beto, caio, ana_id, request) = ana_pede_repasse_ao_caio(&rt, &url);
    assert!(matches!(request, Signal::RelayRequest { .. }), "{request:?}");
    caio.client.signal(
        &ana_id,
        &Signal::WatchDeny {
            reason: "Não dá para repassar agora.".into(),
        },
    );
    let (who, reason) = ana.wait("a falha", |u| match u {
        RoomUpdate::WatchFailed { member_id, reason } => Some((member_id, reason)),
        RoomUpdate::WatchReady { .. } => panic!("não devia conectar"),
        _ => None,
    });
    // Não sobrou ninguém: a falha é sobre o Beto, com o motivo da conexão direta.
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
        ..
    } = beto.signal_from(&rt, &ana_id)
    else {
        panic!("a Ana devia aceitar")
    };
    assert_eq!((stream.width, stream.height, stream.fps), (1280, 720, 30));
    let conn = rt
        .block_on(beto_peer.connect(&candidates, None, &fingerprint))
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
        .block_on(beto_peer.connect_for(&candidates, None, &fingerprint, Purpose::Voice))
        .expect("o aperto de mão passa: a fingerprint do Beto foi autorizada");
    let reason = rt.block_on(voice.closed());
    assert!(reason.to_string().contains("desconhecido"), "{reason}");

    // A Ana para: quem assistia sai, e uma conexão nova é recusada.
    ana.room.stream_stopped();
    assert_eq!(ana.room.viewer_count(), 0);
    let late = rt
        .block_on(beto_peer.connect(&candidates, None, &fingerprint))
        .expect("o aperto de mão passa");
    let reason = rt.block_on(late.closed());
    assert!(reason.to_string().contains("nao esta transmitindo"), "{reason}");
}

#[test]
fn sala_sem_limite_de_pessoas() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let _crowd: Vec<Friend> = (0..12)
        .map(|i| friend(&rt, &url, &code, &format!("Amigo {i}")))
        .collect();
    let ana = join(&url, &code, "Ana", None);
    ana.members("os 13 na sala", |m| m.len() == 13);
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
                // Cada pedido vence no seu prazo (várias lives, ADR 0019): o de quem já saiu também.
                Ok(RoomUpdate::WatchFailed { member_id, reason }) if member_id == beto.id => {
                    break (member_id, reason);
                }
                Ok(_) => {}
                Err(_) => panic!("o pedido não venceu"),
            }
        }
    };
    assert_eq!(who, beto.id);
    assert!(reason.contains("não respondeu"), "{reason}");
    assert!(started.elapsed() >= Duration::from_secs(10));

    // Resposta atrasada, depois de desistir: ignorada.
    beto.client
        .signal(&ana_id, &Signal::WatchDeny { reason: "tarde".into() });
    beto.client.set_status(true, false, false, &[], None, None);
    ana.wait("o marcador", |u| match u {
        RoomUpdate::Members(m) if m.iter().any(|x| x.name == "Beto" && x.live) => Some(()),
        RoomUpdate::WatchFailed { member_id, .. } if member_id == beto.id => {
            panic!("resposta atrasada não pode falhar de novo")
        }
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
        matches!(u, RoomUpdate::Reconnecting { .. }).then_some(())
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
        // Quem é derrubado por erro perde a vaga: volta como membro novo (ADR 0022).
        RoomUpdate::Reconnected { resumed } => {
            assert!(!resumed, "derrubado por erro não guarda a vaga");
            Some(())
        }
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

/// Quem já assiste e recebe um pedido de repasse (ADR 0014): aceita, recebe a conexão de quem pediu
/// e manda para ela os frames do stream que assiste (não o próprio). Pedido de outro stream: recusa.
#[test]
fn quem_ja_assiste_repassa_o_stream_a_quem_pediu() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let beto = friend(&rt, &url, &code, "Beto");
    let mut dani = friend(&rt, &url, &code, "Dani");
    let ana_id = ana.id_of("Ana");

    // A Ana assiste o Beto: o que chega dele vai para este Broadcaster de repasse. Os pedidos de
    // keyframe sobem por uma conexão de verdade (aqui, entre dois pontos de teste).
    let (up, down) = rt.block_on(async { (Peer::bind(&[]).await.unwrap(), Peer::bind(&[]).await.unwrap()) });
    up.allowed().allow(down.fingerprint());
    let keyframes = rt.block_on(async {
        let (_, conn) = tokio::join!(up.accept(), down.connect(up.candidates(), None, up.fingerprint()));
        Receiver::start(conn.unwrap(), Sink(mpsc::channel().0)).keyframe_requester()
    });
    let relay = Broadcaster::new();
    ana.room.add_relay_source(RelaySource {
        streamer: beto.id.clone(),
        desc: desc(),
        broadcaster: relay.clone(),
        keyframes,
    });

    // Pedido de repasse de outro stream: recusa.
    let dani_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let ask = |streamer: &str| Signal::RelayRequest {
        streamer: streamer.to_owned(),
        candidates: dani_peer.candidates().to_vec(),
        fingerprint: dani_peer.fingerprint().to_owned(),
    };
    dani.client.signal(&ana_id, &ask("0123456789abcdef"));
    assert!(matches!(dani.signal_from(&rt, &ana_id), Signal::WatchDeny { .. }));

    // Do Beto: aceita, com o formato do stream dele.
    dani.client.signal(&ana_id, &ask(&beto.id));
    let Signal::WatchAccept {
        candidates,
        fingerprint,
        stream,
        ..
    } = dani.signal_from(&rt, &ana_id)
    else {
        panic!("a Ana devia aceitar repassar")
    };
    assert_eq!(stream, desc());
    let conn = rt
        .block_on(dani_peer.connect(&candidates, None, &fingerprint))
        .expect("conectar na Ana");
    let (tx, got) = mpsc::channel();
    let _receiver = {
        let _guard = rt.enter();
        Receiver::start(conn, Sink(tx))
    };
    for _ in 0..100 {
        if relay.viewer_count() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(relay.viewer_count(), 1, "a Dani recebe pelo repasse");
    assert_eq!(ana.room.viewer_count(), 0, "não é espectadora da transmissão da Ana");

    // O que chega do Beto na Ana segue para a Dani.
    relay.send_video(&VideoFrame {
        header: FrameHeader {
            seq: 5,
            timestamp_us: 1,
            keyframe: true,
            encode_us: 1,
        },
        data: std::sync::Arc::from(vec![7u8; 2000]),
    });
    let first = got.recv_timeout(WAIT).expect("o frame repassado não chegou");
    assert!(matches!(first, Got::Video(5)));
}

// --- Voz repassada por um membro da sala (ADR 0020) ---

/// A Ana entra com voz (o motor liga mesmo sem microfone nem saída: a falha vira só um aviso).
/// Entra de novo até pegar um id baixo: quem tem o id menor chama na voz, e os cenários precisam
/// que seja a Ana a chamar o Beto.
fn join_with_voice(url: &str, code: &str) -> (Someone, String) {
    for _ in 0..64 {
        let (tx, updates) = mpsc::channel();
        let room = Room::join(
            url,
            code,
            JoinAs::named("Ana"),
            None,
            Some(VoiceSettings::default()),
            move |u| {
                let _ = tx.send(u);
            },
        )
        .expect("a Ana entra");
        assert!(room.voice().is_some(), "a voz da Ana não ligou");
        let ana = Someone { room, updates };
        let id = ana.members("a Ana", |m| !m.is_empty())[0].id.clone();
        if id.as_str() < "4" {
            return (ana, id);
        }
    }
    panic!("a Ana não pegou um id baixo");
}

/// Amigos até ter um com id maior que o da Ana (é a Ana que chama esse para a voz) e outro.
fn two_friends(rt: &Runtime, url: &str, code: &str, ana_id: &str) -> (Friend, Friend) {
    let (mut higher, mut other) = (None, None);
    for i in 0..7 {
        let f = friend(rt, url, code, &format!("Amigo {i}"));
        if higher.is_none() && f.id.as_str() > ana_id {
            higher = Some(f);
        } else if other.is_none() {
            other = Some(f);
        }
        if higher.is_some() && other.is_some() {
            break;
        }
    }
    (higher.expect("um amigo com id maior"), other.expect("outro amigo"))
}

/// Liga a voz entre a Ana e `friend` (quem tem o id menor chama, como no app). Devolve a conexão
/// do lado do amigo e a fingerprint da Ana.
fn voice_with_ana(rt: &Runtime, ana_id: &str, friend: &mut Friend, peer: &Peer) -> (quinn::Connection, String) {
    if ana_id < friend.id.as_str() {
        let Signal::VoiceOffer { fingerprint, .. } = friend.signal_from(rt, ana_id) else {
            panic!("a Ana devia chamar para a voz")
        };
        peer.allowed().allow(&fingerprint);
        friend.client.signal(
            ana_id,
            &Signal::VoiceAnswer {
                candidates: peer.candidates().to_vec(),
                fingerprint: peer.fingerprint().to_owned(),
                relay: None,
            },
        );
        let accepted = rt
            .block_on(async { tokio::time::timeout(WAIT, peer.accept()).await })
            .expect("a Ana conectou")
            .expect("conexão");
        assert_eq!(accepted.purpose, Purpose::Voice);
        (accepted.conn, fingerprint)
    } else {
        friend.client.signal(
            ana_id,
            &Signal::VoiceOffer {
                candidates: peer.candidates().to_vec(),
                fingerprint: peer.fingerprint().to_owned(),
            },
        );
        let Signal::VoiceAnswer {
            candidates,
            fingerprint,
            ..
        } = friend.signal_from(rt, ana_id)
        else {
            panic!("a Ana devia atender a voz")
        };
        // Como no app: quem chama autoriza quem atendeu (a conexão de repasse vem depois).
        peer.allowed().allow(&fingerprint);
        let conn = rt
            .block_on(peer.connect_for(&candidates, None, &fingerprint, Purpose::Voice))
            .expect("conectar na Ana");
        (conn, fingerprint)
    }
}

/// Espera a Ana ver a voz ligada com todos de `ids`.
fn ana_talks_to(ana: &Someone, ids: &[&str]) {
    ana.wait("a voz da Ana ligada", |u| match u {
        RoomUpdate::VoicePeers(p) if ids.iter().all(|id| p.iter().any(|x| x == id)) => Some(()),
        _ => None,
    });
}

/// O Beto (id maior que o da Ana) não deixa a voz direta abrir: responde sem endereço nenhum.
fn beto_unreachable(rt: &Runtime, ana_id: &str, beto: &mut Friend) {
    let Signal::VoiceOffer { .. } = beto.signal_from(rt, ana_id) else {
        panic!("a Ana devia chamar o Beto para a voz")
    };
    beto.client.signal(
        ana_id,
        &Signal::VoiceAnswer {
            candidates: Vec::new(),
            fingerprint: "ab".into(),
            relay: None,
        },
    );
}

#[test]
fn sem_voz_direta_a_ana_ouve_o_beto_pela_conexao_do_hugo() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let (ana, ana_id) = join_with_voice(&url, &code);
    let (mut beto, mut hugo) = two_friends(&rt, &url, &code, &ana_id);
    let hugo_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let (hugo_voice, ana_fp) = voice_with_ana(&rt, &ana_id, &mut hugo, &hugo_peer);
    ana_talks_to(&ana, &[&hugo.id]);

    // A voz direta com o Beto não abre: a Ana pede ao Hugo, que fala com os dois.
    beto_unreachable(&rt, &ana_id, &mut beto);
    let request = hugo.signal_from(&rt, &ana_id);
    assert_eq!(request, Signal::VoiceRelayRequest { to: beto.id.clone() });
    hugo.client
        .signal(&ana_id, &Signal::VoiceRelayAccept { to: beto.id.clone() });

    // A chave do par vai direto para o Beto (o Hugo não vê).
    let Signal::VoiceRelayKey { via, key } = beto.signal_from(&rt, &ana_id) else {
        panic!("a chave devia ir para o Beto")
    };
    assert_eq!(via, hugo.id);
    let key = RelayKey::from_hex(&key).expect("chave em hex");

    // O Hugo abre a conexão de repasse com a Ana, pelo mesmo caminho da voz.
    let relay = rt
        .block_on(hugo_peer.connect_for(&[hugo_voice.remote_address()], None, &ana_fp, Purpose::VoiceRelay))
        .expect("conexão de repasse");
    // O Beto aparece ligado na voz, pela conexão do Hugo.
    ana_talks_to(&ana, &[&beto.id]);
    let bridges = ana.wait("a ponte", |u| match u {
        RoomUpdate::VoiceBridges(b) if !b.is_empty() => Some(b),
        RoomUpdate::VoiceFailed { reason, .. } => panic!("não devia falhar: {reason}"),
        _ => None,
    });
    assert_eq!(bridges.get(&beto.id), Some(&hugo.id));

    // Um pacote do Beto, assinado com a chave do par, passa pelo Hugo sem problema.
    let (ana_b, beto_b) = (relay_peer(&ana_id).unwrap(), relay_peer(&beto.id).unwrap());
    let mut audio = Vec::new();
    encode_audio(1, 2, 0.5, &[1, 2, 3], &mut audio);
    let mut packet = Vec::new();
    encode_relayed(&beto_b, &key.tag(&beto_b, &ana_b, &audio), &audio, &mut packet);
    relay.send_datagram(packet.into()).expect("mandar");

    // O Hugo para de repassar: a Ana perde o Beto. Não pede ao Hugo de novo (ficaria caindo) e,
    // sem mais ninguém para repassar, a falha chega à interface.
    relay.close(quinn::VarInt::from_u32(0), b"tchau");
    ana.wait("o Beto fora da voz", |u| match u {
        RoomUpdate::VoicePeers(p) if !p.contains(&beto.id) => Some(()),
        _ => None,
    });
    let (who, reason) = ana.wait("a falha da voz", |u| match u {
        RoomUpdate::VoiceFailed { member_id, reason } => Some((member_id, reason)),
        _ => None,
    });
    assert_eq!(who, beto.id);
    assert!(reason.contains("ponte da voz caiu"), "{reason}");
}

#[test]
fn ninguem_pode_repassar_a_voz_e_a_falha_chega_sem_falar_de_repassador() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let (ana, ana_id) = join_with_voice(&url, &code);
    let (mut beto, mut hugo) = two_friends(&rt, &url, &code, &ana_id);
    let hugo_peer = rt.block_on(Peer::bind(&[])).unwrap();
    let _voice = voice_with_ana(&rt, &ana_id, &mut hugo, &hugo_peer);
    ana_talks_to(&ana, &[&hugo.id]);

    beto_unreachable(&rt, &ana_id, &mut beto);
    assert_eq!(
        hugo.signal_from(&rt, &ana_id),
        Signal::VoiceRelayRequest { to: beto.id.clone() }
    );
    // Recusa de quem não foi chamado não conta; a do Hugo, sim.
    beto.client.signal(
        &ana_id,
        &Signal::VoiceRelayDeny {
            to: beto.id.clone(),
            reason: "não".into(),
        },
    );
    hugo.client.signal(
        &ana_id,
        &Signal::VoiceRelayDeny {
            to: beto.id.clone(),
            reason: "já estou repassando a voz de muita gente".into(),
        },
    );
    let (who, reason) = ana.wait("a falha da voz", |u| match u {
        RoomUpdate::VoiceFailed { member_id, reason } => Some((member_id, reason)),
        _ => None,
    });
    assert_eq!(who, beto.id);
    assert!(reason.contains("conectar"), "{reason}");
    assert!(!reason.contains("repassador"), "{reason}");
}

#[test]
fn a_ana_repassa_a_voz_entre_o_hugo_e_o_beto_sem_mexer_na_assinatura() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let (ana, ana_id) = join_with_voice(&url, &code);
    let (mut beto, mut hugo) = two_friends(&rt, &url, &code, &ana_id);
    let (beto_peer, hugo_peer) = (
        rt.block_on(Peer::bind(&[])).unwrap(),
        rt.block_on(Peer::bind(&[])).unwrap(),
    );
    let _beto_voice = voice_with_ana(&rt, &ana_id, &mut beto, &beto_peer);
    let _hugo_voice = voice_with_ana(&rt, &ana_id, &mut hugo, &hugo_peer);
    ana_talks_to(&ana, &[&beto.id, &hugo.id]);

    // Pedido para alguém que não está na sala: recusa.
    hugo.client.signal(
        &ana_id,
        &Signal::VoiceRelayRequest {
            to: "0000000000000000".into(),
        },
    );
    let Signal::VoiceRelayDeny { .. } = hugo.signal_from(&rt, &ana_id) else {
        panic!("devia recusar")
    };

    // O Hugo não alcança o Beto e pede à Ana, que fala com os dois.
    hugo.client
        .signal(&ana_id, &Signal::VoiceRelayRequest { to: beto.id.clone() });
    assert_eq!(
        hugo.signal_from(&rt, &ana_id),
        Signal::VoiceRelayAccept { to: beto.id.clone() }
    );
    // A Ana abre a conexão de repasse com os dois.
    let accept = |peer: &Peer| {
        let accepted = rt
            .block_on(async { tokio::time::timeout(WAIT, peer.accept()).await })
            .expect("a Ana abriu o repasse")
            .expect("conexão");
        assert_eq!(accepted.purpose, Purpose::VoiceRelay);
        accepted.conn
    };
    let (to_hugo, to_beto) = (accept(&hugo_peer), accept(&beto_peer));

    // O Hugo manda a voz para o Beto; chega ao Beto com a origem trocada e a assinatura igual.
    let (hugo_b, beto_b) = (relay_peer(&hugo.id).unwrap(), relay_peer(&beto.id).unwrap());
    let mut packet = Vec::new();
    encode_relayed(&beto_b, &[7; 8], b"voz do hugo", &mut packet);
    let got = rt.block_on(async {
        // Datagrama pode se perder: manda de novo até chegar.
        for _ in 0..50 {
            to_hugo.send_datagram(packet.clone().into()).expect("mandar");
            if let Ok(Ok(d)) = tokio::time::timeout(Duration::from_millis(100), to_beto.read_datagram()).await {
                return d;
            }
        }
        panic!("a voz do Hugo não chegou ao Beto");
    });
    assert_eq!(
        decode_relayed(&got),
        Some(Relayed::Audio {
            peer: hugo_b,
            tag: [7; 8],
            audio: b"voz do hugo"
        })
    );

    // O Hugo sai: a Ana avisa o Beto que a voz dele parou de chegar.
    drop(hugo);
    let ended = rt.block_on(async {
        loop {
            let d = tokio::time::timeout(WAIT, to_beto.read_datagram())
                .await
                .expect("o aviso chegou")
                .expect("datagrama");
            if let Some(Relayed::Ended { peer }) = decode_relayed(&d) {
                return peer;
            }
        }
    });
    assert_eq!(ended, hugo_b);
}

/// Regressão: um código digitado que não é de sala nenhuma criava uma sala vazia (e o app entrava
/// nela). Agora a entrada falha com "Código inválido", e o código continua livre para quem criar.
#[test]
fn entrar_por_um_codigo_que_nao_existe_e_codigo_invalido() {
    let Some(url) = worker() else { return };
    let code = new_room_code();
    let only_join = JoinAs {
        create: false,
        ..JoinAs::named("Ana")
    };
    let err = Room::join(&url, &code, only_join.clone(), None, None, |_| {}).unwrap_err();
    assert_eq!(err, "Código inválido: essa sala não existe.");

    // Quem cria a sala entra; depois, quem entra pelo código encontra a sala.
    let beto = join(&url, &code, "Beto", None);
    let ana = Room::join(&url, &code, only_join, None, None, |_| {}).expect("a sala agora existe");
    beto.members("a Ana", |m| m.iter().any(|x| x.name == "Ana"));
    drop(ana);
}

/// Fliperama (ADR 0023): quem abre precisa estar ao vivo e ter o driver de controle; fechado, o
/// pedido de vaga é recusado com o motivo.
#[test]
fn fliperama_fechado_recusa_a_vaga_e_abrir_pede_live_e_driver() {
    let Some(url) = worker() else { return };
    let rt = Runtime::new().unwrap();
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let mut beto = friend(&rt, &url, &code, "Beto");
    let ana_id = ana.members("o Beto", |m| m.iter().any(|x| x.name == "Beto"))[0]
        .id
        .clone();
    let peer = rt.block_on(Peer::bind(&[])).unwrap();
    beto.client.signal(
        &ana_id,
        &Signal::ArcadeJoin {
            candidates: peer.candidates().to_vec(),
            fingerprint: peer.fingerprint().to_owned(),
        },
    );
    assert_eq!(
        beto.signal_from(&rt, &ana_id),
        Signal::ArcadeDeny {
            reason: "O fliperama está fechado.".into()
        }
    );

    let err = ana.room.arcade_open(Some("Bomberman".into()), 3, None).unwrap_err();
    assert!(err.contains("Comece a transmitir"), "{err}");
    ana.room.stream_started(KeyframeHandle::detached());
    match ana.room.arcade_open(Some("Bomberman".into()), 3, None) {
        // Na CI não há o ViGEmBus: a mensagem leva às Configurações.
        Err(e) => assert!(e.contains("ViGEmBus"), "{e}"),
        Ok(()) => {
            // Com o driver: o fliperama aparece para os outros no status.
            beto.wait(&rt, "o fliperama no status da Ana", |e| match e {
                RoomEvent::MemberUpdated(m) if m.id == ana_id => m.arcade.filter(|a| a.slots == 3).map(|_| ()),
                _ => None,
            });
            let no_window = ana.room.arcade_grant_kbm(Some(&beto.id)).unwrap_err();
            assert!(no_window.contains("não está jogando"), "{no_window}");
            ana.room.arcade_close();
        }
    }
    ana.room.stream_stopped();
}

/// O fliperama de ponta a ponta no mesmo PC: o Beto ganha a vaga e cada tecla do mapa padrão
/// (WASD, setas, J/Espaço, K, L, I, Enter...) chega ao controle virtual da Ana (o Windows vê pelo
/// XInput). Tirar o Beto solta a vaga e o controle.
#[test]
#[ignore = "precisa do ViGEmBus instalado e do Worker local (SQUADVIEW_SIGNALING_URL)"]
fn jogador_aperta_tecla_e_o_controle_virtual_do_host_aperta_o_botao() {
    let Some(url) = worker() else { return };
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let beto = join(&url, &code, "Beto", None);
    let ana_id = beto.id_of("Ana");
    let beto_id = ana.id_of("Beto");
    ana.room.stream_started(KeyframeHandle::detached());
    ana.room
        .arcade_open(Some("Teste".into()), 2, None)
        .expect("ViGEmBus instalado");

    beto.room.arcade_join(&ana_id, sv_input::KeyMap::default());
    let slot = beto.wait("a vaga", |u| match u {
        RoomUpdate::ArcadeSeat { host, slot } if host == ana_id => Some(slot),
        RoomUpdate::ArcadeLeft { reason, .. } => panic!("recusado: {reason}"),
        _ => None,
    });
    ana.wait("o Beto conectado", |u| match u {
        RoomUpdate::ArcadePlayers(seats) => seats
            .iter()
            .any(|s| s.member_id == beto_id && s.connected && s.slot == slot)
            .then_some(()),
        _ => None,
    });

    assert!((2..=4).contains(&slot), "o host é o P1; o Beto ganhou o P{slot}");
    // A vaga do XInput do controle virtual (com o host no teclado, é a 0, e mesmo assim P2).
    beto.room.arcade_key("KeyJ", true);
    let xinput = (0..100)
        .find_map(|_| {
            std::thread::sleep(Duration::from_millis(20));
            (0..sv_input::xinput::SLOTS)
                .find(|&i| sv_input::xinput::read(i).is_some_and(|p| p.pressed(sv_input::pad::buttons::A)))
        })
        .expect("o A do controle virtual apertou");
    beto.room.arcade_key("KeyJ", false);

    use sv_input::pad::buttons;
    type Check = fn(&sv_input::PadState) -> bool;
    let keys: [(&str, Check); 14] = [
        ("KeyW", |p| p.thumb_ly > 20_000),
        ("KeyS", |p| p.thumb_ly < -20_000),
        ("KeyA", |p| p.thumb_lx < -20_000),
        ("KeyD", |p| p.thumb_lx > 20_000),
        ("ArrowLeft", |p| p.thumb_lx < -20_000),
        ("ArrowRight", |p| p.thumb_lx > 20_000),
        ("Space", |p| p.pressed(buttons::A)),
        ("KeyK", |p| p.pressed(buttons::B)),
        ("KeyL", |p| p.pressed(buttons::X)),
        ("KeyI", |p| p.pressed(buttons::Y)),
        ("KeyU", |p| p.pressed(buttons::LEFT_SHOULDER)),
        ("KeyQ", |p| p.left_trigger > 200),
        ("Enter", |p| p.pressed(buttons::START)),
        ("Backspace", |p| p.pressed(buttons::BACK)),
    ];
    for (key, ok) in keys {
        beto.room.arcade_key(key, true);
        let seen = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            sv_input::xinput::read(xinput).is_some_and(|p| ok(&p))
        });
        beto.room.arcade_key(key, false);
        let released = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            sv_input::xinput::read(xinput).is_some_and(|p| p == sv_input::PadState::default())
        });
        assert!(seen, "{key} chegou ao controle virtual");
        assert!(released, "{key} soltou");
    }

    ana.room.arcade_kick(&beto_id);
    beto.wait("tirado do fliperama", |u| match u {
        RoomUpdate::ArcadeLeft { host, reason } if host == ana_id && !reason.is_empty() => Some(()),
        _ => None,
    });
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        sv_input::xinput::read(xinput).is_none()
    });
    assert!(gone, "o controle virtual saiu do PC");
    ana.room.stream_stopped();
}

/// Mede a latência da entrada do fliperama no mesmo PC (sinalização local, QUIC pelo loopback,
/// controle virtual do ViGEmBus): do `arcade_key` do jogador até o XInput do host ver o botão.
/// Não inclui a rede de verdade nem o vídeo; mostra o que o app acrescenta. Ver o PR para os
/// números antes e depois.
#[test]
#[ignore = "precisa do ViGEmBus instalado e do Worker local; mede, não confere comportamento"]
fn latencia_da_tecla_ate_o_controle_virtual() {
    let Some(url) = worker() else { return };
    let code = new_room_code();
    let ana = join(&url, &code, "Ana", None);
    let beto = join(&url, &code, "Beto", None);
    let ana_id = beto.id_of("Ana");
    ana.room.stream_started(KeyframeHandle::detached());
    ana.room
        .arcade_open(Some("Teste".into()), 1, None)
        .expect("ViGEmBus instalado");
    beto.room.arcade_join(&ana_id, sv_input::KeyMap::default());
    let slot = beto.wait("a vaga", |u| match u {
        RoomUpdate::ArcadeSeat { slot, .. } => Some(slot),
        _ => None,
    });
    ana.wait("o Beto conectado", |u| match u {
        RoomUpdate::ArcadePlayers(seats) => seats.iter().any(|s| s.connected).then_some(()),
        _ => None,
    });
    let xinput = u32::from(slot) - 1;
    let pressed =
        |want: bool| sv_input::xinput::read(xinput).is_some_and(|p| p.pressed(sv_input::pad::buttons::A) == want);
    let mut samples = Vec::new();
    for _ in 0..30 {
        for down in [true, false] {
            let started = Instant::now();
            beto.room.arcade_key("KeyJ", down);
            while !pressed(down) {
                assert!(
                    started.elapsed() < Duration::from_secs(2),
                    "o botão não chegou (amostra {}, apertando {down}): {:?}",
                    samples.len(),
                    sv_input::xinput::read(xinput)
                );
                std::hint::spin_loop();
            }
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    samples.sort_by(f64::total_cmp);
    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    let p95 = samples[samples.len() * 95 / 100];
    eprintln!(
        "LATENCIA entrada: média {avg:.2} ms, p95 {p95:.2} ms, máx {:.2} ms ({} amostras)",
        samples.last().unwrap(),
        samples.len()
    );
    ana.room.stream_stopped();
}
