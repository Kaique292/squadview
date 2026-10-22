//! Cliente da sinalização (o Worker em `services/signaling`, contrato em `PROTOCOL.md`).
//!
//! Só serve para entrar na sala, saber quem está nela e quem está ao vivo, e trocar [`Signal`]s
//! com um membro. Nenhuma mídia passa por aqui.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::Signal;

/// Versão do contrato da sinalização (o Worker recusa versões diferentes).
pub const SIGNALING_PROTOCOL: u32 = 1;

/// De quanto em quanto tempo mandar um ping para a sala. Sem tráfego, a conexão pode ser
/// fechada no meio do caminho (e às vezes o fechamento nem chega até aqui).
const PING_EVERY: std::time::Duration = std::time::Duration::from_secs(20);
/// Sem receber nada (nem o pong) por esse tempo, a conexão é dada como perdida.
const SILENCE_LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

/// Letras dos códigos de sala: fáceis de ditar (sem 0/O, 1/I/L). Igual ao `room-code.ts`.
pub const ROOM_CODE_ALPHABET: &str = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
pub const ROOM_CODE_LENGTH: usize = 6;

/// Conta do Discord de um membro: o id e o hash da foto (nunca uma URL; a interface monta o
/// endereço do CDN do Discord). Serve para mostrar a foto; o Worker não confere a conta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    #[serde(default)]
    pub avatar: Option<String>,
}

/// Efeito em volta da foto quando o membro fala, escolhido por ele (ADR 0015). `kind` fica em
/// texto: um efeito novo, de uma versão mais nova, não pode estragar a leitura do membro (a
/// interface desenha o anel padrão no lugar do que não conhece).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakingEffect {
    pub kind: String,
    /// Cor no formato `#rrggbb`.
    pub color: String,
}

impl SpeakingEffect {
    /// Efeitos que esta versão sabe desenhar (iguais aos do Worker).
    pub const KINDS: [&'static str; 4] = ["dots", "waves", "equalizer", "rays"];

    /// O efeito, se for um que conhecemos e a cor estiver no formato; senão `None` (o anel padrão).
    pub fn cleaned(kind: &str, color: &str) -> Option<Self> {
        let color = color.to_ascii_lowercase();
        let hex = color.strip_prefix('#')?;
        let valid = hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit());
        (valid && Self::KINDS.contains(&kind)).then(|| Self {
            kind: kind.to_owned(),
            color,
        })
    }
}

/// Quem pediu para entrar na sala (um amigo de alguém daqui, conferido pelo Worker).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequester {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub live: bool,
    /// Microfone silenciado (para todos). Ausente no Worker antigo.
    #[serde(default)]
    pub muted: bool,
    /// Ensurdecido (não ouve ninguém). Ausente no Worker antigo.
    #[serde(default)]
    pub deafened: bool,
    /// Conta do Discord vinculada. Ausente no Worker antigo.
    #[serde(default)]
    pub discord: Option<DiscordUser>,
    /// Conta conferida pela sessão do SquadView (só conferidos viram amigos).
    #[serde(default)]
    pub verified: bool,
    /// Quem este membro está assistindo (id). Ausente no Worker antigo e em apps até o 0.4.0.
    #[serde(default)]
    pub watching: Option<String>,
    /// Todas as lives que ele assiste (ADR 0019). Ausente no Worker antigo e em apps até o 0.8.0:
    /// aí vale só o `watching`.
    #[serde(default, rename = "watchingAll")]
    pub watching_all: Vec<String>,
    /// Efeito de fala escolhido (`None`: o anel padrão). Ausente no Worker antigo e em apps até o 0.5.0.
    #[serde(default)]
    pub effect: Option<SpeakingEffect>,
}

/// Vaga para voltar à sala com o mesmo id depois de uma queda (ADR 0022): o nosso id e o token
/// que veio no `welcome`.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Resume {
    pub id: String,
    pub token: String,
}

