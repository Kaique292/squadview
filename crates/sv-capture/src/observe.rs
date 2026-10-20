//! Observa as janelas para o [`GameFollower`]: a do jogo, as da família dele e a do primeiro
//! plano. Só consultas ao Windows (nada entra no processo do jogo). Roda no vigia da prévia,
//! a cada 500 ms, nunca no caminho do frame.

use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, WS_EX_TOOLWINDOW,
};

use crate::follow::{GameFollower, Observation, Rect, WindowFacts};
use crate::sources::{WindowInfo, exe_name, process_path, top_level_windows, window_info};
use crate::{CaptureSource, Source};

/// Por quantas checagens o caminho do executável de um processo fica guardado (10 s no vigia).
/// O Windows pode reaproveitar o número de um processo que fechou; guardar por pouco tempo limita
/// o engano a uma janela que, além disso, teria que ir para a frente.
const PATH_CACHE_CHECKS: u32 = 20;

/// Observa as janelas a cada checagem do vigia, guardando o caminho dos executáveis entre uma
/// checagem e outra (abrir cada processo é a parte cara).
#[derive(Debug, Default)]
pub struct Observer {
    paths: HashMap<u32, Option<String>>,
    checks: u32,
}

impl Observer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uma checagem: o que o vigia sabe agora. `quiet_ms` é há quanto tempo a captura atual não
    /// entrega quadros.
    pub fn observe(&mut self, follower: &GameFollower, quiet_ms: u64) -> Observation {
        // SAFETY: chamada simples sem argumentos.
        let own = unsafe { GetCurrentProcessId() };
        self.observe_skipping(follower, quiet_ms, own)
    }

    /// Como [`Observer::observe`], ignorando as janelas do processo `skip_pid` (o próprio SquadView).
    pub(crate) fn observe_skipping(&mut self, follower: &GameFollower, quiet_ms: u64, skip_pid: u32) -> Observation {
        self.checks = self.checks.wrapping_add(1);
        if self.checks.is_multiple_of(PATH_CACHE_CHECKS) {
            self.paths.clear();
        }
        observe_with(&mut self.paths, follower, quiet_ms, skip_pid)
    }
}

fn observe_with(
    paths: &mut HashMap<u32, Option<String>>,
    follower: &GameFollower,
    quiet_ms: u64,
    skip_pid: u32,
) -> Observation {
    let mut path_of = |pid: u32| paths.entry(pid).or_insert_with(|| process_path(pid)).clone();
    // A regra de família só depende do processo: decide uma vez por processo nesta checagem.
    let mut verdicts: HashMap<u32, bool> = HashMap::new();
    let game_id = follower.game().id;

    let mut obs = Observation {
        quiet_ms,
        ..Observation::default()
    };
    for hwnd in top_level_windows() {
        // Primeiro o que é barato (há centenas de janelas de primeiro nível, quase todas
        // escondidas): processo, estilo e dono. Título e geometria só para as da família.
        let (pid, tool_window, has_owner) = basics(hwnd);
        if pid == skip_pid || tool_window || has_owner {
            continue;
        }
        let id = CaptureSource::Window(hwnd);
        let family = *verdicts
            .entry(pid)
            .or_insert_with(|| follower.is_family(&family_probe(pid, &path_of(pid))));
        if id != game_id && !family {
            continue;
        }
        let path = path_of(pid);
        let info = window_info(hwnd);
        // Sem título não é uma janela que o jogador reconheceria. Escondida ainda vale (o cliente
        // do LoL fica escondido durante a partida): a visibilidade entra nos fatos.
        if info.title.trim().is_empty() && id != game_id {
            continue;
        }
        let facts = facts(hwnd, source_of(hwnd, &info, path), &info);
        if id == game_id {
            obs.game = Some(facts.clone());
        }
        obs.family.push(facts);
    }
    // A janela do jogo pode ter perdido o título (ou ganhado um dono): ainda é ela.
    if obs.game.is_none()
        && let CaptureSource::Window(h) = game_id
        && exists(h)
    {
        let info = window_info(h);
        obs.game = Some(facts(h, follower.game().clone(), &info));
    }
    obs.foreground = foreground().map(|h| {
        let info = window_info(h);
        let path = path_of(info.process_id);
        facts(h, source_of(h, &info, path), &info)
    });
    obs
}

