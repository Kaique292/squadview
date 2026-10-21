//! Rich Presence: mostra no perfil do Discord que a pessoa está numa sala do SquadView (e se está
//! transmitindo ou assistindo), com o botão "Entrar na sala" quando ela permite.
//!
//! Fala com o cliente do Discord aberto no PC pelo pipe local `\\.\pipe\discord-ipc-N` (o mesmo
//! que jogos usam). Tudo roda numa thread própria: se o Discord estiver fechado, tenta de novo de
//! tempos em tempos, e nunca segura a interface nem a mídia.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

/// ID público do app no Discord Developer Portal.
const CLIENT_ID: &str = "1561766901233750108";
/// O Discord aceita poucas atualizações por minuto; abaixo disso, junta as mudanças.
const MIN_INTERVAL: Duration = Duration::from_secs(4);
/// Com o Discord fechado, tenta conectar de novo depois disso.
const RETRY: Duration = Duration::from_secs(15);
/// Maior mensagem aceita do Discord.
const MAX_FRAME: usize = 64 * 1024;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

/// O que a interface manda para aparecer no perfil.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceInfo {
    pub members: u32,
    pub live: bool,
    /// Nome de quem estamos assistindo.
    pub watching: Option<String>,
    /// Quando entramos na sala (ms desde 1970).
    pub joined_at_ms: i64,
    /// Link de convite para o botão; ausente se a pessoa não quer o botão no perfil.
    pub invite_url: Option<String>,
}

pub struct Presence(Mutex<Sender<Option<PresenceInfo>>>);

impl Presence {
    /// Liga a thread do Rich Presence.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || run(rx))
            .expect("thread do Rich Presence");
        Self(Mutex::new(tx))
    }
}

/// Atualiza (ou limpa, com `None`) o que aparece no Discord.
#[tauri::command]
pub fn set_presence(state: tauri::State<'_, Presence>, info: Option<PresenceInfo>) {
    let _ = state.0.lock().unwrap_or_else(|e| e.into_inner()).send(info);
}

fn run(rx: Receiver<Option<PresenceInfo>>) {
    let mut desired: Option<PresenceInfo> = None;
    // O que o Discord está mostrando (`None` = não sabemos).
    let mut shown: Option<Option<PresenceInfo>> = None;
    let mut ipc: Option<Ipc<std::fs::File>> = None;
    let mut last_send: Option<Instant> = None;
    let mut last_attempt: Option<Instant> = None;

    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(info) => desired = info,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if shown.as_ref() == Some(&desired) {
            continue;
        }
        // Respeita o limite do Discord e fica só com a mudança mais recente.
        if let Some(wait) = last_send.map(|t| MIN_INTERVAL.saturating_sub(t.elapsed())) {
            std::thread::sleep(wait);
        }
        while let Ok(info) = rx.try_recv() {
            desired = info;
        }

        if ipc.is_none() {
            if desired.is_none() {
                // Nada para mostrar e nada conectado: não há o que limpar.
                shown = Some(None);
                continue;
            }
            if last_attempt.is_some_and(|t| t.elapsed() < RETRY) {
                continue;
            }
            last_attempt = Some(Instant::now());
            ipc = connect().ok();
        }
        if let Some(conn) = ipc.as_mut() {
            match conn.set_activity(desired.as_ref()) {
                Ok(()) => {
                    shown = Some(desired.clone());
                    last_send = Some(Instant::now());
                }
                Err(_) => {
                    // Discord fechou ou reiniciou: reconecta na próxima volta.
                    ipc = None;
                    shown = None;
                    last_attempt = None;
                }
            }
        }
    }
}

/// Procura o Discord aberto (ele usa o primeiro pipe livre entre 0 e 9).
fn connect() -> io::Result<Ipc<std::fs::File>> {
    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{i}");
        if let Ok(pipe) = OpenOptions::new().read(true).write(true).open(path)
            && let Ok(ipc) = Ipc::handshake(pipe)
        {
            return Ok(ipc);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "Discord fechado"))
}

/// Conexão com o cliente do Discord: quadros `[op u32][tamanho u32][JSON]`, em little-endian.
struct Ipc<S> {
    stream: S,
    nonce: u64,
}

impl<S: Read + Write> Ipc<S> {
    fn handshake(stream: S) -> io::Result<Self> {
        let mut ipc = Self { stream, nonce: 0 };
        ipc.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": CLIENT_ID }))?;
        match ipc.recv()? {
            (OP_FRAME, body) if body["evt"] == "READY" => Ok(ipc),
            _ => Err(io::Error::other("o Discord recusou a conexão")),
        }
    }

    fn set_activity(&mut self, info: Option<&PresenceInfo>) -> io::Result<()> {
        self.nonce += 1;
        let message = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": info.map(activity) },
            "nonce": self.nonce.to_string(),
        });
        self.send(OP_FRAME, &message)?;
        match self.recv()? {
            (OP_CLOSE, _) => Err(io::Error::other("o Discord fechou a conexão")),
            // Um erro do Discord (ex.: campo recusado) não derruba a conexão.
            _ => Ok(()),
        }
    }

    fn send(&mut self, op: u32, value: &Value) -> io::Result<()> {
        self.stream.write_all(&encode(op, value)?)?;
        self.stream.flush()
    }

    fn recv(&mut self) -> io::Result<(u32, Value)> {
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header)?;
        let op = u32::from_le_bytes(header[..4].try_into().expect("4 bytes"));
        let len = u32::from_le_bytes(header[4..].try_into().expect("4 bytes")) as usize;
        if len > MAX_FRAME {
            return Err(io::Error::other("mensagem grande demais do Discord"));
        }
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body)?;
        Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }
}