impl std::fmt::Debug for Resume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // O token não vai para o registro.
        f.debug_struct("Resume").field("id", &self.id).finish_non_exhaustive()
    }
}

/// Mensagens que mandamos.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Outgoing<'a> {
    Join {
        name: &'a str,
        protocol: u32,
        /// Nome da sala; só vale para quem cria (entra na sala vazia).
        #[serde(skip_serializing_if = "Option::is_none")]
        room: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        discord: Option<&'a DiscordUser>,
        /// Sessão do SquadView (ADR 0011): o Worker confere e o membro conta como conferido.
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<&'a str>,
        /// Voltando de uma queda: pede a mesma vaga (ADR 0022).
        #[serde(skip_serializing_if = "Option::is_none")]
        resume: Option<&'a Resume>,
    },
    Status {
        live: bool,
        muted: bool,
        deafened: bool,
        /// A primeira live que assistimos (para apps antigos, que só leem uma).
        watching: Option<&'a str>,
        /// Todas as lives que assistimos: quem transmite vê quantos e quem assistem.
        #[serde(rename = "watchingAll")]
        watching_all: &'a [String],
        /// O efeito de fala que escolhemos (`None`: o anel padrão).
        effect: Option<&'a SpeakingEffect>,
    },
    /// Renomeia a sala.
    Room {
        name: &'a str,
    },
    /// Troca o nosso apelido e, se vier `discord`, a conta (`Some(None)`: desvinculou).
    Rename {
        name: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        discord: Option<Option<&'a DiscordUser>>,
    },
    Signal {
        to: &'a str,
        payload: &'a Signal,
    },
    /// Entrou com o Discord já dentro da sala: confere a sessão.
    Verify {
        session: &'a str,
    },
    /// Resposta a um pedido de entrada.
    JoinAnswer {
        id: &'a str,
        accept: bool,
    },
    Leave,
}

/// Mensagens que o Worker manda.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Incoming {
    Welcome {
        you: String,
        members: Vec<Member>,
        /// Ausente no Worker antigo.
        #[serde(default)]
        room: Option<String>,
        /// Token para voltar com o mesmo id (ADR 0022). Ausente no Worker antigo.
        #[serde(default)]
        resume: Option<String>,
    },
    RoomUpdated {
        name: String,
    },
    MemberJoined {
        member: Member,
    },
    MemberUpdated {
        member: Member,
    },
    MemberLeft {
        id: String,
    },
    Signal {
        from: String,
        payload: serde_json::Value,
    },
    JoinRequest {
        id: String,
        from: JoinRequester,
    },
    JoinRequestClosed {
        id: String,
    },
    /// Credenciais do repassador TURN (ADR 0014). Só no Worker com o repassador configurado.
    Turn(TurnCredentials),
    Error {
        code: String,
    },
}