/// Só o que a regra de família usa (processo e caminho), sem consultar o título.
fn family_probe(pid: u32, path: &Option<String>) -> Source {
    Source {
        id: CaptureSource::Window(0),
        name: String::new(),
        process_id: Some(pid),
        process_name: None,
        process_path: path.clone(),
    }
}

/// Processo dono, se é janela de ferramenta e se tem dono: consultas locais, sem mensagens.
fn basics(handle: isize) -> (u32, bool, bool) {
    let hwnd = HWND(handle as *mut _);
    let mut pid = 0u32;
    // SAFETY: consultas a um HWND que pode já não existir; o Windows só devolve zero/erro.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let tool = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid());
        (pid, tool, owned)
    }
}

fn source_of(hwnd: isize, info: &WindowInfo, path: Option<String>) -> Source {
    Source {
        id: CaptureSource::Window(hwnd),
        name: info.title.clone(),
        process_id: Some(info.process_id),
        process_name: path.as_deref().and_then(exe_name).map(str::to_owned),
        process_path: path,
    }
}

fn facts(hwnd: isize, source: Source, info: &WindowInfo) -> WindowFacts {
    let (rect, monitor, monitor_rect, minimized) = geometry(hwnd);
    WindowFacts {
        source,
        rect,
        monitor,
        monitor_rect,
        visible: info.visible && !info.cloaked,
        minimized,
    }
}

fn exists(hwnd: isize) -> bool {
    // SAFETY: consulta a um HWND que pode já não existir; o Windows só devolve falso.
    unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
}

fn foreground() -> Option<isize> {
    // SAFETY: chamada simples sem argumentos; pode devolver nulo (ex.: no meio do alt-tab).
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
}

/// Retângulo da janela, monitor dela (handle e retângulo) e se está minimizada.
///
/// Usa `GetWindowRect` (com as bordas invisíveis de redimensionar): uma janela maximizada passa um
/// pouco do monitor e não conta como tela cheia; a de um jogo em tela cheia (exclusiva ou sem
/// bordas) é exatamente o monitor.
fn geometry(handle: isize) -> (Rect, isize, Rect, bool) {
    let hwnd = HWND(handle as *mut _);
    let mut rect = RECT::default();
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: consultas a um HWND que pode já não existir (as funções só falham); `rect` e `info`
    // são locais com o tamanho certo.
    let (monitor, minimized) = unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(monitor, &mut info);
        (monitor.0 as isize, IsIconic(hwnd).as_bool())
    };
    (to_rect(rect), monitor, to_rect(info.rcMonitor), minimized)
}

