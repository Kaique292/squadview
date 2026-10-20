//! Login com o Discord (OAuth2 de cliente público, com PKCE): o app nunca vê senha nem guarda
//! segredo. O navegador abre a página de autorização do Discord, que devolve um código para um
//! servidorzinho local em `127.0.0.1`; o app troca o código por um token, lê o perfil público
//! (escopo `identify`), troca o token por uma sessão do SquadView no Worker (amigos, ADR 0011)
//! e descarta o token. Ficam salvos o perfil, em `discord.json`, e a sessão, em `session.json`.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// ID público do app no Discord Developer Portal (não é segredo).
const CLIENT_ID: &str = "1561766901233750108";
const PORT: u16 = 53134;
const REDIRECT_URI: &str = "http://127.0.0.1:53134/callback";
const API: &str = "https://discord.com/api/v10";
/// Tempo para a pessoa autorizar no navegador.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
/// Maior pedido HTTP aceito no servidor local.
const MAX_REQUEST: usize = 8 * 1024;

/// O que o app guarda do perfil do Discord.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiscordProfile {
    pub id: String,
    pub username: String,
    #[serde(alias = "global_name")]
    pub global_name: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// Resultado do redirecionamento do Discord para o servidor local.
#[derive(Debug, PartialEq)]
enum Callback {
    Code {
        code: String,
        state: String,
    },
    Denied,
    /// Outro caminho (ex.: `/favicon.ico`): ignora e continua esperando.
    Other,
}

/// Faz o login e devolve o perfil (já salvo).
#[tauri::command]
pub async fn discord_login<R: Runtime>(app: AppHandle<R>) -> Result<DiscordProfile, String> {
    let verifier = random_token(32)?;
    let state = random_token(16)?;

    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .await
        .map_err(|_| format!("A porta {PORT} está ocupada. Feche outros logins do SquadView e tente de novo."))?;

    app.opener()
        .open_url(authorize_url(&challenge(&verifier), &state), None::<&str>)
        .map_err(|e| format!("Não deu para abrir o navegador: {e}"))?;

    let code = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| "O login demorou demais. Tente de novo.".to_string())??;
    drop(listener);

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let token: TokenResponse = http
        .post(format!("{API}/oauth2/token"))
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("O Discord recusou o login: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let profile: DiscordProfile = http
        .get(format!("{API}/users/@me"))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Não deu para ler seu perfil do Discord: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Sessão do SquadView para os amigos. Se o Worker não aceitar, o login segue sem amigos.
    let session = crate::friends::exchange(&http, &token.access_token).await;
    drop(token);
    save(&app, Some(&profile))?;
    crate::friends::save_session(&app, session.as_deref());
    crate::friends::start(&app);
    // Já numa sala: a sala confere a conta agora, sem precisar sair e entrar.
    if let Some(session) = session.as_deref() {
        crate::room::verify_in_room(&app, session);
    }
    Ok(profile)
}

/// Perfil salvo do último login, se houver.
#[tauri::command]
pub fn discord_profile<R: Runtime>(app: AppHandle<R>) -> Option<DiscordProfile> {
    let data = std::fs::read(profile_path(&app).ok()?).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Esquece o perfil.
#[tauri::command]
pub fn discord_logout<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::friends::stop(&app);
    crate::friends::save_session(&app, None);
    save(&app, None)
}

fn profile_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("discord.json"))
}

fn save<R: Runtime>(app: &AppHandle<R>, profile: Option<&DiscordProfile>) -> Result<(), String> {
    let path = profile_path(app)?;
    match profile {
        Some(p) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let json = serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| e.to_string())
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        },
    }
}

/// Espera o navegador voltar com o código; responde uma página curta para a pessoa voltar ao app.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    loop {
        let (mut socket, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; MAX_REQUEST];
        let mut len = 0;
        // Só a primeira linha interessa; lê até ela chegar (ou o limite).
        while len < buf.len() && !buf[..len].contains(&b'\n') {
            match socket.read(&mut buf[len..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => len += n,
            }
        }
        let request = String::from_utf8_lossy(&buf[..len]);
        let result = parse_callback(request.lines().next().unwrap_or(""));
        let (status, message) = match &result {
            Callback::Code { state: s, .. } if s == state => ("200 OK", "Pronto! Pode voltar para o SquadView."),
            Callback::Code { .. } => (
                "400 Bad Request",
                "Este login não veio do SquadView. Tente de novo pelo app.",
            ),
            Callback::Denied => ("200 OK", "Login cancelado. Pode fechar esta aba."),
            Callback::Other => ("404 Not Found", ""),
        };
        let _ = socket.write_all(page(status, message).as_bytes()).await;
        let _ = socket.shutdown().await;
        match result {
            Callback::Code { code, state: s } if s == state => return Ok(code),
            Callback::Denied => return Err("Você cancelou o login no Discord.".into()),
            _ => continue,
        }
    }
}

fn page(status: &str, message: &str) -> String {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>SquadView</title>\
         <body style=\"background:#1e1f22;color:#f2f3f5;font:16px system-ui;\
         display:grid;place-items:center;height:100vh;margin:0\"><p>{message}</p>"
    );
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Lê `GET /callback?code=...&state=... HTTP/1.1`.
fn parse_callback(request_line: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Other;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Other;
    };
    if url.path() != "/callback" {
        return Callback::Other;
    }
    let get = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
    if get("error").is_some() {
        return Callback::Denied;
    }
    match (get("code"), get("state")) {
        (Some(code), Some(state)) => Callback::Code { code, state },
        _ => Callback::Other,
    }
}

