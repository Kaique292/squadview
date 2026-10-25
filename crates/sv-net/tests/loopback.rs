//! Dois membros no mesmo PC: conexão autorizada, vídeo, áudio, relógio e recusa de quem não foi
//! autorizado. Sem STUN (não precisa de internet).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sv_net::protocol::FrameHeader;
use sv_net::stripes::{STRIPES, Splitter, StripeHeader};
use sv_net::{
    Broadcaster, Layer, Peer, Purpose, Receiver, ReceiverSink, StripeReceiver, StripeSink, VideoFrame, send_stripe,
};
use tokio::sync::mpsc;

#[derive(Debug)]
enum Got {
    Video(FrameHeader, Vec<u8>),
    Audio(u32, Vec<u8>),
    Closed,
}

struct Sink(mpsc::UnboundedSender<Got>);

impl ReceiverSink for Sink {
    fn video(&mut self, frame: FrameHeader, data: Vec<u8>) {
        let _ = self.0.send(Got::Video(frame, data));
    }
    fn audio(&mut self, seq: u32, _ts: u64, _peak: f32, opus: &[u8]) {
        let _ = self.0.send(Got::Audio(seq, opus.to_vec()));
    }
    fn closed(&mut self, _reason: String) {
        let _ = self.0.send(Got::Closed);
    }
}

fn frame(seq: u32, keyframe: bool, size: usize) -> VideoFrame {
    VideoFrame {
        header: FrameHeader {
            seq,
            timestamp_us: 1_000 + u64::from(seq),
            keyframe,
            encode_us: 2_000,
        },
        data: Arc::from(vec![(seq % 251) as u8; size]),
        layer: Layer::Main,
    }
}

async fn next(rx: &mut mpsc::UnboundedReceiver<Got>) -> Got {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("nada chegou em 5 s")
        .expect("canal fechou")
}