/// Credenciais temporárias do repassador TURN, como o Worker manda (formato `iceServers`).
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct TurnCredentials {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl std::fmt::Debug for TurnCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A senha não vai para o registro.
        f.debug_struct("TurnCredentials")
            .field("urls", &self.urls)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl TurnCredentials {
    /// O repassador por UDP, com o endereço resolvido (consulta o DNS: rodar fora do caminho quente).
    pub fn server(&self) -> Option<crate::TurnServer> {
        crate::TurnServer::from_urls(&self.urls, &self.username, &self.credential)
    }
}

/// O que acontece na sala.
#[derive(Debug, Clone, PartialEq)]
pub enum RoomEvent {
    Joined {
        you: String,
        members: Vec<Member>,
        /// Nome da sala (`None`: sem nome, mostra o código).
        room: Option<String>,
        /// Token para voltar com o mesmo `you` se a conexão cair (`None`: Worker antigo).
        resume: Option<String>,
    },
    /// Alguém renomeou a sala.
    RoomRenamed(String),
    MemberJoined(Member),
    MemberUpdated(Member),
    MemberLeft(String),
    Signal {
        from: String,
        signal: Signal,
    },
    /// Um amigo de alguém da sala quer entrar (qualquer membro aceita ou recusa).
    JoinRequest {
        id: String,
        from: JoinRequester,
    },
    /// O pedido foi respondido por alguém ou venceu.
    JoinRequestClosed(String),
    /// O repassador TURN desta sala (para quem não conecta direto).
    Turn(TurnCredentials),
    /// Erro vindo do Worker (ex.: `room-full`, `version-mismatch`).
    Error(String),
    /// A conexão com a sinalização caiu ou foi fechada. O texto diz o motivo, para o registro.
    Disconnected(String),
}

impl RoomEvent {
    fn from_incoming(msg: Incoming) -> Option<Self> {
        Some(match msg {
            Incoming::Welcome {
                you,
                members,
                room,
                resume,
            } => Self::Joined {
                you,
                members,
                room,
                resume,
            },
            Incoming::RoomUpdated { name } => Self::RoomRenamed(name),
            Incoming::MemberJoined { member } => Self::MemberJoined(member),
            Incoming::MemberUpdated { member } => Self::MemberUpdated(member),
            Incoming::MemberLeft { id } => Self::MemberLeft(id),
            // Sinal que não entendemos (versão mais nova do app do outro lado): ignora.
            Incoming::Signal { from, payload } => Self::Signal {
                from,
                signal: serde_json::from_value(payload).ok()?,
            },
            Incoming::JoinRequest { id, from } => Self::JoinRequest { id, from },
            Incoming::JoinRequestClosed { id } => Self::JoinRequestClosed(id),
            Incoming::Turn(credentials) => Self::Turn(credentials),
            Incoming::Error { code } => Self::Error(code),
        })
    }
}

/// Mensagem amigável para os erros do Worker.
pub fn error_message(code: &str) -> &'static str {
    match code {
        // Só de um Worker antigo: hoje a sala não tem limite de pessoas.
        "room-full" => "A sala está cheia.",
        "version-mismatch" => "Alguém na sala está com outra versão do SquadView. Atualizem o app.",
        "rate-limited" => "Muitas mensagens em pouco tempo. Tente entrar de novo.",
        "unknown-peer" => "Esse membro já saiu da sala.",
        _ => "A sala recusou a conexão.",
    }
}

/// Conexão com a sala. Para (sai da sala) ao ser descartada.
#[derive(Debug)]
pub struct RoomClient {
    outgoing: mpsc::UnboundedSender<Message>,
    /// Avisa (ou fecha) quando a tarefa que escreve na conexão termina: o `leave` já foi.
    flushed: std::sync::mpsc::Receiver<()>,
}

impl RoomClient {
    /// Entra na sala `code` em `base_url` (ex.: `wss://squadview-signaling.exemplo.workers.dev`).
    /// `room` dá nome à sala, se formos os primeiros a entrar; `resume` pede a vaga de antes de uma
    /// queda (ADR 0022). Os eventos chegam por `events`. Precisa rodar dentro de um runtime tokio.
    pub async fn join(
        base_url: &str,
        code: &str,
        name: &str,
        discord: Option<&DiscordUser>,
        session: Option<&str>,
        room: Option<&str>,
        resume: Option<&Resume>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<RoomEvent>), String> {
        if !is_valid_room_code(code) {
            return Err("Código de sala inválido.".into());
        }
        let url = format!("{}/rooms/{}", base_url.trim_end_matches('/'), code);
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| format!("não deu para alcançar o serviço de salas: {e}"))?;
        let (mut write, mut read) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (flushed_tx, flushed) = std::sync::mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<RoomEvent>();

        let join = serde_json::to_string(&Outgoing::Join {
            name,
            protocol: SIGNALING_PROTOCOL,
            room,
            discord,
            session,
            resume,
        })
        .expect("json");
        write
            .send(Message::Text(join.into()))
            .await
            .map_err(|e| e.to_string())?;

