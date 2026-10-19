//! Login com o Discord (OAuth2 de cliente público, com PKCE): o app nunca vê senha nem guarda
//! segredo. O navegador abre a página de autorização do Discord, que devolve um código para um
//! servidorzinho local em `127.0.0.1`; o app troca o código por um token, lê o perfil público
//! (escopo `identify`) e descarta o token. Só o perfil fica salvo, em `discord.json`.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
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
pub async fn discord_login(app: AppHandle) -> Result<DiscordProfile, String> {
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

    save(&app, Some(&profile))?;
    Ok(profile)
}

/// Perfil salvo do último login, se houver.
#[tauri::command]
pub fn discord_profile(app: AppHandle) -> Option<DiscordProfile> {
    let data = std::fs::read(profile_path(&app).ok()?).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Esquece o perfil.
#[tauri::command]
pub fn discord_logout(app: AppHandle) -> Result<(), String> {
    save(&app, None)
}

fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("discord.json"))
}

fn save(app: &AppHandle, profile: Option<&DiscordProfile>) -> Result<(), String> {
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