#[tokio::test(flavor = "multi_thread")]
async fn assiste_um_stream_de_ponta_a_ponta() {
    let streamer = Peer::bind(&[]).await.unwrap();
    let viewer = Peer::bind(&[]).await.unwrap();
    streamer.allowed().allow(viewer.fingerprint());

    let keyframes = Arc::new(Mutex::new(0u32));
    let broadcaster = Broadcaster::new();
    let accept = async {
        let accepted = streamer.accept().await.expect("aceitar");
        assert_eq!(accepted.purpose, Purpose::Stream);
        assert_eq!(accepted.fingerprint.as_deref(), Some(viewer.fingerprint()));
        let conn = accepted.conn;
        let k = keyframes.clone();
        broadcaster.add(conn, false, Arc::new(move |_| *k.lock().unwrap() += 1));
    };
    let connect = viewer.connect(streamer.candidates(), None, streamer.fingerprint());
    let (_, conn) = tokio::join!(accept, connect);
    let conn = conn.expect("conectar");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let receiver = Receiver::start(conn, Sink(tx));
    assert_eq!(broadcaster.viewer_count(), 1);
    assert_eq!(*keyframes.lock().unwrap(), 1, "quem entra ganha um keyframe");

    // Um keyframe grande (300 KB) e dois frames comuns, na ordem.
    broadcaster.send_video(&frame(0, true, 300_000));
    broadcaster.send_video(&frame(1, false, 20_000));
    broadcaster.send_video(&frame(2, false, 20_000));
    for expected in 0..3u32 {
        match next(&mut rx).await {
            Got::Video(h, data) => {
                assert_eq!(h.seq, expected);
                assert_eq!(data.len(), if expected == 0 { 300_000 } else { 20_000 });
                assert!(data.iter().all(|&b| b == (expected % 251) as u8));
            }
            other => panic!("esperava vídeo, veio {other:?}"),
        }
    }

    // Áudio por datagrama.
    broadcaster.send_audio(5, 123, 0.3, &[1, 2, 3, 4]);
    loop {
        match next(&mut rx).await {
            Got::Audio(seq, opus) => {
                assert_eq!((seq, opus), (5, vec![1, 2, 3, 4]));
                break;
            }
            Got::Video(..) => {}
            Got::Closed => panic!("fechou antes do áudio"),
        }
    }

    // Pedido de keyframe de quem assiste chega em quem transmite.
    receiver.request_keyframe();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*keyframes.lock().unwrap(), 2);

    // O relógio é medido (mesmo PC: diferença perto de zero).
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(receiver.rtt_us() > 0, "nenhum ping respondido");
    assert!(receiver.clock_offset().load(std::sync::atomic::Ordering::Relaxed).abs() < 20_000);

    // Quem transmite para: quem assiste fica sabendo.
    broadcaster.close();
    loop {
        if let Got::Closed = next(&mut rx).await {
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn quem_nao_foi_autorizado_nao_conecta() {
    let streamer = Peer::bind(&[]).await.unwrap();
    let intruder = Peer::bind(&[]).await.unwrap();
    // Ninguém autorizado: o handshake falha do lado de quem transmite.
    let accept = tokio::time::timeout(Duration::from_secs(3), streamer.accept());
    let connect = intruder.connect(streamer.candidates(), None, streamer.fingerprint());
    let (accepted, connected) = tokio::join!(accept, connect);
    assert!(accepted.is_err(), "não podia aceitar ninguém");
    // A conexão pode até abrir do lado do cliente e cair logo; o que importa é não receber nada.
    if let Ok(conn) = connected {
        let closed = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
        assert!(closed.is_ok(), "a conexão não autorizada devia cair");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nao_conecta_em_quem_tem_outra_fingerprint() {
    let streamer = Peer::bind(&[]).await.unwrap();
    let viewer = Peer::bind(&[]).await.unwrap();
    streamer.allowed().allow(viewer.fingerprint());
    // Alguém se passando por quem transmite (fingerprint diferente da combinada).
    let fake = "00".repeat(32);
    let result = viewer.connect(streamer.candidates(), None, &fake).await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn conexao_de_voz_chega_marcada_como_voz_e_com_quem_e() {
    let ana = Peer::bind(&[]).await.unwrap();
    let beto = Peer::bind(&[]).await.unwrap();
    ana.allowed().allow(beto.fingerprint());

    let accept = ana.accept();
    let connect = beto.connect_for(ana.candidates(), None, ana.fingerprint(), Purpose::Voice);
    let (accepted, conn) = tokio::join!(accept, connect);
    let accepted = accepted.expect("aceitar");
    let conn = conn.expect("conectar");
    assert_eq!(accepted.purpose, Purpose::Voice);
    assert_eq!(accepted.fingerprint.as_deref(), Some(beto.fingerprint()));

    // Voz vai em datagramas, nos dois sentidos.
    conn.send_datagram(bytes::Bytes::from_static(b"oi")).unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), accepted.conn.read_datagram())
        .await
        .expect("nada chegou")
        .expect("datagrama");
    assert_eq!(&got[..], b"oi");
}

/// Espera até `pred` valer (ou o prazo acabar) e devolve o último valor visto.
async fn settle(broadcaster: &Broadcaster, pred: impl Fn(usize) -> bool) -> usize {
    for _ in 0..100 {
        if pred(broadcaster.viewer_count()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    broadcaster.viewer_count()
}

/// Espera um frame de vídeo cujos bytes são todos `fill`, descartando os outros.
async fn video_filled_with(rx: &mut mpsc::UnboundedReceiver<Got>, fill: u8) -> FrameHeader {
    loop {
        if let Got::Video(header, data) = next(rx).await
            && data.iter().all(|b| *b == fill)
        {
            return header;
        }
    }
}

// Simulcast (ADR 0026): quem perde frames desce para a camada baixa no keyframe dela, e quem tem
// um app antigo (não avisou que aceita trocar de camada) continua na principal.
#[tokio::test(flavor = "multi_thread")]
async fn quem_perde_frames_desce_para_a_camada_baixa_e_o_app_antigo_fica_na_principal() {
    let streamer = Peer::bind(&[]).await.unwrap();
    let novo = Peer::bind(&[]).await.unwrap();
    let antigo = Peer::bind(&[]).await.unwrap();
    streamer.allowed().allow(novo.fingerprint());
    streamer.allowed().allow(antigo.fingerprint());
    let broadcaster = Broadcaster::new();
    let pedidos = Arc::new(Mutex::new(Vec::new()));
    let mut receivers = Vec::new();
    for (viewer, simulcast) in [(&novo, true), (&antigo, false)] {
        let accept = async {
            let accepted = streamer.accept().await.expect("aceitar");
            let p = pedidos.clone();
            broadcaster.add(
                accepted.conn,
                simulcast,
                Arc::new(move |layer| p.lock().unwrap().push((simulcast, layer))),
            );
        };
        let connect = viewer.connect(streamer.candidates(), None, streamer.fingerprint());
        let (_, conn) = tokio::join!(accept, connect);
        let (tx, rx) = mpsc::unbounded_channel();
        receivers.push((Receiver::start(conn.expect("conectar"), Sink(tx)), rx));
    }
    // Os `Receiver` ficam vivos até o fim: descartar um fecha a conexão.
    let (_novo, mut novo_rx) = receivers.remove(0);
    let (_antigo, mut antigo_rx) = receivers.remove(0);

    // Uma rajada maior que a fila dos dois (6 frames): os dois perdem frames no mesmo segundo.
    // Os frames já prontos antes, para a rajada sair mais rápido do que qualquer fila esvazia.
    let burst: Vec<_> = (0..60).map(|seq| frame(seq, seq == 0, 400_000)).collect();
    for f in &burst {
        broadcaster.send_video(f);
    }
    let pedidos_agora = pedidos.lock().unwrap().clone();
    assert!(pedidos_agora.contains(&(true, Layer::Low)), "{pedidos_agora:?}");
    assert!(!pedidos_agora.contains(&(false, Layer::Low)), "{pedidos_agora:?}");
    assert!(broadcaster.wants_low(), "o segundo encoder tem que ligar");

    // As filas esvaziam; chega o keyframe da camada baixa e depois um da principal.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let low = VideoFrame {
        header: FrameHeader {
            seq: 0,
            timestamp_us: 2_000,
            keyframe: true,
            encode_us: 1_000,
        },
        data: Arc::from(vec![0xaa; 1_000]),
        layer: Layer::Low,
    };
    broadcaster.send_video(&low);
    let main = VideoFrame {
        header: FrameHeader {
            seq: 60,
            timestamp_us: 2_100,
            keyframe: true,
            encode_us: 1_000,
        },
        data: Arc::from(vec![0xbb; 1_000]),
        layer: Layer::Main,
    };
    broadcaster.send_video(&main);

    // O novo recebe a baixa e para de receber a principal; o antigo só recebe a principal.
    assert_eq!(video_filled_with(&mut novo_rx, 0xaa).await.timestamp_us, 2_000);
    assert_eq!(video_filled_with(&mut antigo_rx, 0xbb).await.timestamp_us, 2_100);
    let next_frame = VideoFrame {
        header: FrameHeader {
            seq: 1,
            timestamp_us: 2_200,
            keyframe: false,
            encode_us: 1_000,
        },
        data: Arc::from(vec![0xcc; 1_000]),
        layer: Layer::Low,
    };
    broadcaster.send_video(&next_frame);
    // Antes da baixa seguinte, nada da principal chegou ao novo.
    loop {
        match next(&mut novo_rx).await {
            Got::Video(_, data) if data.iter().all(|b| *b == 0xcc) => break,
            Got::Video(_, data) => assert!(!data.iter().all(|b| *b == 0xbb), "a principal chegou ao novo"),
            _ => {}
        }
    }
}

/// O que sai da remontagem das faixas.
#[derive(Debug, PartialEq)]
enum Assembled {
    Frame(u32, Vec<u8>),
    Lost(u32, u32),
}

struct Stripes(mpsc::UnboundedSender<Assembled>);

impl StripeSink for Stripes {
    fn frame(&mut self, header: FrameHeader, data: Vec<u8>) {
        let _ = self.0.send(Assembled::Frame(header.seq, data));
    }
    fn lost(&mut self, first_seq: u32, count: u32) {
        let _ = self.0.send(Assembled::Lost(first_seq, count));
    }
}

// Faixas e paridade (ADR 0028): cada faixa vem por uma conexão (um pai em cada árvore). Uma
// conexão que cai no meio do stream tira uma faixa, e os frames continuam chegando inteiros.
#[tokio::test(flavor = "multi_thread")]
async fn faixas_por_cinco_conexoes_continuam_chegando_quando_uma_cai() {
    let parent = Peer::bind(&[]).await.unwrap();
    let child = Peer::bind(&[]).await.unwrap();
    parent.allowed().allow(child.fingerprint());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let receiver = StripeReceiver::new(Stripes(tx));
    let mut paths = Vec::new();
    let mut child_side = Vec::new();
    for _ in 0..STRIPES {
        let accept = parent.accept();
        let connect = child.connect(parent.candidates(), None, parent.fingerprint());
        let (accepted, conn) = tokio::join!(accept, connect);
        let conn = conn.expect("conectar");
        receiver.attach(conn.clone());
        child_side.push(conn);
        paths.push(accepted.expect("aceitar").conn);
    }

    let mut splitter = Splitter::new();
    let frame_bytes = |seq: u32| -> Vec<u8> { (0..20_003u32).map(|i| (i ^ seq.wrapping_mul(13)) as u8).collect() };
    for seq in 0..30u32 {
        // Depois do frame 10, quem repassava a faixa 2 sai da sala.
        if seq == 10 {
            paths[2].close(0u32.into(), b"saiu");
        }
        let data = frame_bytes(seq);
        let stripes = splitter.split(&data).clone();
        for (i, path) in paths.iter().enumerate() {
            let header = StripeHeader {
                frame: FrameHeader {
                    seq,
                    timestamp_us: u64::from(seq),
                    keyframe: seq == 0,
                    encode_us: 1,
                },
                layer: 0,
                index: i as u8,
                frame_len: data.len() as u32,
            };
            let _ = send_stripe(path, &header, &stripes[i]).await;
        }
    }

    for seq in 0..30u32 {
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("frame não chegou em 5 s")
            .expect("canal fechou");
        assert_eq!(got, Assembled::Frame(seq, frame_bytes(seq)), "frame {seq}");
    }
    drop(child_side);
}

// Achado na esteira: com vários endereços que respondem (o local e o 127.0.0.1), quem transmite
// chegou a contar dois espectadores. A tentativa que perde fecha, e quem transmite tem que tirá-la.
#[tokio::test(flavor = "multi_thread")]
async fn dois_enderecos_que_respondem_viram_uma_conexao_so() {
    let streamer = Arc::new(Peer::bind(&[]).await.unwrap());
    let viewer = Peer::bind(&[]).await.unwrap();
    streamer.allowed().allow(viewer.fingerprint());
    let broadcaster = Arc::new(Broadcaster::new());
    let (s, b) = (streamer.clone(), broadcaster.clone());
    tokio::spawn(async move {
        while let Some(accepted) = s.accept().await {
            b.add(accepted.conn, false, Arc::new(|_| {}));
        }
    });

    // Todos os endereços de quem transmite, repetidos: várias tentativas chegam ao mesmo tempo.
    let targets: Vec<_> = streamer.candidates().iter().copied().cycle().take(6).collect();
    let conn = viewer
        .connect(&targets, None, streamer.fingerprint())
        .await
        .expect("conectar");
    // Dá tempo para as tentativas que perderam chegarem e serem fechadas.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(settle(&broadcaster, |n| n == 1).await, 1, "um espectador só");
    drop(conn);
}

// Bug achado na esteira: quem fechava a conexão sem ter recebido nenhum frame continuava contado
// como espectador até o próximo frame.
#[tokio::test(flavor = "multi_thread")]
async fn quem_sai_antes_do_primeiro_frame_deixa_de_contar() {
    let streamer = Peer::bind(&[]).await.unwrap();
    let viewer = Peer::bind(&[]).await.unwrap();
    streamer.allowed().allow(viewer.fingerprint());
    let broadcaster = Broadcaster::new();
    let accept = async {
        let accepted = streamer.accept().await.expect("aceitar");
        broadcaster.add(accepted.conn, false, Arc::new(|_| {}));
    };
    let connect = viewer.connect(streamer.candidates(), None, streamer.fingerprint());
    let (_, conn) = tokio::join!(accept, connect);
    let conn = conn.expect("conectar");
    assert_eq!(broadcaster.viewer_count(), 1);

    conn.close(0u32.into(), b"tchau");
    assert_eq!(settle(&broadcaster, |n| n == 0).await, 0, "saiu: não conta mais");
}

/// Conecta só pelo IPv6 (o caminho grátis para quem está atrás de CGNAT no IPv4). Só roda num PC
/// com IPv6 global; sem ele, avisa e pula.
#[tokio::test(flavor = "multi_thread")]
async fn conecta_so_pelo_ipv6_quando_os_dois_tem() {
    let ana = Peer::bind(&[]).await.unwrap();
    let beto = Peer::bind(&[]).await.unwrap();
    let only_v6: Vec<_> = ana.candidates().iter().copied().filter(|a| a.is_ipv6()).collect();
    if only_v6.is_empty() {
        eprintln!("pulado: este PC não tem IPv6 global");
        return;
    }
    ana.allowed().allow(beto.fingerprint());
    // Como na sala: quem aceita "fura" na direção de quem pede (abre o firewall), e quem pede conecta.
    ana.punch(beto.candidates());
    let started = std::time::Instant::now();
    let accept = ana.accept();
    let connect = beto.connect(&only_v6, None, ana.fingerprint());
    let (accepted, conn) = tokio::join!(accept, connect);
    let conn = conn.expect("conectar pelo IPv6");
    eprintln!("IPv6 conectou em {:?}", started.elapsed());
    assert!(conn.remote_address().is_ipv6());
    assert!(accepted.expect("aceitar").conn.remote_address().is_ipv6());
}

// Bug achado com o IPv6: com mais endereços, duas tentativas completavam ao mesmo tempo, quem
// aceitava entregava a que o outro lado ia descartar, e a voz caía. Agora só sai a que ficou.
#[tokio::test(flavor = "multi_thread")]
async fn varias_tentativas_de_voz_entregam_so_a_que_ficou() {
    for _ in 0..5 {
        let ana = Peer::bind(&[]).await.unwrap();
        let beto = Peer::bind(&[]).await.unwrap();
        ana.allowed().allow(beto.fingerprint());
        ana.punch(beto.candidates());
        let targets: Vec<_> = ana.candidates().iter().copied().cycle().take(8).collect();
        let accept = ana.accept();
        let connect = beto.connect_for(&targets, None, ana.fingerprint(), Purpose::Voice);
        let (accepted, conn) = tokio::join!(accept, connect);
        let (accepted, conn) = (accepted.expect("aceitar"), conn.expect("conectar"));
        conn.send_datagram(bytes::Bytes::from_static(b"oi")).unwrap();
        let got = tokio::time::timeout(Duration::from_secs(3), accepted.conn.read_datagram())
            .await
            .expect("a voz foi para a conexão descartada")
            .expect("datagrama");
        assert_eq!(&got[..], b"oi");
    }
}