        tokio::spawn(async move {
            let mut ping = tokio::time::interval(PING_EVERY);
            ping.tick().await;
            loop {
                let msg = tokio::select! {
                    msg = out_rx.recv() => match msg {
                        Some(msg) => msg,
                        None => break,
                    },
                    _ = ping.tick() => Message::Ping(Vec::new().into()),
                };
                let leaving = matches!(&msg, Message::Close(_));
                if write.send(msg).await.is_err() || leaving {
                    break;
                }
            }
            let _ = write.close().await;
            let _ = flushed_tx.send(());
        });
        tokio::spawn(async move {
            // Qualquer coisa que chega (mensagem ou pong) prova que a conexão está viva.
            let reason = loop {
                let msg = match tokio::time::timeout(SILENCE_LIMIT, read.next()).await {
                    Ok(Some(Ok(msg))) => msg,
                    other => break disconnect_reason(other),
                };
                let text = match msg {
                    Message::Text(text) => text,
                    Message::Close(frame) => break close_reason(frame.as_ref()),
                    _ => continue,
                };
                let Ok(incoming) = serde_json::from_str::<Incoming>(&text) else {
                    continue;
                };
                if let Some(event) = RoomEvent::from_incoming(incoming)
                    && ev_tx.send(event).is_err()
                {
                    return;
                }
            };
            let _ = ev_tx.send(RoomEvent::Disconnected(reason));
        });
        Ok((
            Self {
                outgoing: out_tx,
                flushed,
            },
            ev_rx,
        ))
    }

    /// Avisa a sala que começamos ou paramos de transmitir.
    pub fn set_live(&self, live: bool) {
        self.set_status(live, false, false, &[], None);
    }

    /// Avisa a sala do nosso estado: ao vivo, microfone silenciado, ensurdecido, quem assistimos e
    /// o efeito de fala.
    pub fn set_status(
        &self,
        live: bool,
        muted: bool,
        deafened: bool,
        watching: &[String],
        effect: Option<&SpeakingEffect>,
    ) {
        self.send(&Outgoing::Status {
            live,
            muted,
            deafened,
            watching: watching.first().map(String::as_str),
            watching_all: watching,
            effect,
        });
    }

    /// Renomeia a sala (todos recebem [`RoomEvent::RoomRenamed`], inclusive nós).
    pub fn set_room_name(&self, name: &str) {
        self.send(&Outgoing::Room { name });
    }

    /// Troca o nosso apelido (os outros recebem [`RoomEvent::MemberUpdated`]).
    pub fn set_name(&self, name: &str) {
        self.send(&Outgoing::Rename { name, discord: None });
    }

    /// Troca o nome e a conta do Discord (vinculou ou desvinculou já na sala).
    pub fn set_profile(&self, name: &str, discord: Option<&DiscordUser>) {
        self.send(&Outgoing::Rename {
            name,
            discord: Some(discord),
        });
    }

    /// Confere a sessão do SquadView já dentro da sala (entrou com o Discord agora).
    pub fn verify(&self, session: &str) {
        self.send(&Outgoing::Verify { session });
    }

    /// Aceita ou recusa um pedido de entrada.
    pub fn answer_join(&self, id: &str, accept: bool) {
        self.send(&Outgoing::JoinAnswer { id, accept });
    }

    pub fn signal(&self, to: &str, signal: &Signal) {
        self.send(&Outgoing::Signal { to, payload: signal });
    }

    fn send(&self, msg: &Outgoing<'_>) {
        if let Ok(text) = serde_json::to_string(msg) {
            let _ = self.outgoing.send(Message::Text(text.into()));
        }
    }
}

impl RoomClient {
    /// Sai da sala e espera (até `wait`) o `leave` sair, para a sala tirar a gente na hora em vez
    /// de guardar a vaga (ADR 0022). Bloqueia: chamar fora das tarefas do runtime, antes de pará-lo
    /// (com o runtime parado, a conexão só fecha, sem `leave`).
    pub fn leave(&self, wait: std::time::Duration) {
        self.send(&Outgoing::Leave);
        let _ = self.outgoing.send(Message::Close(None));
        let _ = self.flushed.recv_timeout(wait);
    }
}