fn encode(op: u32, value: &Value) -> io::Result<Vec<u8>> {
    let data = serde_json::to_vec(value)?;
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&data);
    Ok(frame)
}

/// O que aparece no perfil: o que a pessoa está fazendo, quantos na sala e há quanto tempo.
fn activity(info: &PresenceInfo) -> Value {
    let details = if info.live {
        "Transmitindo a tela".to_owned()
    } else if let Some(name) = &info.watching {
        format!("Assistindo {name}")
    } else {
        "Numa sala com a galera".to_owned()
    };
    // A sala não tem limite (ADR 0012): sem o "(3 de 8)" do Discord, quantos estão vai no texto.
    let state = match info.members {
        0 | 1 => "Na sala".to_owned(),
        n => format!("Na sala com {n} pessoas"),
    };
    let mut activity = json!({
        "details": details,
        "state": state,
        "timestamps": { "start": info.joined_at_ms },
        "assets": { "large_image": "logo", "large_text": "SquadView" },
        "instance": false,
    });
    if let Some(url) = &info.invite_url {
        activity["buttons"] = json!([{ "label": "Entrar na sala", "url": url }]);
    }
    activity
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Discord de mentira: devolve as respostas prontas e guarda o que o app mandou.
    struct FakeDiscord {
        replies: Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Read for FakeDiscord {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.replies.read(buf)
        }
    }
    impl Write for FakeDiscord {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sent.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn fake(replies: &[(u32, Value)]) -> FakeDiscord {
        let bytes = replies.iter().flat_map(|(op, v)| encode(*op, v).unwrap()).collect();
        FakeDiscord {
            replies: Cursor::new(bytes),
            sent: Vec::new(),
        }
    }

    fn frames(mut bytes: &[u8]) -> Vec<(u32, Value)> {
        let mut out = Vec::new();
        while bytes.len() >= 8 {
            let op = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            out.push((op, serde_json::from_slice(&bytes[8..8 + len]).unwrap()));
            bytes = &bytes[8 + len..];
        }
        out
    }

    fn info() -> PresenceInfo {
        PresenceInfo {
            members: 3,
            live: false,
            watching: Some("Minato".into()),
            joined_at_ms: 1_760_000_000_000,
            invite_url: None,
        }
    }

    #[test]
    fn handshake_e_atividade_seguem_o_protocolo_do_discord() {
        let discord = fake(&[
            (OP_FRAME, json!({ "evt": "READY" })),
            (OP_FRAME, json!({ "cmd": "SET_ACTIVITY" })),
        ]);
        let mut ipc = Ipc::handshake(discord).unwrap();
        ipc.set_activity(Some(&info())).unwrap();

        let sent = frames(&ipc.stream.sent);
        assert_eq!(sent[0].0, OP_HANDSHAKE);
        assert_eq!(sent[0].1["client_id"], CLIENT_ID);
        assert_eq!(sent[1].0, OP_FRAME);
        assert_eq!(sent[1].1["cmd"], "SET_ACTIVITY");
        assert_eq!(sent[1].1["args"]["activity"]["details"], "Assistindo Minato");
        assert_eq!(sent[1].1["args"]["pid"], std::process::id());
    }

    #[test]
    fn limpar_manda_atividade_nula() {
        let discord = fake(&[(OP_FRAME, json!({ "evt": "READY" })), (OP_FRAME, json!({}))]);
        let mut ipc = Ipc::handshake(discord).unwrap();
        ipc.set_activity(None).unwrap();
        let sent = frames(&ipc.stream.sent);
        assert!(sent[1].1["args"]["activity"].is_null());
    }

    #[test]
    fn discord_recusando_o_handshake_e_erro() {
        let discord = fake(&[(OP_CLOSE, json!({ "code": 4000, "message": "Invalid Client ID" }))]);
        assert!(Ipc::handshake(discord).is_err());
    }

    #[test]
    fn discord_fechando_derruba_a_conexao() {
        let discord = fake(&[(OP_FRAME, json!({ "evt": "READY" })), (OP_CLOSE, json!({}))]);
        let mut ipc = Ipc::handshake(discord).unwrap();
        assert!(ipc.set_activity(Some(&info())).is_err());
    }

    #[test]
    fn recusa_mensagem_grande_demais() {
        let mut bytes = OP_FRAME.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(MAX_FRAME as u32 + 1).to_le_bytes());
        let mut ipc = Ipc {
            stream: FakeDiscord {
                replies: Cursor::new(bytes),
                sent: Vec::new(),
            },
            nonce: 0,
        };
        assert!(ipc.recv().is_err());
    }

    #[test]
    fn atividade_mostra_o_que_a_pessoa_esta_fazendo() {
        let live = activity(&PresenceInfo { live: true, ..info() });
        assert_eq!(live["details"], "Transmitindo a tela");
        assert_eq!(live["state"], "Na sala com 3 pessoas");
        assert!(live.get("party").is_none());
        assert_eq!(live["timestamps"]["start"], 1_760_000_000_000_i64);
        assert!(live.get("buttons").is_none());

        let idle = activity(&PresenceInfo {
            watching: None,
            ..info()
        });
        assert_eq!(idle["details"], "Numa sala com a galera");
        let alone = activity(&PresenceInfo { members: 1, ..info() });
        assert_eq!(alone["state"], "Na sala");
    }

    #[test]
    fn botao_de_convite_so_com_permissao() {
        let a = activity(&PresenceInfo {
            invite_url: Some("https://kaique292.github.io/squadview/convite/?sala=F5B6S9".into()),
            ..info()
        });
        assert_eq!(a["buttons"][0]["label"], "Entrar na sala");
        assert_eq!(
            a["buttons"][0]["url"],
            "https://kaique292.github.io/squadview/convite/?sala=F5B6S9"
        );
    }
}
