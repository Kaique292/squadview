//! Ajudantes dos testes do app: um app de mentira (MockRuntime do Tauri) com os mesmos estados do
//! app de verdade e uma pasta de dados própria, apagada no fim. Os testes nunca mexem nos dados
//! do SquadView instalado (sessão, perfil, registros).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, AppHandle, Listener, Manager};

pub struct TestApp {
    pub app: App<MockRuntime>,
    dirs: Vec<PathBuf>,
}

impl TestApp {
    pub fn handle(&self) -> &AppHandle<MockRuntime> {
        self.app.handle()
    }

    /// Recebe o conteúdo (JSON) de cada evento `name` emitido para a interface.
    pub fn events(&self, name: &str) -> Events {
        let (tx, rx) = mpsc::channel();
        self.app.listen_any(name.to_owned(), move |e| {
            let _ = tx.send(e.payload().to_owned());
        });
        Events(rx)
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        for dir in &self.dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

pub struct Events(mpsc::Receiver<String>);

impl Events {
    /// Espera um evento cujo JSON satisfaça `pred` (falha depois de 10 s).
    pub fn wait(&self, what: &str, pred: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let payload = self
                .0
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("não chegou: {what}"));
            let value: serde_json::Value = serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
            if pred(&value) {
                return value;
            }
        }
    }
}

fn unique() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}

/// Identificador só deste app de teste. O relógio do Windows anda de 100 em 100 ns, então dois
/// testes em paralelo podiam pegar o mesmo horário, dividir a mesma pasta de dados e um apagar a
/// do outro no `Drop`. O contador separa os apps do mesmo processo; o id do processo e o horário
/// separam execuções diferentes (outras worktrees rodando os testes ao mesmo tempo).
fn test_identifier() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(
        "io.github.kaique292.squadview.testes.{}.{n}.{}",
        std::process::id(),
        unique()
    )
}

/// App de mentira com os estados de verdade, uma janela "main" e pastas de dados só dele.
pub fn test_app() -> TestApp {
    let mut context = mock_context(noop_assets());
    context.config_mut().identifier = test_identifier();
    let app = crate::manage_states(mock_builder(), crate::presence::Presence::start())
        .build(context)
        .expect("app de teste");
    tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("janela main");
    let dirs = [app.path().app_data_dir(), app.path().app_local_data_dir()]
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    TestApp { app, dirs }
}

#[test]
fn apps_de_teste_criados_juntos_tem_pastas_diferentes() {
    // Todas as threads largam juntas, como os testes em paralelo do `cargo test`.
    let start = std::sync::Barrier::new(64);
    let ids: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..64)
            .map(|_| {
                s.spawn(|| {
                    start.wait();
                    test_identifier()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let unicos: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unicos.len(), ids.len(), "{ids:?}");
}

/// Worker de sinalização local e o segredo das sessões dele (`scripts/with-worker.mjs`, usado
/// pelo `yarn test:rs`). `None`: o teste que precisa dele é pulado.
pub fn worker() -> Option<(String, String)> {
    let url = std::env::var("SQUADVIEW_SIGNALING_URL").ok();
    let secret = std::env::var("SQUADVIEW_TEST_SESSION_SECRET").ok();
    if url.is_none() || secret.is_none() {
        eprintln!("pulado: rode pelo `yarn test:rs` (sobe o Worker local)");
    }
    Some((url?, secret?))
}

/// Sessão assinada como a do Worker (`{id, exp}` em base64url, ponto, HMAC-SHA256).
pub fn session(secret: &str, id: &str) -> String {
    let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 3_600_000;
    let payload = URL_SAFE_NO_PAD.encode(serde_json::json!({ "id": id, "exp": exp as u64 }).to_string());
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let sig = ring::hmac::sign(&key, payload.as_bytes());
    format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

/// Id do Discord novo (o Worker local guarda o estado entre execuções). Com o contador, como em
/// [`test_identifier`]: dois testes em paralelo no mesmo horário do relógio não dividem a conta.
pub fn fresh_discord_id() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed) % 1000;
    format!("3{:014}{n:03}", unique() % 100_000_000_000_000)
}