fn authorize_url(challenge: &str, state: &str) -> String {
    let mut url = reqwest::Url::parse("https://discord.com/oauth2/authorize").expect("URL fixa");
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", "identify")
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

/// `code_challenge` do PKCE (S256): SHA-256 do verificador, em base64 para URL.
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_app;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Manda um pedido HTTP cru para o servidor local e devolve a resposta.
    async fn get(addr: std::net::SocketAddr, line: &str) -> String {
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(
                format!(
                    "{line}
Host: 127.0.0.1

"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).await.unwrap();
        reply
    }

    #[test]
    fn servidor_local_espera_o_retorno_certo_do_discord() {
        tauri::async_runtime::block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let addr = listener.local_addr().unwrap();
            let browser = tokio::spawn(async move {
                // Outros caminhos e um `state` que não é o nosso são ignorados.
                let other = get(addr, "GET /favicon.ico HTTP/1.1").await;
                let forged = get(addr, "GET /callback?code=x&state=outro HTTP/1.1").await;
                let ok = get(addr, "GET /callback?code=abc&state=meu HTTP/1.1").await;
                (other, forged, ok)
            });
            let code = wait_for_code(&listener, "meu").await.unwrap();
            assert_eq!(code, "abc");
            let (other, forged, ok) = browser.await.unwrap();
            assert!(other.starts_with("HTTP/1.1 404"), "{other}");
            assert!(
                forged.starts_with("HTTP/1.1 400") && forged.contains("não veio do SquadView"),
                "{forged}"
            );
            assert!(
                ok.starts_with("HTTP/1.1 200") && ok.contains("Pode voltar para o SquadView"),
                "{ok}"
            );
        });
    }

    #[test]
    fn servidor_local_avisa_quando_a_pessoa_cancela() {
        tauri::async_runtime::block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let addr = listener.local_addr().unwrap();
            let browser = tokio::spawn(async move { get(addr, "GET /callback?error=access_denied HTTP/1.1").await });
            let err = wait_for_code(&listener, "meu").await.unwrap_err();
            assert!(err.contains("cancelou"), "{err}");
            assert!(browser.await.unwrap().contains("Login cancelado"));
        });
    }

    #[test]
    fn guarda_le_e_esquece_o_perfil() {
        let t = test_app();
        assert_eq!(discord_profile(t.handle().clone()), None);
        let profile = DiscordProfile {
            id: "123456789012345678".into(),
            username: "kaique".into(),
            global_name: Some("K.A".into()),
            avatar: None,
        };
        save(t.handle(), Some(&profile)).unwrap();
        assert_eq!(discord_profile(t.handle().clone()), Some(profile));
        crate::friends::save_session(t.handle(), Some("abc.def"));
        // Sair esquece o perfil e a sessão dos amigos.
        discord_logout(t.handle().clone()).unwrap();
        assert_eq!(discord_profile(t.handle().clone()), None);
        assert_eq!(crate::friends::session(t.handle()), None);
        // Sair de novo (sem perfil) não é erro.
        discord_logout(t.handle().clone()).unwrap();
    }

    #[test]
    fn desafio_pkce_e_sha256_em_base64_para_url() {
        // Valor conferido à parte (hashlib + base64.urlsafe_b64encode do Python, sem o "=").
        assert_eq!(
            challenge("dBjftJeZ4CVP-mJ92K9qkhCcwP4L9aYrQwPVj2R0YlM"),
            "QlZ6Ifop7joYpXxDN5HlwrPHxARyhVKhJYTdwjSEd-A"
        );
    }

    #[test]
    fn le_o_codigo_do_retorno() {
        assert_eq!(
            parse_callback("GET /callback?code=abc123&state=xyz HTTP/1.1"),
            Callback::Code {
                code: "abc123".into(),
                state: "xyz".into()
            }
        );
    }

    #[test]
    fn reconhece_login_cancelado() {
        assert_eq!(
            parse_callback("GET /callback?error=access_denied&state=xyz HTTP/1.1"),
            Callback::Denied
        );
    }

    #[test]
    fn ignora_outros_caminhos_e_pedidos_estranhos() {
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), Callback::Other);
        assert_eq!(
            parse_callback("POST /callback?code=a&state=b HTTP/1.1"),
            Callback::Other
        );
        assert_eq!(parse_callback("GET /callback?code=a HTTP/1.1"), Callback::Other);
        assert_eq!(parse_callback(""), Callback::Other);
    }

    #[test]
    fn url_de_autorizacao_pede_so_o_perfil_com_pkce() {
        let url = reqwest::Url::parse(&authorize_url("desafio", "estado")).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["scope"], "identify");
        assert_eq!(q["redirect_uri"], REDIRECT_URI);
        assert_eq!(q["code_challenge"], "desafio");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "estado");
    }

    #[test]
    fn perfil_do_discord_vem_em_snake_case() {
        let p: DiscordProfile = serde_json::from_str(
            r#"{"id":"1","username":"kaique","global_name":"K.A","avatar":"abc","discriminator":"0"}"#,
        )
        .unwrap();
        assert_eq!(p.global_name.as_deref(), Some("K.A"));
    }

    #[test]
    fn tokens_aleatorios_sao_diferentes() {
        assert_ne!(random_token(16).unwrap(), random_token(16).unwrap());
    }
}
