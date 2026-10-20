//! Decide o que capturar para **seguir o jogo**: lógica pura (estado + fatos observados → ação),
//! sem nenhuma chamada ao Windows. Quem observa as janelas é [`crate::observe`]; quem troca a
//! captura é o vigia da prévia (a cada 500 ms, nunca no caminho do frame).
//!
//! As regras e o porquê de cada uma estão em `docs/captura.md`. Em resumo:
//! 1. Outra janela da família do jogo foi para o primeiro plano (por 2 checagens seguidas) →
//!    segue ela (cliente → partida → cliente).
//! 2. A janela atual sumiu ou foi escondida → segue a maior janela visível da família.
//! 3. A janela do jogo cobre o monitor inteiro, está na frente e a captura dela parou de entregar
//!    quadros (tela cheia exclusiva) → captura o monitor onde ela está.
//! 4. Capturando o monitor, o jogo saiu da tela cheia, foi minimizado ou outra janela ficou na
//!    frente dele → volta para a janela (o monitor mostraria o que não é o jogo).

use crate::family::same_family;
use crate::{CaptureSource, Source};

/// Checagens seguidas com outra janela da família na frente antes de seguir ela (evita trocar
/// por uma tela de carregamento que aparece por um instante).
pub const FOLLOW_AFTER_TICKS: u8 = 2;
/// Janela do jogo em tela cheia sem entregar quadros por este tempo → captura o monitor.
pub const FULLSCREEN_QUIET_MS: u64 = 2_000;
/// Sem quadros por este tempo numa janela → aviso de "sem imagem".
pub const STALL_AFTER_MS: u64 = 4_000;

/// Retângulo em pixels da tela (as coordenadas do Windows, `right`/`bottom` exclusivos).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn area(&self) -> i64 {
        i64::from((self.right - self.left).max(0)) * i64::from((self.bottom - self.top).max(0))
    }
}

/// O que o vigia sabe de uma janela numa checagem.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowFacts {
    pub source: Source,
    /// Retângulo da janela (sem a sombra do DWM).
    pub rect: Rect,
    /// Monitor onde está a maior parte da janela, e o retângulo dele.
    pub monitor: isize,
    pub monitor_rect: Rect,
    /// Visível e não escondida pelo DWM (cloaked).
    pub visible: bool,
    pub minimized: bool,
}

impl WindowFacts {
    /// Dá para ver a janela: visível e não minimizada.
    fn shown(&self) -> bool {
        self.visible && !self.minimized
    }

    /// Cobre o monitor inteiro (tela cheia, exclusiva ou sem bordas).
    pub fn fullscreen(&self) -> bool {
        self.shown() && self.rect.area() > 0 && self.rect == self.monitor_rect
    }

    /// Grande o bastante para ser a janela principal de um jogo (e não um aviso ou uma tela
    /// de "carregando" pequena): pelo menos 1/8 do monitor.
    fn main_sized(&self) -> bool {
        self.rect.area() * 8 >= self.monitor_rect.area()
    }

    fn id(&self) -> CaptureSource {
        self.source.id
    }
}

/// Uma checagem do vigia.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// A janela do jogo que estamos seguindo, se ela ainda existir (mesmo escondida).
    pub game: Option<WindowFacts>,
    /// Janelas da família do jogo que existem (visíveis ou não), da frente para trás.
    pub family: Vec<WindowFacts>,
    /// A janela em primeiro plano (`None` quando o Windows não informa, ex.: durante o alt-tab).
    pub foreground: Option<WindowFacts>,
    /// Há quanto tempo a captura atual não entrega quadros.
    pub quiet_ms: u64,
}

/// Por que a captura mudou (o vigia usa para o aviso e para o registro).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchReason {
    /// O jogo passou para outra janela (ex.: do cliente para a partida).
    NewWindow,
    /// O jogo está em tela cheia e a janela parou de entregar quadros: capturando o monitor.
    Fullscreen,
    /// Saiu da tela cheia (ou outra janela ficou na frente): de volta à janela do jogo.
    BackToWindow,
}