impl Drop for RoomClient {
    fn drop(&mut self) {
        self.send(&Outgoing::Leave);
        let _ = self.outgoing.send(Message::Close(None));
    }
}

/// Por que a leitura parou: silêncio, fim da conexão sem aviso ou erro.
fn disconnect_reason<T>(
    read: Result<Option<Result<T, tokio_tungstenite::tungstenite::Error>>, tokio::time::error::Elapsed>,
) -> String {
    match read {
        Err(_) => format!("sem resposta da sala por {} s", SILENCE_LIMIT.as_secs()),
        Ok(None) => "a conexão terminou sem aviso".into(),
        Ok(Some(Err(e))) => format!("erro na conexão: {e}"),
        Ok(Some(Ok(_))) => unreachable!("mensagem não é motivo de queda"),
    }
}

/// O motivo que a sala (ou a Cloudflare no caminho) deu ao fechar.
fn close_reason(frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> String {
    match frame {
        Some(f) if f.reason.is_empty() => format!("a sala fechou a conexão ({})", u16::from(f.code)),
        Some(f) => format!("a sala fechou a conexão ({}: {})", u16::from(f.code), f.reason),
        None => "a sala fechou a conexão".into(),
    }
}

/// Sorteia um código de sala novo. A sala passa a existir quando o primeiro membro entra.
pub fn new_room_code() -> String {
    let alphabet: Vec<char> = ROOM_CODE_ALPHABET.chars().collect();
    let mut seed = [0u8; ROOM_CODE_LENGTH];
    // Gerador aleatório criptográfico do sistema (o mesmo que o TLS usa).
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut seed)
        .expect("gerador aleatório do sistema");
    seed.iter().map(|b| alphabet[*b as usize % alphabet.len()]).collect()
}

/// Normaliza o que o usuário digitou (minúsculas, espaços, traço) e confere o formato.
pub fn normalize_room_code(input: &str) -> Option<String> {
    let code: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    is_valid_room_code(&code).then_some(code)
}

