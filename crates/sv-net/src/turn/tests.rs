use std::net::UdpSocket;
use std::sync::atomic::AtomicUsize;

use super::*;
use crate::Peer;

const TXID: [u8; 12] = [9; 12];

#[test]
fn embrulha_e_desembrulha_um_pacote_sem_perder_nada() {
    let peer: SocketAddr = "203.0.113.7:40000".parse().unwrap();
    let mut out = [0u8; MAX_WRAPPED];
    for size in [0, 1, 3, 4, 5, 1199, 1200] {
        let data: Vec<u8> = (0..size).map(|i| i as u8).collect();
        let n = encode_send(&mut out, TXID, peer, &data).unwrap();
        // Tamanho alinhado em 4 bytes, como o TURN pede.
        assert_eq!(n % 4, 0);
        assert_eq!(u16::from_be_bytes([out[0], out[1]]), SEND_INDICATION);
        // O repassador entrega do outro lado como Data indication: mesmo formato, outro tipo.
        out[0..2].copy_from_slice(&DATA_INDICATION.to_be_bytes());
        assert_eq!(parse(&out[..n]), Incoming::Data { peer, data: &data });
    }
}

#[test]
fn pacote_grande_demais_ou_ipv6_nao_e_embrulhado() {
    let mut out = [0u8; MAX_WRAPPED];
    let big = vec![0u8; MAX_WRAPPED];
    assert_eq!(
        encode_send(&mut out, TXID, "203.0.113.7:1".parse().unwrap(), &big),
        None
    );
    assert_eq!(
        encode_send(&mut out, TXID, "[2001:db8::1]:1".parse().unwrap(), b"oi"),
        None
    );
}

#[test]
fn le_o_desafio_e_a_resposta_da_alocacao() {
    let challenge = Message::new(ALLOCATE_ERROR, TXID)
        .attr(ATTR_ERROR_CODE, &[0, 0, 4, 1, b'n', b'o'])
        .attr(ATTR_REALM, b"cloudflare")
        .attr(ATTR_NONCE, b"abc")
        .finish();
    let Incoming::Response(r) = parse(&challenge) else {
        panic!()
    };
    assert_eq!(r.error, Some((401, "no".into())));
    assert_eq!(
        (r.realm.as_deref(), r.nonce.as_deref()),
        (Some("cloudflare"), Some("abc"))
    );

    let relayed: SocketAddr = "198.51.100.9:50000".parse().unwrap();
    let ok = Message::new(ALLOCATE_OK, TXID)
        .attr(ATTR_XOR_RELAYED_ADDRESS, &xor_address(relayed).unwrap())
        .attr(ATTR_LIFETIME, &600u32.to_be_bytes())
        .finish();
    let Incoming::Response(r) = parse(&ok) else { panic!() };
    assert_eq!((r.relayed, r.lifetime, r.error), (Some(relayed), Some(600), None));
}

#[test]
fn lixo_nao_quebra_a_leitura() {
    for junk in [&b""[..], b"svpunch", &[0x40; 40], &[0u8; 19]] {
        assert_eq!(parse(junk), Incoming::Other);
    }
}

#[test]
fn credencial_vem_da_primeira_url_turn_por_udp() {
    let urls = vec![
        "stun:127.0.0.1:3478".to_owned(),
        "turn:127.0.0.1:3478?transport=tcp".to_owned(),
        "turn:127.0.0.1:53?transport=udp".to_owned(),
    ];
    let server = TurnServer::from_urls(&urls, "u", "segredo").unwrap();
    assert_eq!(server.addr, "127.0.0.1:53".parse().unwrap());
    assert!(
        !format!("{server:?}").contains("segredo"),
        "a senha não pode ir para o registro"
    );
    assert!(TurnServer::from_urls(&urls[..2], "u", "p").is_none());
}

#[test]
fn so_ip_publico_entra_na_permissao() {
    assert!(is_public("203.0.113.7".parse().unwrap()));
    for ip in ["192.168.0.2", "10.0.0.1", "127.0.0.1", "169.254.1.1", "0.0.0.0"] {
        assert!(!is_public(ip.parse().unwrap()), "{ip}");
    }
}

// --- Repassador de mentira --------------------------------------------------------------------

const REALM: &str = "squadview";
const USER: &str = "u";
const PASSWORD: &str = "p";

/// Confere o MESSAGE-INTEGRITY de uma mensagem (como um repassador de verdade faz).
fn integrity_ok(msg: &[u8], key: &[u8; 16]) -> bool {
    let mut i = 20;
    while i + 4 <= msg.len() {
        let attr = u16::from_be_bytes([msg[i], msg[i + 1]]);
        let size = u16::from_be_bytes([msg[i + 2], msg[i + 3]]) as usize;
        if attr == ATTR_MESSAGE_INTEGRITY {
            let mut signed = msg[..i].to_vec();
            signed[2..4].copy_from_slice(&((i - 20 + 24) as u16).to_be_bytes());
            let hmac = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
            return ring::hmac::verify(&hmac, &signed, &msg[i + 4..i + 24]).is_ok();
        }
        i += 4 + size.div_ceil(4) * 4;
    }
    false
}