/// Situação quando a captura continua a mesma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Tudo certo (ou cedo demais para dizer).
    Ok,
    /// A janela do jogo está minimizada.
    Minimized,
    /// A janela do jogo foi escondida ou fechou e nenhuma outra do jogo apareceu ainda.
    Waiting,
    /// O jogo está em tela cheia, mas outra janela está na frente dele.
    Covered,
    /// A janela está aí, mas não chega imagem nova (ex.: o jogo está carregando ou travou).
    Stalled,
}

/// O que o vigia deve fazer.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Stay(Status),
    /// Capturar `capture`; `game` é a janela do jogo (para o áudio e os avisos).
    Switch {
        capture: CaptureSource,
        game: Source,
        reason: SwitchReason,
    },
    /// Estávamos no monitor e o jogo fechou sem deixar nenhuma janela: parar, para não mostrar a
    /// área de trabalho para a sala.
    Stop,
}

/// Estado de quem segue o jogo.
#[derive(Debug, Clone)]
pub struct GameFollower {
    /// A fonte que o usuário escolheu (define a família).
    chosen: Source,
    /// A janela do jogo que estamos seguindo agora.
    game: Source,
    /// O que está sendo capturado: a janela do jogo ou o monitor dele.
    capture: CaptureSource,
    /// Janela da família na frente, e há quantas checagens.
    pending: Option<(CaptureSource, u8)>,
}

impl GameFollower {
    /// Começa seguindo a janela escolhida pelo usuário.
    pub fn new(chosen: Source) -> Self {
        Self {
            capture: chosen.id,
            game: chosen.clone(),
            chosen,
            pending: None,
        }
    }

    pub fn game(&self) -> &Source {
        &self.game
    }

    pub fn capture(&self) -> CaptureSource {
        self.capture
    }

    /// A janela é do mesmo jogo que a escolhida (ou que a que seguimos agora)?
    pub fn is_family(&self, source: &Source) -> bool {
        same_family(&self.chosen, source) || same_family(&self.game, source)
    }

    /// Decide o passo desta checagem. Se for uma troca, o estado já passa a valer a fonte nova.
    pub fn step(&mut self, obs: &Observation) -> Step {
        if let Some(next) = self.next_window(obs) {
            self.pending = None;
            return self.switch(next.id(), next.source.clone(), SwitchReason::NewWindow);
        }
        let Some(game) = obs.game.as_ref() else {
            return match self.capture {
                CaptureSource::Monitor(_) => Step::Stop,
                CaptureSource::Window(_) => Step::Stay(Status::Waiting),
            };
        };
        match self.capture {
            CaptureSource::Window(_) => {
                let quiet = obs.quiet_ms;
                if game.fullscreen() && quiet >= FULLSCREEN_QUIET_MS && self.owns_monitor(game, obs) {
                    return self.switch(
                        CaptureSource::Monitor(game.monitor),
                        game.source.clone(),
                        SwitchReason::Fullscreen,
                    );
                }
                Step::Stay(window_status(game, quiet, self.owns_monitor(game, obs)))
            }
            CaptureSource::Monitor(monitor) => {
                if game.fullscreen() && game.monitor == monitor && self.owns_monitor(game, obs) {
                    Step::Stay(Status::Ok)
                } else {
                    self.switch(game.id(), game.source.clone(), SwitchReason::BackToWindow)
                }
            }
        }
    }

    fn switch(&mut self, capture: CaptureSource, game: Source, reason: SwitchReason) -> Step {
        self.capture = capture;
        self.game = game.clone();
        Step::Switch { capture, game, reason }
    }

