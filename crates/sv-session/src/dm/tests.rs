//! Dois apps no mesmo PC conversando de verdade (QUIC pelo loopback), com o sinal passando direto
//! de um para o outro no lugar do registro de amigos.

use std::sync::mpsc::{Receiver, channel};

use super::history::tests::temp_dir;
use super::*;

const ANA: &str = "111111111111111111";
const BETO: &str = "222222222222222222";

/// Um app: o motor e os eventos que ele mandou para a interface.
struct App {
    engine: Arc<DmEngine>,
    events: Receiver<DmEvent>,
    dir: PathBuf,
}

impl App {
    /// Espera um evento que passe no filtro (descarta os outros), até 10 s.
    fn wait(&self, what: &str, ok: impl Fn(&DmEvent) -> bool) -> DmEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.events.recv_timeout(left) {
                Ok(e) if ok(&e) => return e,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("não chegou: {what}");
    }

    fn message(&self, what: &str, ok: impl Fn(&DmMessage) -> bool) -> DmMessage {
        match self.wait(what, |e| matches!(e, DmEvent::Message { message, .. } if ok(message))) {
            DmEvent::Message { message, .. } => message,
            _ => unreachable!(),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Ana e Beto, com o sinal de um chegando no outro (e `online` dizendo se o "registro" entrega).
fn pair(name: &str) -> (App, App, Arc<std::sync::atomic::AtomicBool>) {
    type Slot = Arc<Mutex<Option<Arc<DmEngine>>>>;
    let (ana_slot, beto_slot): (Slot, Slot) = Default::default();
    let online = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let make = |me: &'static str, other: Slot, mine: Slot, other_id: &'static str| {
        let (tx, rx) = channel();
        let dir = temp_dir(&format!("{name}-{me}"));
        let online = online.clone();
        let sender = mine.clone();
        let engine = Arc::new(
            DmEngine::start(
                me,
                dir.join("conversas"),
                &[],
                Arc::new(move |to: &str, payload| {
                    assert_eq!(to, other_id);
                    // Como o registro: entrega se o amigo está online; senão avisa quem mandou.
                    let target = lock(&other).clone();
                    let from = lock(&sender).clone();
                    if online.load(Ordering::Relaxed) {
                        if let Some(target) = target {
                            target.on_signal(me, payload);
                        }
                    } else if let Some(from) = from {
                        from.on_unreachable(to);
                    }
                }),
                Arc::new(move |e| {
                    let _ = tx.send(e);
                }),
            )
            .unwrap(),
        );
        *lock(&mine) = Some(engine.clone());
        App {
            engine,
            events: rx,
            dir,
        }
    };
    let ana = make(ANA, beto_slot.clone(), ana_slot.clone(), BETO);
    let beto = make(BETO, ana_slot, beto_slot, ANA);
    (ana, beto, online)
}

#[test]
fn texto_vai_direto_e_volta_entregue() {
    let (ana, beto, _) = pair("texto");
    let sent = ana.engine.send_text(BETO, "  bora jogar?  ").unwrap();
    assert_eq!(sent.text.as_deref(), Some("bora jogar?"));
    assert_eq!(sent.status, Status::Pending);

    let got = beto.message("texto no Beto", |m| !m.mine);
    assert_eq!(got.text.as_deref(), Some("bora jogar?"));
    assert_eq!(got.id, sent.id);
    ana.message("entregue", |m| m.id == sent.id && m.status == Status::Delivered);
    assert!(ana.engine.online(BETO));
    assert!(beto.engine.online(ANA));

    // A resposta usa a mesma conexão.
    let reply = beto.engine.send_text(ANA, "bora").unwrap();
    ana.message("resposta", |m| m.id == reply.id && !m.mine);
    beto.message("resposta entregue", |m| {
        m.id == reply.id && m.status == Status::Delivered
    });

    // Cada um guarda o histórico no próprio PC.
    assert_eq!(ana.engine.messages(BETO).len(), 2);
    assert_eq!(beto.engine.conversations()[0].friend, ANA);
}

#[test]
fn amigo_offline_a_mensagem_espera_e_vai_quando_ele_volta() {
    let (ana, beto, online) = pair("offline");
    online.store(false, Ordering::Relaxed);
    let sent = ana.engine.send_text(BETO, "tá aí?").unwrap();
    ana.wait(
        "aviso de offline",
        |e| matches!(e, DmEvent::Unreachable { friend } if friend == BETO),
    );
    assert_eq!(ana.engine.conversations()[0].pending, 1);

    // O Beto voltou: a próxima tentativa leva a pendente junto.
    online.store(true, Ordering::Relaxed);
    let depois = ana.engine.send_text(BETO, "oi de novo").unwrap();
    beto.message("pendente", |m| m.id == sent.id);
    beto.message("nova", |m| m.id == depois.id);
    ana.message("pendente entregue", |m| {
        m.id == sent.id && m.status == Status::Delivered
    });
}

#[test]
fn os_dois_chamando_juntos_fica_uma_conexao_so() {
    let (ana, beto, _) = pair("juntos");
    let a = ana.engine.send_text(BETO, "oi Beto").unwrap();
    let b = beto.engine.send_text(ANA, "oi Ana").unwrap();
    beto.message("da Ana", |m| m.id == a.id);
    ana.message("do Beto", |m| m.id == b.id);
    ana.message("entregue para o Beto", |m| {
        m.id == a.id && m.status == Status::Delivered
    });
    beto.message("entregue para a Ana", |m| m.id == b.id && m.status == Status::Delivered);
}

#[test]
fn arquivo_vai_quando_pedido_e_continua_de_onde_parou() {
    let (ana, beto, _) = pair("arquivo");
    let source = ana.dir.join("clipe.mp4");
    std::fs::create_dir_all(&ana.dir).unwrap();
    let content: Vec<u8> = (0..1_500_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &content).unwrap();

    // Sem cópia: a conversa aponta para o arquivo original.
    let sent = ana.engine.send_file(BETO, &source, None).unwrap();
    let file = sent.file.clone().unwrap();
    assert_eq!(
        (file.name.as_str(), file.mime.as_str(), file.size),
        ("clipe.mp4", "video/mp4", 1_500_000)
    );
    assert_eq!(file.path.as_deref(), Some(source.to_string_lossy().as_ref()));

    let offer = beto.message("aviso do arquivo", |m| m.id == sent.id);
    assert!(
        offer.file.as_ref().unwrap().path.is_none(),
        "quem recebe ainda não baixou"
    );

    // Uma parte já tinha chegado antes: só o resto vem.
    let dest = beto.dir.join("baixados").join("clipe.mp4");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(part_of(&dest), &content[..600_000]).unwrap();
    beto.engine.download(ANA, &sent.id, &dest);
    let done = beto.message("baixado", |m| {
        m.id == sent.id && m.file.as_ref().is_some_and(|f| f.path.is_some())
    });
    assert_eq!(done.file.unwrap().done, 1_500_000);
    assert_eq!(std::fs::read(&dest).unwrap(), content);
    assert!(!part_of(&dest).exists());
}

#[test]
fn guardar_copia_aponta_para_a_copia_do_app() {
    let (ana, _beto, _) = pair("copia");
    let source = ana.dir.join("foto.png");
    std::fs::create_dir_all(&ana.dir).unwrap();
    std::fs::write(&source, b"png").unwrap();
    let anexos = ana.dir.join("anexos");
    let sent = ana.engine.send_file(BETO, &source, Some(&anexos)).unwrap();
    let path = PathBuf::from(sent.file.unwrap().path.unwrap());
    assert!(path.starts_with(&anexos));
    assert_eq!(std::fs::read(path).unwrap(), b"png");
}

#[test]
fn recusa_mensagem_vazia_longa_e_arquivo_que_nao_existe() {
    let (ana, _beto, _) = pair("recusa");
    assert_eq!(ana.engine.send_text(BETO, "   "), Err(SendError::Empty));
    assert_eq!(
        ana.engine.send_text(BETO, &"a".repeat(MAX_TEXT_CHARS + 1)),
        Err(SendError::TooLong)
    );
    assert!(matches!(
        ana.engine.send_file(BETO, Path::new("nao-existe.png"), None),
        Err(SendError::Unreadable(_))
    ));
    assert!(matches!(
        ana.engine.send_file(BETO, &std::env::temp_dir(), None),
        Err(SendError::Unreadable(_))
    ));
    assert!(SendError::TooLong.to_string().contains("4000"));
    // Sinal que não é de conversa é ignorado.
    ana.engine.on_signal(BETO, serde_json::json!({"kind": "outra-coisa"}));
    ana.engine.clear(None);
    assert!(ana.engine.conversations().is_empty());
    assert!(format!("{:?}", ana.engine).contains(ANA));
}

#[test]
fn tipo_do_arquivo_pelo_nome() {
    assert_eq!(mime_of("Foto.JPG"), "image/jpeg");
    assert_eq!(mime_of("video.webm"), "video/webm");
    assert_eq!(mime_of("sem-extensao"), "application/octet-stream");
    assert_eq!(part_of(Path::new("a/b.zip")), PathBuf::from("a/b.zip.part"));
}

#[test]
fn sinal_no_formato_combinado() {
    let offer = DmSignal::DmOffer {
        candidates: vec!["203.0.113.7:51000".parse().unwrap()],
        fingerprint: "ab".into(),
    };
    assert_eq!(
        serde_json::to_value(&offer).unwrap(),
        serde_json::json!({"kind": "dm-offer", "candidates": ["203.0.113.7:51000"], "fingerprint": "ab"})
    );
    // `relay` ausente vale `None` (quem não tem repassador nem manda).
    let answer: DmSignal = serde_json::from_value(serde_json::json!({
        "kind": "dm-answer", "candidates": [], "fingerprint": "cd"
    }))
    .unwrap();
    assert_eq!(
        answer,
        DmSignal::DmAnswer {
            candidates: vec![],
            fingerprint: "cd".into(),
            relay: None
        }
    );
}
