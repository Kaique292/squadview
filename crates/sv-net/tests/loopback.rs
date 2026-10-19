//! Dois membros no mesmo PC: conexão autorizada, vídeo, áudio, relógio e recusa de quem não foi
//! autorizado. Sem STUN (não precisa de internet).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sv_net::protocol::FrameHeader;
use sv_net::{Broadcaster, Peer, Purpose, Receiver, ReceiverSink, VideoFrame};
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
        broadcaster.add(conn, Arc::new(move || *k.lock().unwrap() += 1));
    };
    let connect = viewer.connect(streamer.candidates(), streamer.fingerprint());
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
    let connect = intruder.connect(streamer.candidates(), streamer.fingerprint());
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
    let result = viewer.connect(streamer.candidates(), &fake).await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn conexao_de_voz_chega_marcada_como_voz_e_com_quem_e() {
    let ana = Peer::bind(&[]).await.unwrap();
    let beto = Peer::bind(&[]).await.unwrap();
    ana.allowed().allow(beto.fingerprint());

    let accept = ana.accept();
    let connect = beto.connect_for(ana.candidates(), ana.fingerprint(), Purpose::Voice);
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