    /// Outra janela do jogo para seguir, se houver.
    fn next_window<'a>(&mut self, obs: &'a Observation) -> Option<&'a WindowFacts> {
        let current = self.game.id;
        let candidate = |w: &&WindowFacts| w.id() != current && w.shown() && w.main_sized();
        // Outra janela da família na frente: o jogador está olhando para ela.
        let front = obs
            .foreground
            .as_ref()
            .filter(candidate)
            .filter(|f| self.is_family(&f.source));
        // A janela atual sumiu ou foi escondida (minimizada não conta: o jogador volta para ela).
        let gone = obs.game.as_ref().is_none_or(|g| !g.visible);
        if gone {
            self.pending = None;
            let others = || obs.family.iter().filter(|w| w.id() != current);
            return front
                // Senão, a maior visível; no empate, a da frente.
                .or_else(|| others().filter(candidate).rev().max_by_key(|w| w.rect.area()))
                // Nenhuma visível e o jogo fechou: uma que ainda existe escondida (ex.: o cliente
                // do LoL que reaparece depois da partida), de preferência a escolhida, para não
                // ficar no monitor mostrando a área de trabalho.
                .or_else(|| {
                    obs.game.is_none().then_some(())?;
                    others().find(|w| w.id() == self.chosen.id).or_else(|| others().next())
                });
        }
        let Some(front) = front else {
            self.pending = None;
            return None;
        };
        // A atual continua visível: só segue a da frente depois de algumas checagens seguidas.
        let ticks = match self.pending {
            Some((id, n)) if id == front.id() => n.saturating_add(1),
            _ => 1,
        };
        self.pending = Some((front.id(), ticks));
        (ticks >= FOLLOW_AFTER_TICKS).then_some(front)
    }

    /// O monitor do jogo mostra só o jogo: ninguém de fora da família na frente dele naquele monitor.
    fn owns_monitor(&self, game: &WindowFacts, obs: &Observation) -> bool {
        obs.foreground
            .as_ref()
            .is_none_or(|f| f.id() == game.id() || f.monitor != game.monitor || self.is_family(&f.source))
    }
}

