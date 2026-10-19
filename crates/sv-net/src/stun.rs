//! STUN mínimo (RFC 5389): descobre o endereço público que o roteador dá ao nosso socket UDP.
//!
//! Só o pedido "Binding" e a leitura de XOR-MAPPED-ADDRESS / MAPPED-ADDRESS.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const BINDING_REQUEST: u16 = 0x0001;
const BINDING_RESPONSE: u16 = 0x0101;
const MAGIC_COOKIE: u32 = 0x2112_A442;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// Servidores STUN públicos e gratuitos (só respondem "qual é o meu IP"; nenhuma mídia passa).
pub const DEFAULT_SERVERS: &[&str] = &["stun.cloudflare.com:3478", "stun.l.google.com:19302"];

/// Monta um pedido Binding com o identificador de transação `txid`.
pub fn binding_request(txid: [u8; 12]) -> [u8; 20] {
    let mut msg = [0u8; 20];
    msg[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Comprimento dos atributos: zero.
    msg[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    msg[8..20].copy_from_slice(&txid);
    msg
}

/// Lê a resposta e devolve o endereço mapeado, se ela for a resposta do pedido `txid`.
pub fn parse_response(data: &[u8], txid: [u8; 12]) -> Option<SocketAddr> {
    if data.len() < 20
        || u16::from_be_bytes([data[0], data[1]]) != BINDING_RESPONSE
        || u32::from_be_bytes([data[4], data[5], data[6], data[7]]) != MAGIC_COOKIE
        || data[8..20] != txid
    {
        return None;
    }
    let len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let attrs = data.get(20..20 + len)?;
    let mut fallback = None;
    let mut i = 0;
    while i + 4 <= attrs.len() {
        let kind = u16::from_be_bytes([attrs[i], attrs[i + 1]]);
        let size = u16::from_be_bytes([attrs[i + 2], attrs[i + 3]]) as usize;
        let value = attrs.get(i + 4..i + 4 + size)?;
        match kind {
            ATTR_XOR_MAPPED_ADDRESS => return decode_address(value, Some(txid)),
            ATTR_MAPPED_ADDRESS => fallback = decode_address(value, None),
            _ => {}
        }
        // Atributos são alinhados em 4 bytes.
        i += 4 + size.div_ceil(4) * 4;
    }
    fallback
}

fn decode_address(value: &[u8], xor_txid: Option<[u8; 12]>) -> Option<SocketAddr> {
    if value.len() < 8 {
        return None;
    }
    let family = value[1];
    let mut port = u16::from_be_bytes([value[2], value[3]]);
    let cookie = MAGIC_COOKIE.to_be_bytes();
    if xor_txid.is_some() {
        port ^= (MAGIC_COOKIE >> 16) as u16;
    }
    let ip = match family {
        0x01 => {
            let mut b: [u8; 4] = value[4..8].try_into().ok()?;
            if xor_txid.is_some() {
                for (x, c) in b.iter_mut().zip(cookie) {
                    *x ^= c;
                }
            }
            IpAddr::V4(Ipv4Addr::from(b))
        }
        0x02 => {
            let mut b: [u8; 16] = value.get(4..20)?.try_into().ok()?;
            if let Some(txid) = xor_txid {
                let key: Vec<u8> = cookie.iter().chain(txid.iter()).copied().collect();
                for (x, k) in b.iter_mut().zip(key) {
                    *x ^= k;
                }
            }
            IpAddr::V6(Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// Pergunta a cada servidor até um responder. Usa o próprio `socket` (o mesmo que o QUIC vai usar),
/// porque o endereço público vale para aquele socket.
pub fn discover(socket: &UdpSocket, servers: &[&str], timeout: Duration) -> Option<SocketAddr> {
    let previous = socket.read_timeout().ok().flatten();
    let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));
    let mut result = None;
    'servers: for server in servers {
        let Some(addr) = server
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.find(|a| a.is_ipv4() == socket.local_addr().is_ok_and(|l| l.is_ipv4())))
        else {
            continue;
        };
        let txid = random_txid();
        let request = binding_request(txid);
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 576];
        while Instant::now() < deadline {
            let _ = socket.send_to(&request, addr);
            if let Ok((n, from)) = socket.recv_from(&mut buf)
                && from == addr
                && let Some(mapped) = parse_response(&buf[..n], txid)
            {
                result = Some(mapped);
                break 'servers;
            }
        }
    }
    let _ = socket.set_read_timeout(previous);
    result
}

fn random_txid() -> [u8; 12] {
    let mut txid = [0u8; 12];
    // Não precisa ser criptográfico: só separa respostas de pedidos diferentes.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        ^ (std::process::id() as u128) << 64;
    for (i, b) in txid.iter_mut().enumerate() {
        *b = (seed >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(97);
    }
    txid
}

#[cfg(test)]
mod tests {
    use super::*;

    const TXID: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

    /// Resposta com XOR-MAPPED-ADDRESS de 203.0.113.5:40000, como um servidor real mandaria.
    fn response_v4() -> Vec<u8> {
        let port = 40000u16 ^ 0x2112;
        let ip = [203u8 ^ 0x21, 0x12, 113 ^ 0xA4, 5 ^ 0x42];
        let mut m = vec![0x01, 0x01, 0, 12, 0x21, 0x12, 0xA4, 0x42];
        m.extend_from_slice(&TXID);
        m.extend_from_slice(&[0x00, 0x20, 0, 8, 0, 0x01]);
        m.extend_from_slice(&port.to_be_bytes());
        m.extend_from_slice(&ip);
        m
    }

    #[test]
    fn pedido_tem_cabecalho_e_cookie_certos() {
        let r = binding_request(TXID);
        assert_eq!(&r[0..2], &[0, 1]);
        assert_eq!(&r[2..4], &[0, 0]);
        assert_eq!(&r[4..8], &[0x21, 0x12, 0xA4, 0x42]);
        assert_eq!(&r[8..], &TXID);
    }

    #[test]
    fn le_o_endereco_publico_ipv4() {
        assert_eq!(
            parse_response(&response_v4(), TXID),
            Some("203.0.113.5:40000".parse().unwrap())
        );
    }

    #[test]
    fn ignora_resposta_de_outro_pedido_ou_malformada() {
        assert_eq!(parse_response(&response_v4(), [0; 12]), None);
        assert_eq!(parse_response(&response_v4()[..10], TXID), None);
        let mut wrong_type = response_v4();
        wrong_type[1] = 0x11;
        assert_eq!(parse_response(&wrong_type, TXID), None);
    }

    #[test]
    fn aceita_mapped_address_sem_xor_de_servidores_antigos() {
        let mut m = vec![0x01, 0x01, 0, 12, 0x21, 0x12, 0xA4, 0x42];
        m.extend_from_slice(&TXID);
        m.extend_from_slice(&[0x00, 0x01, 0, 8, 0, 0x01, 0x1F, 0x90, 10, 0, 0, 7]);
        assert_eq!(parse_response(&m, TXID), Some("10.0.0.7:8080".parse().unwrap()));
    }

    #[test]
    fn le_endereco_ipv6_com_xor() {
        let ip: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let mut key = MAGIC_COOKIE.to_be_bytes().to_vec();
        key.extend_from_slice(&TXID);
        let x: Vec<u8> = ip.octets().iter().zip(&key).map(|(a, b)| a ^ b).collect();
        let mut m = vec![0x01, 0x01, 0, 24, 0x21, 0x12, 0xA4, 0x42];
        m.extend_from_slice(&TXID);
        m.extend_from_slice(&[0x00, 0x20, 0, 20, 0, 0x02]);
        m.extend_from_slice(&(5000u16 ^ 0x2112).to_be_bytes());
        m.extend_from_slice(&x);
        assert_eq!(parse_response(&m, TXID), Some(SocketAddr::new(IpAddr::V6(ip), 5000)));
    }

    #[test]
    #[ignore = "precisa de internet"]
    fn descobre_o_ip_publico_de_verdade() {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let addr = discover(&socket, DEFAULT_SERVERS, Duration::from_secs(2)).expect("nenhum STUN respondeu");
        eprintln!("endereço público: {addr}");
        assert!(addr.port() > 0);
    }
}
