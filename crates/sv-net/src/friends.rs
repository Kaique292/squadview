//! Cliente do registro de amigos e presença (ver `docs/adr/0011-amigos-e-presenca.md` e
//! `services/signaling/PROTOCOL.md`, "Amigos e presença").
//!
//! A sessão vai no cabeçalho `Authorization` do WebSocket, nunca na URL. O que o registro manda
//! chega como JSON cru: quem mostra (a interface) é quem interpreta.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

/// De quanto em quanto tempo mandar um ping (como na sala).
const PING_EVERY: std::time::Duration = std::time::Duration::from_secs(20);
/// Sem receber nada por esse tempo, a conexão é dada como perdida.
const SILENCE_LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

/// Em que sala estamos (o código fica no servidor; os amigos só veem o nome).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomStatus {
    pub code: String,
    pub name: Option<String>,
}

/// O que o app pode pedir ao registro. Espelha `UserClientMessage` do Worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FriendAction {
    Profile {
        name: String,
        avatar: Option<String>,
    },
    Status {
        room: Option<RoomStatus>,
        live: bool,
    },
    FriendRequest {
        id: String,
    },
    FriendAccept {
        id: String,
    },
    FriendDecline {
        id: String,
    },
    FriendCancel {
        id: String,
    },
    Unfriend {
        id: String,
    },
    SetDirect {
        id: String,
        direct: bool,
    },
    Join {
        id: String,
    },
    Invite {
        id: String,
    },
    /// Cria um servidor (ADR 0016): quem cria é o dono.
    ServerCreate {
        name: String,
    },
    /// Entra num servidor pelo convite (`<id>.<segredo>`).
    ServerJoin {
        invite: String,
    },
    /// Um pedido a um servidor nosso.
    ServerAction {
        server: String,
        op: ServerOp,
    },
    /// Sinal da conversa direta para o amigo `to` (ADR 0017); o conteúdo nunca vai por aqui.
    DmSignal {
        to: String,
        payload: serde_json::Value,
    },
    /// Credenciais do repassador TURN para a conversa direta.
    TurnRequest,
}

/// Pedido a um servidor. Espelha `ServerOp` do Worker (que confere de novo quem pode o quê).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ServerOp {
    /// Nome e/ou ícone (data URL de 96 px; `Some(None)` tira o ícone).
    Update {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
        icon: Option<Option<String>>,
    },
    Delete,
    Leave,
    RoomCreate {
        name: String,
    },
    RoomRename {
        room: String,
        name: String,
    },
    RoomDelete {
        room: String,
    },
    Invite,
    Kick {
        member: String,
    },
    Role {
        member: String,
        role: ServerRole,
    },
}

/// Campo presente (mesmo `null`) vira `Some`; ausente fica no `default` (`None`): assim "tirar o
/// ícone" (`null`) não se confunde com "não mexer no ícone" (sem o campo).
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Papel que o dono dá a um membro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerRole {
    Admin,
    Member,
}

/// O que chega do registro.
#[derive(Debug, Clone, PartialEq)]
pub enum FriendsEvent {
    /// Mensagem do registro (JSON), para a interface.
    Message(serde_json::Value),
    /// Sessão renovada: guardar no lugar da antiga.
    Session(String),
    /// A conexão caiu.
    Disconnected,
}

/// Por que não deu para conectar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendsConnectError {
    /// A sessão não vale mais (ou o Worker não tem o segredo): é preciso entrar de novo.
    Unauthorized,
    /// Rede ou serviço fora do ar: tentar de novo depois.
    Unavailable(String),
}

#[derive(Debug)]
pub struct FriendsClient {
    outgoing: mpsc::UnboundedSender<Message>,
}