fn to_rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::follow::{Status, Step};
    use crate::test_fullscreen::FullscreenGame;
    use crate::test_window::TestWindow;

    fn follower_of(window: &TestWindow) -> GameFollower {
        let info = window_info(window.hwnd);
        GameFollower::new(source_of(window.hwnd, &info, process_path(info.process_id)))
    }

    #[test]
    fn observa_a_janela_do_jogo_e_a_familia_dele() {
        let game = TestWindow::open("SquadView (jogo)", 400, 300);
        // Maior que as janelas dos outros testes (que rodam em paralelo, no mesmo processo) e
        // grande o bastante para ser a janela principal de um jogo.
        let other = TestWindow::open("SquadView (cliente)", 1200, 800);
        let follower = follower_of(&game);
        // As janelas de teste são deste processo: não pula nenhum processo.
        let obs = Observer::new().observe_skipping(&follower, 123, 0);
        assert_eq!(obs.quiet_ms, 123);
        let g = obs.game.clone().expect("a janela do jogo existe");
        assert_eq!(g.source.id, CaptureSource::Window(game.hwnd));
        assert!(g.visible && !g.minimized && !g.fullscreen());
        assert!(g.rect.area() > 0 && g.monitor_rect.area() > g.rect.area());
        assert!(
            g.source.process_path.is_some(),
            "o caminho do próprio processo sempre sai"
        );
        // Mesmo processo = mesma família.
        assert!(
            obs.family
                .iter()
                .any(|w| w.source.id == CaptureSource::Window(other.hwnd))
        );
        assert!(obs.family.iter().any(|w| w.source.id == g.source.id));

        // Escondida, a janela continua sendo observada (como o cliente do LoL na partida).
        game.hide();
        let obs = Observer::new().observe_skipping(&follower, 0, 0);
        assert!(!obs.game.expect("ainda existe").visible);

        // Fechada: some da observação, e o seguidor passa para a outra janela da família.
        let mut follower = follower;
        let hwnd = game.hwnd;
        drop(game);
        let obs = Observer::new().observe_skipping(&follower, 0, 0);
        assert!(obs.game.is_none());
        assert!(obs.family.iter().all(|w| w.source.id != CaptureSource::Window(hwnd)));
        match follower.step(&obs) {
            Step::Switch { capture, .. } => assert_eq!(capture, CaptureSource::Window(other.hwnd)),
            other => panic!("devia seguir a outra janela: {other:?}"),
        }
    }

    #[test]
    fn o_proprio_processo_fica_de_fora() {
        let window = TestWindow::open("SquadView (própria)", 320, 240);
        let follower = follower_of(&window);
        let obs = Observer::new().observe(&follower, 0);
        // A janela seguida ainda é observada (por handle), mas não entra como família.
        assert!(obs.family.is_empty());
        assert!(obs.game.is_some());
        let mut follower = follower;
        assert_eq!(follower.step(&obs), Step::Stay(Status::Ok));
    }

    #[test]
    fn caminhos_ficam_guardados_por_algumas_checagens() {
        let window = TestWindow::open("SquadView (cache)", 320, 240);
        let follower = follower_of(&window);
        let mut observer = Observer::new();
        observer.observe_skipping(&follower, 0, 0);
        assert!(!observer.paths.is_empty(), "guardou os caminhos da primeira checagem");
        for _ in 1..PATH_CACHE_CHECKS - 1 {
            observer.observe_skipping(&follower, 0, 0);
        }
        // A checagem de número PATH_CACHE_CHECKS começa do zero (e guarda de novo).
        observer.paths.insert(u32::MAX, None);
        observer.observe_skipping(&follower, 0, 0);
        assert!(!observer.paths.contains_key(&u32::MAX));
    }

    #[test]
    fn janela_inexistente_nao_aparece() {
        let follower = GameFollower::new(Source {
            id: CaptureSource::Window(0x7fff_fff0),
            name: "sumiu".into(),
            process_id: Some(u32::MAX),
            process_name: None,
            process_path: None,
        });
        let obs = Observer::new().observe(&follower, 0);
        assert!(obs.game.is_none());
        assert!(!exists(0x7fff_fff0));
    }

    #[test]
    #[ignore = "mede o custo da checagem; precisa de uma sessão de desktop"]
    fn custo_da_checagem_do_vigia() {
        let window = TestWindow::open("SquadView (medição)", 320, 240);
        let follower = follower_of(&window);
        let mut observer = Observer::new();
        let first = std::time::Instant::now();
        std::hint::black_box(observer.observe(&follower, 0));
        println!("observe(): primeira checagem {} µs", first.elapsed().as_micros());
        let runs = 200;
        let mut times: Vec<u128> = (0..runs)
            .map(|_| {
                let start = std::time::Instant::now();
                std::hint::black_box(observer.observe(&follower, 0));
                start.elapsed().as_micros()
            })
            .collect();
        times.sort_unstable();
        let windows = top_level_windows().len();
        println!(
            "observe(): {windows} janelas de primeiro nível; mediana {} µs, p95 {} µs, máx {} µs",
            times[runs / 2],
            times[runs * 95 / 100],
            times[runs - 1]
        );
    }

    /// Quadros que a captura de `source` entrega em `duration`.
    fn frames_from(gpu: &sv_gpu::Gpu, source: CaptureSource, duration: std::time::Duration) -> u32 {
        use std::sync::atomic::{AtomicU32, Ordering};
        let count = std::sync::Arc::new(AtomicU32::new(0));
        let c = count.clone();
        let capture = crate::VideoCapture::start(gpu, source, move |_| {
            c.fetch_add(1, Ordering::Relaxed);
        });
        if capture.is_err() {
            return 0;
        }
        std::thread::sleep(duration);
        drop(capture);
        count.load(Ordering::Relaxed)
    }

    /// Roda o vigia de verdade (observação + decisão + captura) sobre o "jogo" em tela cheia e
    /// devolve o que foi capturado no fim e quantos quadros chegaram no último segundo.
    fn follow_for_real(gpu: &sv_gpu::Gpu, game: &FullscreenGame, ticks: u32) -> (Vec<String>, CaptureSource, u32) {
        use std::sync::atomic::{AtomicU64, Ordering};
        let info = window_info(game.hwnd);
        let mut follower = GameFollower::new(source_of(game.hwnd, &info, process_path(info.process_id)));
        let last = std::sync::Arc::new(AtomicU64::new(sv_gpu::clock::now_us()));
        let start = |source: CaptureSource| {
            last.store(sv_gpu::clock::now_us(), Ordering::Relaxed);
            let l = last.clone();
            crate::VideoCapture::start(gpu, source, move |_| {
                l.store(sv_gpu::clock::now_us(), Ordering::Relaxed)
            })
            .ok()
        };
        let mut capture = start(follower.capture());
        let mut log = Vec::new();
        let mut observer = Observer::new();
        for _ in 0..ticks {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let quiet_ms = (sv_gpu::clock::now_us().saturating_sub(last.load(Ordering::Relaxed))) / 1000;
            let obs = observer.observe_skipping(&follower, quiet_ms, 0);
            let step = follower.step(&obs);
            log.push(format!("quieto {quiet_ms} ms → {step:?}"));
            if let Step::Switch { capture: to, .. } = step {
                drop(capture.take());
                capture = start(to);
            }
        }
        drop(capture);
        let current = follower.capture();
        (
            log,
            current,
            frames_from(gpu, current, std::time::Duration::from_secs(1)),
        )
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop (ocupa a tela por uns 15 s)"]
    fn jogo_em_tela_cheia_sempre_tem_imagem() {
        use crate::test_fullscreen::{FullscreenGame, Mode};
        let gpu = sv_gpu::Gpu::new().unwrap();
        for mode in [Mode::Borderless, Mode::Exclusive] {
            let game = FullscreenGame::open(mode);
            std::thread::sleep(std::time::Duration::from_millis(800));
            let info = window_info(game.hwnd);
            let follower = GameFollower::new(source_of(game.hwnd, &info, None));
            let obs = Observer::new().observe_skipping(&follower, 0, 0);
            let facts = obs.game.clone().expect("a janela do jogo existe");
            let in_front = obs.foreground.as_ref().map(|f| f.source.id) == Some(facts.source.id);
            let second = std::time::Duration::from_secs(1);
            let window_frames = frames_from(&gpu, CaptureSource::Window(game.hwnd), second);
            let monitor_frames = frames_from(&gpu, CaptureSource::Monitor(facts.monitor), second);
            let (log, last, frames) = follow_for_real(&gpu, &game, 8);
            println!(
                "{mode:?} (exclusiva aceita: {}): tela cheia detectada {}, na frente {in_front};                  quadros em 1 s: janela {window_frames}, monitor {monitor_frames};                  vigia terminou em {last:?} com {frames} quadros/s",
                game.exclusive,
                facts.fullscreen()
            );
            for line in &log {
                println!("  {line}");
            }
            assert!(
                facts.fullscreen(),
                "{mode:?}: janela do tamanho do monitor é tela cheia"
            );
            assert!(frames > 0, "{mode:?}: o vigia tem que terminar numa captura com imagem");
            drop(game);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