/// Um repassador TURN mínimo no próprio PC: exige a credencial, empresta um endereço e encaminha
/// nos dois sentidos. Conta os pedidos com credencial que recebeu.
struct FakeTurn {
    addr: SocketAddr,
    signed: Arc<AtomicUsize>,
}

fn fake_turn() -> FakeTurn {
    let control = UdpSocket::bind("127.0.0.1:0").unwrap();
    let relay = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = control.local_addr().unwrap();
    let relayed = relay.local_addr().unwrap();
    let client: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
    let signed = Arc::new(AtomicUsize::new(0));
    let key = long_term_key(USER, REALM, PASSWORD);

    // Do repassador para o cliente: o que chega no endereço emprestado vira Data indication.
    let (to_client, from_peers, seen) = (control.try_clone().unwrap(), relay.try_clone().unwrap(), client.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, peer)) = from_peers.recv_from(&mut buf) {
            let Some(client) = *lock(&seen) else { continue };
            let msg = Message::new(DATA_INDICATION, TXID)
                .attr(ATTR_XOR_PEER_ADDRESS, &xor_address(peer).unwrap())
                .attr(ATTR_DATA, &buf[..n])
                .finish();
            let _ = to_client.send_to(&msg, client);
        }
    });

    let counter = signed.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = control.recv_from(&mut buf) {
            let msg = &buf[..n];
            let kind = u16::from_be_bytes([msg[0], msg[1]]);
            let txid: [u8; 12] = msg[8..20].try_into().unwrap();
            if kind == SEND_INDICATION {
                // Mesmo formato da Data indication: lê com o mesmo código e manda para o destino.
                let mut copy = msg.to_vec();
                copy[0..2].copy_from_slice(&DATA_INDICATION.to_be_bytes());
                if let Incoming::Data { peer, data } = parse(&copy) {
                    let _ = relay.send_to(data, peer);
                }
                continue;
            }
            let reply = if !integrity_ok(msg, &key) {
                Message::new(kind | 0x0110, txid)
                    .attr(ATTR_ERROR_CODE, &[0, 0, 4, 1])
                    .attr(ATTR_REALM, REALM.as_bytes())
                    .attr(ATTR_NONCE, b"n1")
                    .finish()
            } else {
                counter.fetch_add(1, Ordering::Relaxed);
                *lock(&client) = Some(from);
                let ok = Message::new(kind | 0x0100, txid);
                if kind == ALLOCATE {
                    ok.attr(ATTR_XOR_RELAYED_ADDRESS, &xor_address(relayed).unwrap())
                        .attr(ATTR_LIFETIME, &600u32.to_be_bytes())
                        .finish()
                } else {
                    ok.finish()
                }
            };
            let _ = control.send_to(&reply, from);
        }
    });
    FakeTurn { addr, signed }
}

#[test]
fn alocacao_responde_ao_desafio_com_a_credencial_certa() {
    let turn = fake_turn();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = TurnServer {
        addr: turn.addr,
        username: USER.into(),
        password: PASSWORD.into(),
    };
    let (relayed, _) = allocate(&socket, &server, &AtomicU64::new(1)).unwrap();
    assert_eq!(relayed.ip(), "127.0.0.1".parse::<IpAddr>().unwrap());
    assert_eq!(turn.signed.load(Ordering::Relaxed), 1);

    // Senha errada: o repassador recusa (e não fica pedindo para sempre).
    let wrong = TurnServer {
        password: "errada".into(),
        ..server
    };
    assert!(matches!(
        allocate(&socket, &wrong, &AtomicU64::new(1)),
        Err(TurnError::Timeout | TurnError::Rejected(..))
    ));
}

#[test]
fn quic_conecta_pelo_endereco_emprestado_quando_nao_ha_caminho_direto() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let turn = fake_turn();
        let server = TurnServer {
            addr: turn.addr,
            username: USER.into(),
            password: PASSWORD.into(),
        };
        // Quem aceita (quem transmite) tem endereço no repassador; quem conecta não tem nenhum
        // endereço direto para tentar: só o repassador pode dar certo.
        let streamer = Peer::bind(&[]).await.unwrap();
        let viewer = Peer::bind(&[]).await.unwrap();
        let relay = streamer.enable_relay(server).await.expect("endereço emprestado");
        assert_eq!(streamer.relay_addr(), Some(relay));
        streamer.allowed().allow(viewer.fingerprint());
        streamer.permit_relay(viewer.candidates());

        let accept = async { streamer.accept().await.expect("aceitou") };
        let connect = viewer.connect(&[], Some(relay), streamer.fingerprint());
        let (accepted, conn) = tokio::join!(accept, connect);
        let conn = conn.expect("conectou pelo repassador");
        assert_eq!(conn.remote_address(), relay);

        // Os dados passam nos dois sentidos pelo repassador.
        let mut send = conn.open_uni().await.unwrap();
        send.write_all(b"ola pelo repassador").await.unwrap();
        send.finish().unwrap();
        let mut recv = accepted.conn.accept_uni().await.unwrap();
        assert_eq!(recv.read_to_end(1024).await.unwrap(), b"ola pelo repassador");
        assert!(turn.signed.load(Ordering::Relaxed) >= 1);
        streamer.close();
    });
}