impl FriendsClient {
    /// Conecta ao registro em `base_url` (o mesmo endereço da sinalização, `ws://` ou `wss://`).
    pub async fn connect(
        base_url: &str,
        session: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<FriendsEvent>), FriendsConnectError> {
        let url = format!("{}/me", base_url.trim_end_matches('/'));
        let mut request = url
            .into_client_request()
            .map_err(|e| FriendsConnectError::Unavailable(e.to_string()))?;
        let auth =
            HeaderValue::from_str(&format!("Bearer {session}")).map_err(|_| FriendsConnectError::Unauthorized)?;
        request.headers_mut().insert("Authorization", auth);
        let (ws, _) = tokio_tungstenite::connect_async(request).await.map_err(|e| match &e {
            tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == 401 || r.status() == 503 => {
                FriendsConnectError::Unauthorized
            }
            _ => FriendsConnectError::Unavailable(e.to_string()),
        })?;
        let (mut write, mut read) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<FriendsEvent>();

        tokio::spawn(async move {
            let mut ping = tokio::time::interval(PING_EVERY);
            ping.tick().await;
            loop {
                let msg = tokio::select! {
                    m = out_rx.recv() => match m {
                        Some(m) => m,
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
        });
        tokio::spawn(async move {
            while let Ok(Some(Ok(msg))) = tokio::time::timeout(SILENCE_LIMIT, read.next()).await {
                let Message::Text(text) = msg else { continue };
                let Some(event) = parse_event(&text) else { continue };
                if ev_tx.send(event).is_err() {
                    return;
                }
            }
            let _ = ev_tx.send(FriendsEvent::Disconnected);
        });
        Ok((Self { outgoing: out_tx }, ev_rx))
    }

    pub fn send(&self, action: &FriendAction) {
        if let Ok(text) = serde_json::to_string(action) {
            let _ = self.outgoing.send(Message::Text(text.into()));
        }
    }
}

impl Drop for FriendsClient {
    fn drop(&mut self) {
        let _ = self.outgoing.send(Message::Close(None));
    }
}

fn parse_event(text: &str) -> Option<FriendsEvent> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("type")?.as_str()? == "session" {
        return Some(FriendsEvent::Session(value.get("session")?.as_str()?.to_owned()));
    }
    Some(FriendsEvent::Message(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acoes_seguem_o_contrato_do_worker() {
        let status = FriendAction::Status {
            room: Some(RoomStatus {
                code: "B8RJ2Q".into(),
                name: Some("Galera".into()),
            }),
            live: true,
        };
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"type":"status","room":{"code":"B8RJ2Q","name":"Galera"},"live":true}"#
        );
        assert_eq!(
            serde_json::to_string(&FriendAction::SetDirect {
                id: "1".into(),
                direct: true
            })
            .unwrap(),
            r#"{"type":"set-direct","id":"1","direct":true}"#
        );
        // A interface manda no mesmo formato e o Rust confere antes de repassar.
        let from_ui: FriendAction = serde_json::from_str(r#"{"type":"friend-request","id":"2"}"#).unwrap();
        assert_eq!(from_ui, FriendAction::FriendRequest { id: "2".into() });
        assert!(serde_json::from_str::<FriendAction>(r#"{"type":"outra"}"#).is_err());
    }

    #[test]
    fn pedidos_de_servidor_seguem_o_contrato_do_worker() {
        let json = |a: &FriendAction| serde_json::to_string(a).unwrap();
        let action = |op| FriendAction::ServerAction {
            server: "0123456789abcdef".into(),
            op,
        };
        assert_eq!(
            json(&FriendAction::ServerCreate { name: "Turma".into() }),
            r#"{"type":"server-create","name":"Turma"}"#
        );
        assert_eq!(
            json(&action(ServerOp::RoomCreate { name: "Jogos".into() })),
            r#"{"type":"server-action","server":"0123456789abcdef","op":{"kind":"room-create","name":"Jogos"}}"#
        );
        assert_eq!(
            json(&action(ServerOp::Role {
                member: "1".into(),
                role: ServerRole::Admin
            })),
            r#"{"type":"server-action","server":"0123456789abcdef","op":{"kind":"role","member":"1","role":"admin"}}"#
        );
        // Ícone: ausente não mexe, `null` tira, texto troca.
        let update = |s: &str| serde_json::from_str::<ServerOp>(s).unwrap();
        assert_eq!(
            update(r#"{"kind":"update","name":"X"}"#),
            ServerOp::Update {
                name: Some("X".into()),
                icon: None
            }
        );
        assert_eq!(
            update(r#"{"kind":"update","icon":null}"#),
            ServerOp::Update {
                name: None,
                icon: Some(None)
            }
        );
        let tirar = ServerOp::Update {
            name: None,
            icon: Some(None),
        };
        assert_eq!(
            serde_json::to_string(&tirar).unwrap(),
            r#"{"kind":"update","icon":null}"#
        );
        assert_eq!(
            json(&FriendAction::DmSignal {
                to: "1".into(),
                payload: serde_json::json!({"kind": "dm-offer"})
            }),
            r#"{"type":"dm-signal","to":"1","payload":{"kind":"dm-offer"}}"#
        );
        assert_eq!(json(&FriendAction::TurnRequest), r#"{"type":"turn-request"}"#);
        // Papel de dono não existe para dar a ninguém.
        assert!(serde_json::from_str::<ServerOp>(r#"{"kind":"role","member":"1","role":"owner"}"#).is_err());
    }

    #[test]
    fn separa_a_sessao_do_resto() {
        assert_eq!(
            parse_event(r#"{"type":"session","session":"abc"}"#),
            Some(FriendsEvent::Session("abc".into()))
        );
        assert!(matches!(
            parse_event(r#"{"type":"state","friends":[]}"#),
            Some(FriendsEvent::Message(_))
        ));
        assert_eq!(parse_event("não é json"), None);
    }
}