/// Situação de uma captura de janela que continua a mesma.
fn window_status(game: &WindowFacts, quiet_ms: u64, owns_monitor: bool) -> Status {
    if !game.visible {
        Status::Waiting
    } else if quiet_ms <= STALL_AFTER_MS {
        Status::Ok
    } else if game.minimized {
        Status::Minimized
    } else if game.fullscreen() && !owns_monitor {
        Status::Covered
    } else {
        Status::Stalled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOBBY: &str = r"C:\Riot Games\League of Legends\LeagueClientUx.exe";
    const GAME: &str = r"C:\Riot Games\League of Legends\Game\League of Legends.exe";

    const MON1: isize = 1001;
    const MON2: isize = 1002;
    const FULL1: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const FULL2: Rect = Rect {
        left: 1920,
        top: 0,
        right: 4480,
        bottom: 1440,
    };

    fn rect(left: i32, top: i32, w: i32, h: i32) -> Rect {
        Rect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        }
    }

    fn win(hwnd: isize, pid: u32, path: &str, rect: Rect) -> WindowFacts {
        WindowFacts {
            source: Source {
                id: CaptureSource::Window(hwnd),
                name: format!("janela {hwnd}"),
                process_id: Some(pid),
                process_name: None,
                process_path: Some(path.into()),
            },
            rect,
            monitor: MON1,
            monitor_rect: FULL1,
            visible: true,
            minimized: false,
        }
    }

    fn lobby() -> WindowFacts {
        win(1, 10, LOBBY, rect(320, 180, 1280, 720))
    }

    fn match_window() -> WindowFacts {
        win(2, 20, GAME, FULL1)
    }

    fn chrome() -> WindowFacts {
        win(
            9,
            90,
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            rect(0, 0, 1600, 900),
        )
    }

    fn hidden(mut w: WindowFacts) -> WindowFacts {
        w.visible = false;
        w
    }

    fn minimized(mut w: WindowFacts) -> WindowFacts {
        w.minimized = true;
        w.rect = rect(-32000, -32000, 160, 28);
        w
    }

    fn obs(game: Option<WindowFacts>, family: &[WindowFacts], fg: Option<WindowFacts>, quiet_ms: u64) -> Observation {
        Observation {
            game,
            family: family.to_vec(),
            foreground: fg,
            quiet_ms,
        }
    }

    fn switched_to(step: &Step) -> Option<(CaptureSource, SwitchReason)> {
        match step {
            Step::Switch { capture, reason, .. } => Some((*capture, *reason)),
            _ => None,
        }
    }

    #[test]
    fn lol_cliente_partida_em_tela_cheia_e_volta_ao_cliente() {
        let mut f = GameFollower::new(lobby().source);
        // No lobby, tudo certo.
        assert_eq!(
            f.step(&obs(Some(lobby()), &[lobby()], Some(lobby()), 100)),
            Step::Stay(Status::Ok)
        );

        // A partida começa: o cliente se esconde, a janela do jogo ainda não existe.
        let o = obs(Some(hidden(lobby())), &[hidden(lobby())], None, 1_000);
        assert_eq!(f.step(&o), Step::Stay(Status::Waiting));

        // A janela da partida aparece em tela cheia, na frente: segue ela (cliente escondido = na hora).
        let o = obs(
            Some(hidden(lobby())),
            &[match_window(), hidden(lobby())],
            Some(match_window()),
            1_500,
        );
        let step = f.step(&o);
        assert_eq!(switched_to(&step), Some((match_window().id(), SwitchReason::NewWindow)));
        let Step::Switch { game, .. } = step else {
            unreachable!()
        };
        assert_eq!(game.process_id, Some(20), "o áudio passa para o processo da partida");

        // A captura da janela em tela cheia não entrega quadros: passa para o monitor dela.
        let o = obs(
            Some(match_window()),
            &[match_window(), hidden(lobby())],
            Some(match_window()),
            500,
        );
        assert_eq!(f.step(&o), Step::Stay(Status::Ok), "ainda cedo");
        let o = obs(
            Some(match_window()),
            &[match_window(), hidden(lobby())],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        );
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((CaptureSource::Monitor(MON1), SwitchReason::Fullscreen))
        );
        assert_eq!(f.capture(), CaptureSource::Monitor(MON1));
        // No monitor, uma tela parada não é problema.
        let o = obs(
            Some(match_window()),
            &[match_window(), hidden(lobby())],
            Some(match_window()),
            60_000,
        );
        assert_eq!(f.step(&o), Step::Stay(Status::Ok));

        // Fim da partida: a janela do jogo fecha e o cliente volta.
        let o = obs(None, &[lobby()], Some(lobby()), 0);
        assert_eq!(switched_to(&f.step(&o)), Some((lobby().id(), SwitchReason::NewWindow)));
        assert_eq!(f.game().process_id, Some(10));
        assert_eq!(
            f.step(&obs(Some(lobby()), &[lobby()], Some(lobby()), 0)),
            Step::Stay(Status::Ok)
        );
    }

    #[test]
    fn cliente_visivel_atras_da_partida_tambem_e_seguido() {
        // Alguns launchers continuam visíveis atrás do jogo: vale a janela da família na frente.
        let mut f = GameFollower::new(lobby().source);
        let o = obs(Some(lobby()), &[match_window(), lobby()], Some(match_window()), 300);
        assert_eq!(f.step(&o), Step::Stay(Status::Ok), "espera mais uma checagem");
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((match_window().id(), SwitchReason::NewWindow))
        );
    }

    #[test]
    fn tela_de_carregamento_rapida_nao_rouba_a_captura() {
        let mut f = GameFollower::new(lobby().source);
        let splash = win(3, 30, GAME, rect(560, 315, 800, 450));
        assert_eq!(
            f.step(&obs(Some(lobby()), &[splash.clone(), lobby()], Some(splash), 0)),
            Step::Stay(Status::Ok)
        );
        // Sumiu antes da segunda checagem: a contagem zera.
        assert_eq!(
            f.step(&obs(Some(lobby()), &[lobby()], Some(lobby()), 0)),
            Step::Stay(Status::Ok)
        );
        let o = obs(Some(lobby()), &[match_window(), lobby()], Some(match_window()), 0);
        assert_eq!(f.step(&o), Step::Stay(Status::Ok));
    }

    #[test]
    fn avisos_pequenos_do_jogo_nao_viram_a_fonte() {
        let mut f = GameFollower::new(lobby().source);
        let toast = win(4, 10, LOBBY, rect(1600, 900, 300, 120));
        for _ in 0..5 {
            assert_eq!(
                f.step(&obs(Some(lobby()), &[toast.clone(), lobby()], Some(toast.clone()), 0)),
                Step::Stay(Status::Ok)
            );
        }
    }

    #[test]
    fn launcher_que_fecha_passa_para_o_jogo() {
        let launcher = win(
            1,
            10,
            r"C:\Riot Games\VALORANT\live\VALORANT.exe",
            rect(0, 0, 1280, 720),
        );
        let game = win(
            2,
            20,
            r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe",
            rect(0, 0, 1920, 1080),
        );
        let mut f = GameFollower::new(launcher.source.clone());
        // O launcher fecha; ninguém na frente ainda; a janela do jogo existe visível.
        let step = f.step(&obs(None, std::slice::from_ref(&game), None, 0));
        assert_eq!(switched_to(&step), Some((game.id(), SwitchReason::NewWindow)));
    }

    #[test]
    fn janela_que_fecha_sem_substituta_espera() {
        let mut f = GameFollower::new(lobby().source);
        let unrelated = chrome();
        assert_eq!(
            f.step(&obs(None, &[], Some(unrelated), 0)),
            Step::Stay(Status::Waiting),
            "janela de outro programa nunca é seguida"
        );
    }

    #[test]
    fn jogo_minimizado_avisa_e_nao_troca() {
        let mut f = GameFollower::new(match_window().source);
        let min = minimized(match_window());
        let o = obs(Some(min.clone()), &[min.clone(), hidden(lobby())], None, 1_000);
        assert_eq!(f.step(&o), Step::Stay(Status::Ok));
        let o = obs(Some(min.clone()), &[min, hidden(lobby())], None, STALL_AFTER_MS + 1);
        assert_eq!(f.step(&o), Step::Stay(Status::Minimized));
    }

    #[test]
    fn alt_tab_para_a_area_de_trabalho_sai_do_monitor() {
        let mut f = GameFollower::new(match_window().source);
        let o = obs(
            Some(match_window()),
            &[match_window()],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        );
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((CaptureSource::Monitor(MON1), SwitchReason::Fullscreen))
        );

        // Jogo sem bordas: alt-tab deixa o Chrome por cima, no mesmo monitor → volta para a janela.
        let o = obs(Some(match_window()), &[match_window()], Some(chrome()), 0);
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((match_window().id(), SwitchReason::BackToWindow))
        );
        // Enquanto o Chrome está na frente, não volta para o monitor, mesmo sem quadros.
        let o = obs(
            Some(match_window()),
            &[match_window()],
            Some(chrome()),
            STALL_AFTER_MS + 1,
        );
        assert_eq!(f.step(&o), Step::Stay(Status::Covered));

        // Tela cheia exclusiva: alt-tab minimiza o jogo → volta para a janela e avisa.
        let mut f = GameFollower::new(match_window().source);
        let o = obs(
            Some(match_window()),
            &[match_window()],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        );
        f.step(&o);
        let min = minimized(match_window());
        let o = obs(Some(min.clone()), std::slice::from_ref(&min), None, 0);
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((match_window().id(), SwitchReason::BackToWindow))
        );
        let o = obs(Some(min.clone()), &[min], None, STALL_AFTER_MS + 1);
        assert_eq!(f.step(&o), Step::Stay(Status::Minimized));

        // De volta ao jogo: a janela não entrega quadros de novo → monitor de novo.
        let o = obs(
            Some(match_window()),
            &[match_window()],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        );
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((CaptureSource::Monitor(MON1), SwitchReason::Fullscreen))
        );
    }

    #[test]
    fn jogo_que_sai_da_tela_cheia_volta_para_a_janela() {
        let mut f = GameFollower::new(match_window().source);
        f.step(&obs(
            Some(match_window()),
            &[],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        ));
        let mut windowed = match_window();
        windowed.rect = rect(100, 100, 1280, 720);
        let step = f.step(&obs(Some(windowed.clone()), &[], Some(windowed), 0));
        assert_eq!(
            switched_to(&step),
            Some((match_window().id(), SwitchReason::BackToWindow))
        );
    }

    #[test]
    fn tela_cheia_que_entrega_quadros_fica_na_janela() {
        // Tela cheia "otimizada" (flip): a captura da janela funciona e é a melhor opção.
        let mut f = GameFollower::new(match_window().source);
        for _ in 0..10 {
            assert_eq!(
                f.step(&obs(Some(match_window()), &[], Some(match_window()), 16)),
                Step::Stay(Status::Ok)
            );
        }
    }

    #[test]
    fn jogo_que_fecha_com_o_monitor_capturado_para_a_transmissao() {
        let mut f = GameFollower::new(match_window().source);
        f.step(&obs(
            Some(match_window()),
            &[],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        ));
        assert_eq!(f.step(&obs(None, &[], None, 0)), Step::Stop);
    }

    #[test]
    fn jogo_que_fecha_volta_para_o_cliente_mesmo_escondido() {
        // Partida acabou e o cliente ainda não reapareceu: captura a janela dele (escondida) em
        // vez de continuar mostrando o monitor.
        let mut f = GameFollower::new(lobby().source);
        f.step(&obs(
            Some(hidden(lobby())),
            &[match_window(), hidden(lobby())],
            Some(match_window()),
            0,
        ));
        f.step(&obs(
            Some(match_window()),
            &[match_window()],
            Some(match_window()),
            FULLSCREEN_QUIET_MS,
        ));
        assert_eq!(f.capture(), CaptureSource::Monitor(MON1));
        let other_hidden = hidden(win(5, 10, LOBBY, rect(0, 0, 1280, 720)));
        let step = f.step(&obs(None, &[other_hidden, hidden(lobby())], None, 0));
        assert_eq!(switched_to(&step), Some((lobby().id(), SwitchReason::NewWindow)));
        assert_eq!(
            f.step(&obs(Some(hidden(lobby())), &[hidden(lobby())], None, 0)),
            Step::Stay(Status::Waiting)
        );
    }

    #[test]
    fn dois_monitores_captura_o_monitor_do_jogo() {
        let mut game = match_window();
        game.monitor = MON2;
        game.monitor_rect = FULL2;
        game.rect = FULL2;
        let mut discord = chrome();
        discord.rect = rect(0, 0, 1920, 1080);
        let mut f = GameFollower::new(game.source.clone());
        // O jogador clicou no Discord do outro monitor: o monitor do jogo continua só com o jogo.
        let o = obs(Some(game.clone()), &[], Some(discord.clone()), FULLSCREEN_QUIET_MS);
        assert_eq!(
            switched_to(&f.step(&o)),
            Some((CaptureSource::Monitor(MON2), SwitchReason::Fullscreen))
        );
        assert_eq!(
            f.step(&obs(Some(game.clone()), &[], Some(discord), 60_000)),
            Step::Stay(Status::Ok)
        );

        // O jogo foi para o monitor 1 (ainda em tela cheia): volta para a janela, depois o monitor 1.
        let mut moved = match_window();
        moved.source = game.source.clone();
        let step = f.step(&obs(Some(moved.clone()), &[], Some(moved.clone()), 0));
        assert_eq!(switched_to(&step), Some((game.id(), SwitchReason::BackToWindow)));
        let step = f.step(&obs(Some(moved.clone()), &[], Some(moved), FULLSCREEN_QUIET_MS));
        assert_eq!(
            switched_to(&step),
            Some((CaptureSource::Monitor(MON1), SwitchReason::Fullscreen))
        );
    }

    #[test]
    fn janela_parada_que_nao_esta_em_tela_cheia() {
        let mut f = GameFollower::new(lobby().source);
        let o = obs(Some(lobby()), &[lobby()], Some(lobby()), STALL_AFTER_MS + 1);
        assert_eq!(f.step(&o), Step::Stay(Status::Stalled));
        // Tela cheia sem estar na frente não vai para o monitor.
        let mut f = GameFollower::new(match_window().source);
        let o = obs(Some(match_window()), &[], Some(chrome()), STALL_AFTER_MS + 1);
        assert_eq!(f.step(&o), Step::Stay(Status::Covered));
    }

    #[test]
    fn retangulo_e_tela_cheia() {
        assert_eq!(rect(0, 0, 10, 20).area(), 200);
        assert_eq!(Rect::default().area(), 0);
        assert_eq!(
            Rect {
                left: 5,
                top: 5,
                right: 0,
                bottom: 0
            }
            .area(),
            0
        );
        assert!(match_window().fullscreen());
        assert!(!lobby().fullscreen());
        assert!(!minimized(match_window()).fullscreen());
        assert!(!hidden(match_window()).fullscreen());
    }
}