pub fn is_valid_room_code(code: &str) -> bool {
    code.chars().count() == ROOM_CODE_LENGTH && code.chars().all(|c| ROOM_CODE_ALPHABET.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mensagens_seguem_o_contrato_do_worker() {
        let join = serde_json::to_string(&Outgoing::Join {
            name: "Ana",
            protocol: 1,
            room: None,
            discord: None,
            session: None,
            resume: None,
        })
        .unwrap();
        // Sem nome de sala, igual ao app 0.1.0 (o Worker antigo também aceita).
        assert_eq!(join, r#"{"type":"join","name":"Ana","protocol":1}"#);
        let creating = serde_json::to_string(&Outgoing::Join {
            name: "Ana",
            protocol: 1,
            room: Some("Galera"),
            discord: None,
            session: None,
            resume: None,
        })
        .unwrap();
        assert_eq!(creating, r#"{"type":"join","name":"Ana","protocol":1,"room":"Galera"}"#);
        let back = Resume {
            id: "aa".into(),
            token: "ff".into(),
        };
        let resuming = serde_json::to_string(&Outgoing::Join {
            name: "Ana",
            protocol: 1,
            room: None,
            discord: None,
            session: None,
            resume: Some(&back),
        })
        .unwrap();
        assert_eq!(
            resuming,
            r#"{"type":"join","name":"Ana","protocol":1,"resume":{"id":"aa","token":"ff"}}"#
        );
        assert!(!format!("{back:?}").contains("ff"), "o token não vai para o registro");
        assert_eq!(
            serde_json::to_string(&Outgoing::Room { name: "Squad" }).unwrap(),
            r#"{"type":"room","name":"Squad"}"#
        );
        assert_eq!(
            serde_json::to_string(&Outgoing::Rename {
                name: "K.A",
                discord: None
            })
            .unwrap(),
            r#"{"type":"rename","name":"K.A"}"#
        );
        let conta = DiscordUser {
            id: "123456789012345678".into(),
            avatar: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        };
        assert_eq!(
            serde_json::to_string(&Outgoing::Rename {
                name: "K.A",
                discord: Some(Some(&conta))
            })
            .unwrap(),
            r#"{"type":"rename","name":"K.A","discord":{"id":"123456789012345678","avatar":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#
        );
        // Desvinculou: vai `null`, para a sala tirar a foto.
        assert_eq!(
            serde_json::to_string(&Outgoing::Rename {
                name: "K.A",
                discord: Some(None)
            })
            .unwrap(),
            r#"{"type":"rename","name":"K.A","discord":null}"#
        );
        let status = serde_json::to_string(&Outgoing::Status {
            live: true,
            muted: false,
            deafened: true,
            watching: None,
            watching_all: &[],
            effect: None,
        })
        .unwrap();
        assert_eq!(
            status,
            r#"{"type":"status","live":true,"muted":false,"deafened":true,"watching":null,"watchingAll":[],"effect":null}"#
        );
        let watching = serde_json::to_string(&Outgoing::Status {
            live: false,
            muted: false,
            deafened: false,
            watching: Some("0123456789abcdef"),
            watching_all: &["0123456789abcdef".to_owned(), "fedcba9876543210".to_owned()],
            effect: None,
        })
        .unwrap();
        assert!(watching.contains(r#""watching":"0123456789abcdef""#), "{watching}");
        assert!(
            watching.contains(r#""watchingAll":["0123456789abcdef","fedcba9876543210"]"#),
            "{watching}"
        );
        let ondas = SpeakingEffect::cleaned("waves", "#FF8800").unwrap();
        let effect = serde_json::to_string(&Outgoing::Status {
            live: false,
            muted: false,
            deafened: false,
            watching: None,
            watching_all: &[],
            effect: Some(&ondas),
        })
        .unwrap();
        assert!(
            effect.ends_with(r##""effect":{"kind":"waves","color":"#ff8800"}}"##),
            "{effect}"
        );
        let sig = Signal::WatchDeny {
            reason: "não está ao vivo".into(),
        };
        let s = serde_json::to_string(&Outgoing::Signal {
            to: "0123456789abcdef",
            payload: &sig,
        })
        .unwrap();
        assert_eq!(
            s,
            r#"{"type":"signal","to":"0123456789abcdef","payload":{"kind":"watch-deny","reason":"não está ao vivo"}}"#
        );
        assert_eq!(serde_json::to_string(&Outgoing::Leave).unwrap(), r#"{"type":"leave"}"#);
    }

    #[test]
    fn efeito_de_fala_so_aceita_os_conhecidos_com_cor_valida() {
        assert_eq!(
            SpeakingEffect::cleaned("rays", "#A78BFA"),
            Some(SpeakingEffect {
                kind: "rays".into(),
                color: "#a78bfa".into()
            })
        );
        assert_eq!(SpeakingEffect::cleaned("fogo", "#a78bfa"), None);
        assert_eq!(SpeakingEffect::cleaned("dots", "a78bfa"), None);
        assert_eq!(SpeakingEffect::cleaned("dots", "#a78bfz"), None);
        assert_eq!(SpeakingEffect::cleaned("dots", "#a78bfa00"), None);
    }

    #[test]
    fn membro_com_efeito_desconhecido_continua_legivel() {
        // Um efeito de uma versão mais nova não pode fazer o membro sumir da lista.
        let m: Member = serde_json::from_str(
            r##"{"id":"bb","name":"Beto","live":false,"effect":{"kind":"fogo","color":"#ff0000"}}"##,
        )
        .unwrap();
        assert_eq!(m.effect.map(|e| e.kind).as_deref(), Some("fogo"));
    }

    #[test]
    fn le_os_eventos_do_worker() {
        let parse = |s: &str| RoomEvent::from_incoming(serde_json::from_str(s).unwrap());
        assert_eq!(
            parse(r#"{"type":"welcome","you":"aa","members":[{"id":"bb","name":"Beto","live":true}]}"#),
            Some(RoomEvent::Joined {
                you: "aa".into(),
                members: vec![Member {
                    id: "bb".into(),
                    name: "Beto".into(),
                    live: true,
                    muted: false,
                    deafened: false,
                    discord: None,
                    verified: false,
                    watching: None,
                    watching_all: Vec::new(),
                    effect: None,
                }],
                room: None,
                resume: None,
            })
        );
        assert_eq!(
            parse(r#"{"type":"welcome","you":"aa","members":[],"room":"Galera","resume":"ff"}"#),
            Some(RoomEvent::Joined {
                you: "aa".into(),
                members: vec![],
                room: Some("Galera".into()),
                resume: Some("ff".into()),
            })
        );
        assert_eq!(
            parse(r#"{"type":"room-updated","name":"Squad"}"#),
            Some(RoomEvent::RoomRenamed("Squad".into()))
        );
        assert_eq!(
            parse(r#"{"type":"join-request","id":"cc","from":{"id":"1","name":"Beto","avatar":null}}"#),
            Some(RoomEvent::JoinRequest {
                id: "cc".into(),
                from: JoinRequester {
                    id: "1".into(),
                    name: "Beto".into(),
                    avatar: None
                }
            })
        );
        assert_eq!(
            parse(r#"{"type":"join-request-closed","id":"cc"}"#),
            Some(RoomEvent::JoinRequestClosed("cc".into()))
        );
        let turn = parse(
            r#"{"type":"turn","urls":["turn:turn.cloudflare.com:3478?transport=udp"],"username":"u","credential":"segredo"}"#,
        );
        let Some(RoomEvent::Turn(credentials)) = turn else {
            panic!("esperava as credenciais do repassador: {turn:?}")
        };
        assert_eq!(credentials.username, "u");
        // A senha não aparece no registro.
        assert!(!format!("{credentials:?}").contains("segredo"));
        assert_eq!(
            serde_json::to_string(&Outgoing::Verify { session: "abc.def" }).unwrap(),
            r#"{"type":"verify","session":"abc.def"}"#
        );
        assert_eq!(
            serde_json::to_string(&Outgoing::JoinAnswer { id: "cc", accept: true }).unwrap(),
            r#"{"type":"join-answer","id":"cc","accept":true}"#
        );
        assert_eq!(
            parse(r#"{"type":"member-left","id":"bb"}"#),
            Some(RoomEvent::MemberLeft("bb".into()))
        );
        assert_eq!(
            parse(r#"{"type":"error","code":"room-full"}"#),
            Some(RoomEvent::Error("room-full".into()))
        );
        assert_eq!(
            parse(r#"{"type":"signal","from":"bb","payload":{"kind":"watch-deny","reason":"x"}}"#),
            Some(RoomEvent::Signal {
                from: "bb".into(),
                signal: Signal::WatchDeny { reason: "x".into() }
            })
        );
        // Sinal de uma versão mais nova: ignorado sem derrubar nada.
        assert_eq!(
            parse(r#"{"type":"signal","from":"bb","payload":{"kind":"novidade"}}"#),
            None
        );
    }

    #[test]
    fn codigos_de_sala() {
        let code = new_room_code();
        assert!(is_valid_room_code(&code), "{code}");
        assert_ne!(new_room_code(), new_room_code());
        assert_eq!(normalize_room_code(" b8r-j2q "), Some("B8RJ2Q".into()));
        assert_eq!(normalize_room_code("B0RJ2Q"), None, "0 é ambíguo");
        assert_eq!(normalize_room_code("ABC"), None);
    }

    #[test]
    fn erros_tem_mensagem_para_o_usuario() {
        assert!(error_message("room-full").contains("cheia"));
        assert!(error_message("qualquer").contains("recusou"));
    }
}
