//! Fliperama da sala (ADR 0023 e 0024): o host empresta o jogo dele, e até 3 membros jogam como
//! P2 a P4 com o controle, o teclado e o mouse deles.
//!
//! - **Host:** dá as vagas, pluga um controle virtual de Xbox (ViGEmBus) para cada jogador, aplica
//!   os estados que chegam por datagrama e, com o "controle total" de um jogador ligado, injeta o
//!   teclado e o mouse dele (só com a janela do jogo em primeiro plano, e com corte em Ctrl+Alt+F12).
//! - **Jogador:** pede a vaga, abre a conexão do fliperama, lê o controle físico (250 por segundo)
//!   e junta com o teclado e o mouse; manda o estado quando muda e, parado, a cada 100 ms.
//!
//! O vídeo é a live normal de quem hospeda: o jogador assiste pelo caminho de sempre.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use quinn::Connection;
use serde::Serialize;
use sv_input::kbm::{Injector, KbmGuard, PanicHotkey, vk_from_code};
use sv_input::wire::{KBM_EVENT_LEN, decode_pad, encode_pad, is_newer};
use sv_input::{KbmEvent, KeyMap, KeyboardPad, MouseButton, PadReader, PadState, VirtualPad, VirtualPads};
use sv_net::Arcade;
use sv_net::protocol::Signal;

use super::{Inner, RoomUpdate};
use crate::surface::lock;

/// Vagas de um fliperama (P2 a P4: o P1 é quem hospeda).
pub const MAX_SLOTS: u8 = 3;
/// Sem nada do jogador por esse tempo, o controle dele é solto (caiu ou travou).
const SILENCE_RELEASE: Duration = Duration::from_secs(1);
/// Parado, o jogador manda o estado de novo a cada tanto (o host sabe que ele continua lá).
const RESEND_EVERY: Duration = Duration::from_millis(100);
/// De quanto em quanto o jogador olha se o estado mudou (o analógico do mouse volta ao centro).
const TICK: Duration = Duration::from_millis(4);

/// Um jogador no fliperama que abrimos (para a interface do host).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcadeSeatInfo {
    pub member_id: String,
    /// O P do jogador (2 a 4).
    pub slot: u8,
    /// A conexão do fliperama dele já abriu.
    pub connected: bool,
    /// Ele está com o controle total de teclado e mouse (ADR 0024).
    pub kbm: bool,
}

/// O que o fliperama precisa saber da sala.
#[derive(Default)]
pub(super) struct ArcadeState {
    host: Mutex<Option<Host>>,
    player: Mutex<Option<Player>>,
}

struct Host {
    info: Arcade,
    pads: Arc<VirtualPads>,
    seats: BTreeMap<String, Seat>,
    /// Fingerprint de quem recebeu vaga → o membro (só esses abrem a conexão do fliperama).
    expected: HashMap<String, String>,
    /// A janela do jogo (captura de janela); sem ela, o controle total não liga.
    window: Option<isize>,
    /// Quem está com o controle total, e o injetor dele.
    kbm: Option<String>,
    injector: Arc<Mutex<Option<Injector>>>,
    hotkey: Option<PanicHotkey>,
    /// Membros esperando o controle virtual plugar (não contam duas vezes).
    plugging: Vec<String>,
}

struct Seat {
    slot: u8,
    pad: Arc<VirtualPad>,
    conn: Option<Connection>,
}

struct Player {
    host: String,
    slot: Option<u8>,
    conn: Option<Connection>,
    keyboard: Arc<Mutex<KeyboardPad>>,
    physical: Arc<Mutex<PadState>>,
    reader: Option<PadReader>,
    /// Controle total ligado pelo host: teclado e mouse vão crus, por `kbm_tx`.
    kbm: bool,
    kbm_tx: Option<tokio::sync::mpsc::UnboundedSender<KbmEvent>>,
    kbm_guard: KbmGuard,
    mouse: (bool, bool),
    stop: Arc<AtomicBool>,
}

impl Player {
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.reader = None;
        self.kbm_tx = None;
        if let Some(conn) = self.conn.take() {
            conn.close(0u32.into(), b"saiu");
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn sorry(reason: &str) -> Signal {
    Signal::ArcadeDeny { reason: reason.into() }
}

impl Inner {
    // ---------------------------------------------------------------- host

    /// Abre o fliperama sobre a nossa live. `window`: a janela capturada (para o controle total).
    pub(super) fn arcade_open(
        self: &Arc<Self>,
        game: Option<String>,
        slots: u8,
        window: Option<isize>,
    ) -> Result<(), String> {
        if lock(&self.keyframes).is_none() {
            return Err("Comece a transmitir o jogo antes de abrir o fliperama.".into());
        }
        let pads = VirtualPads::connect().map_err(|e| match sv_input::driver::driver_status() {
            sv_input::DriverStatus::Missing => {
                "O driver de controle (ViGEmBus) não está instalado. Instale em Configurações > Fliperama.".to_owned()
            }
            _ => format!("Não deu para usar o driver de controle: {e}"),
        })?;
        let info = Arcade {
            game: game
                .map(|g| g.trim().chars().take(64).collect::<String>())
                .filter(|g| !g.is_empty()),
            slots: slots.clamp(1, MAX_SLOTS),
            players: Vec::new(),
        };
        let previous = lock(&self.arcade.host).replace(Host {
            info,
            pads: Arc::new(pads),
            seats: BTreeMap::new(),
            expected: HashMap::new(),
            window,
            kbm: None,
            injector: Arc::default(),
            hotkey: None,
            plugging: Vec::new(),
        });
        if let Some(old) = previous {
            self.say_goodbye(old);
        }
        self.arcade_changed();
        Ok(())
    }

    /// Fecha o fliperama: tira todos os jogadores e os controles virtuais.
    pub(super) fn arcade_close(&self) {
        let host = lock(&self.arcade.host).take();
        if let Some(host) = host {
            self.say_goodbye(host);
            self.arcade_changed();
        }
    }

    fn say_goodbye(&self, host: Host) {
        if let Some(injector) = lock(&host.injector).as_mut() {
            injector.release_all();
        }
        for (member, seat) in host.seats {
            lock(&self.client).signal(&member, &Signal::ArcadeLeave);
            if let Some(conn) = seat.conn {
                conn.close(0u32.into(), b"fechou");
            }
        }
        for member in host.plugging {
            lock(&self.client).signal(&member, &Signal::ArcadeLeave);
        }
    }

    /// Tira um jogador do fliperama.
    pub(super) fn arcade_kick(&self, member: &str) {
        if self.free_seat(member) {
            lock(&self.client).signal(member, &Signal::ArcadeLeave);
            self.arcade_changed();
        }
    }

    /// Libera a vaga de `member` (saiu, caiu ou foi tirado). Devolve se ele tinha vaga.
    fn free_seat(&self, member: &str) -> bool {
        let mut guard = lock(&self.arcade.host);
        let Some(host) = guard.as_mut() else { return false };
        host.plugging.retain(|m| m != member);
        host.expected.retain(|_, m| m != member);
        if host.kbm.as_deref() == Some(member) {
            host.kbm = None;
            if let Some(mut injector) = lock(&host.injector).take() {
                injector.release_all();
            }
        }
        match host.seats.remove(member) {
            Some(seat) => {
                if let Some(conn) = seat.conn {
                    conn.close(0u32.into(), b"saiu");
                }
                // O controle virtual sai do PC quando o último `Arc` cai (a tarefa da conexão).
                true
            }
            None => false,
        }
    }

    /// Liga o controle total de teclado e mouse para `member` (ou desliga, com `None`). Um jogador
    /// por vez; exige a captura de uma janela (ADR 0024).
    pub(super) fn arcade_grant_kbm(self: &Arc<Self>, member: Option<&str>) -> Result<(), String> {
        let (previous, granted, old_hotkey) = {
            let mut guard = lock(&self.arcade.host);
            let host = guard.as_mut().ok_or("O fliperama está fechado.")?;
            if let Some(m) = member {
                if !host.seats.contains_key(m) {
                    return Err("Esse membro não está jogando no fliperama.".into());
                }
                if host.window.is_none() {
                    return Err(
                        "O controle total precisa da transmissão de uma janela (o jogo), não da tela inteira.".into(),
                    );
                }
            }
            let previous = host.kbm.take();
            if let Some(mut injector) = lock(&host.injector).take() {
                injector.release_all();
            }
            if let (Some(m), Some(window)) = (member, host.window) {
                host.kbm = Some(m.to_owned());
                *lock(&host.injector) = Some(Injector::new(window));
                if host.hotkey.is_none() {
                    let weak: Weak<Inner> = Arc::downgrade(self);
                    // Outro programa pode já usar o atalho: o corte pelo painel continua valendo.
                    host.hotkey = PanicHotkey::start(move || {
                        // Corta fora da thread do atalho: desligar descarta o atalho, que espera
                        // a thread dele terminar.
                        if let Some(inner) = weak.upgrade() {
                            let runtime = inner.runtime.clone();
                            runtime.spawn(async move {
                                let _ = inner.arcade_grant_kbm(None);
                            });
                        }
                    })
                    .ok();
                }
                (previous, host.kbm.clone(), None)
            } else {
                (previous, None, host.hotkey.take())
            }
        };
        // Fora da trava do fliperama: descartar o atalho espera a thread dele.
        drop(old_hotkey);
        if let Some(prev) = previous.filter(|p| granted.as_deref() != Some(p.as_str())) {
            lock(&self.client).signal(&prev, &Signal::ArcadeKbm { granted: false });
        }
        if let Some(m) = &granted {
            lock(&self.client).signal(m, &Signal::ArcadeKbm { granted: true });
        }
        self.arcade_changed();
        Ok(())
    }

    /// Alguém pediu uma vaga no nosso fliperama.
    fn arcade_join_request(self: &Arc<Self>, from: String, candidates: Vec<std::net::SocketAddr>, fingerprint: String) {
        let pads = {
            let mut guard = lock(&self.arcade.host);
            let Some(host) = guard.as_mut() else {
                drop(guard);
                lock(&self.client).signal(&from, &sorry("O fliperama está fechado."));
                return;
            };
            if let Some(seat) = host.seats.get(&from) {
                // Pediu de novo (a conexão caiu): mesma vaga, conexão nova.
                let slot = seat.slot;
                host.expected.insert(fingerprint.clone(), from.clone());
                drop(guard);
                self.send_seat(&from, slot, &candidates, &fingerprint);
                return;
            }
            if host.plugging.contains(&from) {
                return;
            }
            if host.seats.len() + host.plugging.len() >= usize::from(host.info.slots) {
                drop(guard);
                lock(&self.client).signal(&from, &sorry("As vagas do fliperama estão cheias."));
                return;
            }
            host.plugging.push(from.clone());
            host.pads.clone()
        };
        // Plugar espera o Windows aceitar o controle (até alguns segundos): fora da tarefa da rede.
        let inner = self.clone();
        self.runtime.spawn(async move {
            let plugged = tokio::task::spawn_blocking(move || {
                // No primeiro controle depois de instalar o driver, o Windows instala o dele e a
                // espera volta abortada: uma segunda tentativa resolve (docs/fliperama.md).
                pads.plug().or_else(|_| pads.plug())
            })
            .await;
            let pad = match plugged {
                Ok(Ok(pad)) => pad,
                _ => {
                    inner.free_seat(&from);
                    lock(&inner.client).signal(&from, &sorry("Não deu para criar o controle virtual."));
                    return;
                }
            };
            let slot = {
                let mut guard = lock(&inner.arcade.host);
                let Some(host) = guard.as_mut().filter(|h| h.plugging.contains(&from)) else {
                    return; // Fechou ou desistiu no meio: o controle sai com o `pad`.
                };
                host.plugging.retain(|m| m != &from);
                let taken: Vec<u8> = host.seats.values().map(|s| s.slot).collect();
                // O P é a vaga real do XInput + 1 (o host com o controle dele é o P1).
                let slot = pad
                    .user_index()
                    .map(|i| i as u8 + 1)
                    .filter(|s| !taken.contains(s))
                    .unwrap_or_else(|| (2..=4).find(|s| !taken.contains(s)).unwrap_or(2));
                host.expected.insert(fingerprint.clone(), from.clone());
                host.seats.insert(
                    from.clone(),
                    Seat {
                        slot,
                        pad: Arc::new(pad),
                        conn: None,
                    },
                );
                slot
            };
            inner.send_seat(&from, slot, &candidates, &fingerprint);
            inner.arcade_changed();
        });
    }

    fn send_seat(&self, to: &str, slot: u8, candidates: &[std::net::SocketAddr], fingerprint: &str) {
        self.peer.allowed().allow(fingerprint);
        self.peer.punch(candidates);
        self.peer.permit_relay(candidates);
        lock(&self.client).signal(
            to,
            &Signal::ArcadeSeat {
                slot,
                candidates: self.peer.candidates().to_vec(),
                fingerprint: self.peer.fingerprint().to_owned(),
                relay: self.peer.relay_addr(),
            },
        );
    }

    /// Um jogador abriu a conexão do fliperama com a gente.
    pub(super) fn arcade_accept(self: &Arc<Self>, conn: Connection, fingerprint: Option<String>) {
        let found = {
            let mut guard = lock(&self.arcade.host);
            guard.as_mut().and_then(|host| {
                let member = host.expected.get(fingerprint.as_deref()?)?.clone();
                let seat = host.seats.get_mut(&member)?;
                if let Some(old) = seat.conn.replace(conn.clone()) {
                    old.close(0u32.into(), b"trocada");
                }
                Some((member, seat.pad.clone(), host.injector.clone()))
            })
        };
        let Some((member, pad, injector)) = found else {
            conn.close(2u32.into(), b"sem vaga");
            return;
        };
        self.arcade_changed();
        let inner = self.clone();
        self.runtime.spawn(async move {
            serve_player(&inner, &member, &conn, &pad, &injector).await;
            // Caiu: a vaga continua (ele pode voltar), mas o controle fica solto.
            let _ = pad.update(&PadState::default());
            let still = {
                let mut guard = lock(&inner.arcade.host);
                guard
                    .as_mut()
                    .and_then(|h| h.seats.get_mut(&member))
                    .filter(|s| s.conn.as_ref().is_some_and(|c| c.stable_id() == conn.stable_id()))
                    .map(|s| s.conn = None)
                    .is_some()
            };
            if still {
                inner.arcade_changed();
            }
        });
    }

    /// Nosso status (o fliperama aberto, com os jogadores) e a lista para a interface do host.
    fn arcade_changed(&self) {
        let (arcade, seats) = {
            let mut guard = lock(&self.arcade.host);
            match guard.as_mut() {
                Some(host) => {
                    host.info.players = host.seats.keys().cloned().collect();
                    let seats = host
                        .seats
                        .iter()
                        .map(|(member, s)| ArcadeSeatInfo {
                            member_id: member.clone(),
                            slot: s.slot,
                            connected: s.conn.is_some(),
                            kbm: host.kbm.as_deref() == Some(member),
                        })
                        .collect();
                    (Some(host.info.clone()), seats)
                }
                None => (None, Vec::new()),
            }
        };
        lock(&self.me).arcade = arcade;
        self.send_status();
        self.publish_members();
        (self.on_update)(RoomUpdate::ArcadePlayers(seats));
    }

    // ---------------------------------------------------------------- jogador

    /// Pede uma vaga no fliperama de `host` (o vídeo vem pela live dele, que a interface assiste).
    pub(super) fn arcade_join(self: &Arc<Self>, host: &str, map: KeyMap) {
        let previous = lock(&self.arcade.player).replace(Player {
            host: host.to_owned(),
            slot: None,
            conn: None,
            keyboard: Arc::new(Mutex::new(KeyboardPad::new(map))),
            physical: Arc::default(),
            reader: None,
            kbm: false,
            kbm_tx: None,
            kbm_guard: KbmGuard::default(),
            mouse: (false, false),
            stop: Arc::default(),
        });
        if let Some(old) = previous.filter(|p| p.host != host) {
            lock(&self.client).signal(&old.host, &Signal::ArcadeLeave);
        }
        lock(&self.client).signal(
            host,
            &Signal::ArcadeJoin {
                candidates: self.peer.candidates().to_vec(),
                fingerprint: self.peer.fingerprint().to_owned(),
            },
        );
    }

    /// Saímos do fliperama em que jogávamos.
    pub(super) fn arcade_leave(&self) {
        let player = lock(&self.arcade.player).take();
        if let Some(player) = player {
            lock(&self.client).signal(&player.host, &Signal::ArcadeLeave);
            (self.on_update)(RoomUpdate::ArcadeLeft {
                host: player.host.clone(),
                reason: String::new(),
            });
        }
    }

    fn player_gone(&self, host: &str, reason: &str) {
        let mut guard = lock(&self.arcade.player);
        if guard.as_ref().is_some_and(|p| p.host == host) {
            guard.take();
            drop(guard);
            (self.on_update)(RoomUpdate::ArcadeLeft {
                host: host.to_owned(),
                reason: reason.to_owned(),
            });
        }
    }

    /// Ganhamos a vaga: conecta e começa a mandar o controle.
    fn arcade_seated(
        self: &Arc<Self>,
        host: String,
        slot: u8,
        candidates: Vec<std::net::SocketAddr>,
        fingerprint: String,
        relay: Option<std::net::SocketAddr>,
    ) {
        let ok = {
            let mut guard = lock(&self.arcade.player);
            match guard.as_mut().filter(|p| p.host == host) {
                Some(p) => {
                    p.slot = Some(slot);
                    true
                }
                None => false,
            }
        };
        if !ok {
            return;
        }
        let inner = self.clone();
        self.runtime.spawn(async move {
            let conn = match inner
                .peer
                .connect_for(&candidates, relay, &fingerprint, sv_net::Purpose::Arcade)
                .await
            {
                Ok(conn) => conn,
                Err(e) => {
                    inner.player_gone(&host, &format!("Sem conexão com quem hospeda o fliperama: {e}"));
                    return;
                }
            };
            let (physical, keyboard, stop) = {
                let mut guard = lock(&inner.arcade.player);
                let Some(p) = guard.as_mut().filter(|p| p.host == host) else {
                    conn.close(0u32.into(), b"saiu");
                    return;
                };
                p.conn = Some(conn.clone());
                let physical = p.physical.clone();
                let sink = physical.clone();
                p.reader = PadReader::start(move |state| *lock(&sink) = state).ok();
                (physical, p.keyboard.clone(), p.stop.clone())
            };
            (inner.on_update)(RoomUpdate::ArcadeSeat {
                host: host.clone(),
                slot,
            });
            send_pads(&conn, &physical, &keyboard, &stop).await;
            if !stop.load(Ordering::Relaxed) {
                inner.player_gone(&host, "A conexão com o fliperama caiu.");
            }
        });
    }

    /// O host ligou ou desligou nosso controle total.
    fn arcade_kbm(self: &Arc<Self>, host: &str, granted: bool) {
        let mut guard = lock(&self.arcade.player);
        let Some(p) = guard.as_mut().filter(|p| p.host == host) else {
            return;
        };
        p.kbm = granted;
        lock(&p.keyboard).release_all();
        p.kbm_guard = KbmGuard::default();
        p.kbm_tx = None;
        if granted && let Some(conn) = p.conn.clone() {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            p.kbm_tx = Some(tx);
            self.runtime.spawn(send_kbm(conn, rx));
        }
        drop(guard);
        (self.on_update)(RoomUpdate::ArcadeKbm {
            host: host.to_owned(),
            granted,
        });
    }

    /// Uma tecla do jogador (`KeyboardEvent.code`).
    pub(super) fn arcade_key(&self, code: &str, down: bool) {
        let mut guard = lock(&self.arcade.player);
        let Some(p) = guard.as_mut() else { return };
        if p.kbm {
            if let Some((vk, extended)) = vk_from_code(code) {
                p.send_kbm(KbmEvent::Key { vk, extended, down });
            }
        } else {
            lock(&p.keyboard).key(code, down);
        }
    }

    /// Movimento do mouse (ponteiro preso na janela do fliperama).
    pub(super) fn arcade_mouse_move(&self, dx: f32, dy: f32) {
        let mut guard = lock(&self.arcade.player);
        let Some(p) = guard.as_mut() else { return };
        if p.kbm {
            let clamp = |v: f32| v.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
            p.send_kbm(KbmEvent::Move {
                dx: clamp(dx),
                dy: clamp(dy),
            });
        } else {
            lock(&p.keyboard).mouse_move(dx, dy);
        }
    }

    pub(super) fn arcade_mouse_button(&self, button: MouseButton, down: bool) {
        let mut guard = lock(&self.arcade.player);
        let Some(p) = guard.as_mut() else { return };
        if p.kbm {
            p.send_kbm(KbmEvent::Button { button, down });
            return;
        }
        match button {
            MouseButton::Left => p.mouse.0 = down,
            MouseButton::Right => p.mouse.1 = down,
            MouseButton::Middle => return,
        }
        let (left, right) = p.mouse;
        lock(&p.keyboard).mouse_buttons(left, right);
    }

    pub(super) fn arcade_wheel(&self, delta: i16) {
        let mut guard = lock(&self.arcade.player);
        if let Some(p) = guard.as_mut().filter(|p| p.kbm) {
            p.send_kbm(KbmEvent::Wheel { delta });
        }
    }

    /// A janela do fliperama perdeu o foco: solta tudo (nada fica preso no jogo).
    pub(super) fn arcade_release(&self) {
        let mut guard = lock(&self.arcade.player);
        let Some(p) = guard.as_mut() else { return };
        lock(&p.keyboard).release_all();
        p.mouse = (false, false);
        for event in p.kbm_guard.release_all() {
            if let Some(tx) = &p.kbm_tx {
                let _ = tx.send(event);
            }
        }
    }

    pub(super) fn arcade_set_keymap(&self, map: KeyMap) {
        if let Some(p) = lock(&self.arcade.player).as_ref() {
            lock(&p.keyboard).set_map(map);
        }
    }

    // ---------------------------------------------------------------- sinais e saídas

    /// Sinais do fliperama (de quem joga ou de quem hospeda). Devolve `false` se não for um deles.
    pub(super) fn arcade_signal(self: &Arc<Self>, from: &str, signal: Signal) -> Result<(), Signal> {
        match signal {
            Signal::ArcadeJoin {
                candidates,
                fingerprint,
            } => self.arcade_join_request(from.to_owned(), candidates, fingerprint),
            Signal::ArcadeSeat {
                slot,
                candidates,
                fingerprint,
                relay,
            } => self.arcade_seated(from.to_owned(), slot, candidates, fingerprint, relay),
            Signal::ArcadeDeny { reason } => self.player_gone(from, &reason),
            Signal::ArcadeLeave => {
                // De quem joga: saiu da nossa vaga. De quem hospeda: tirou a gente ou fechou.
                if self.free_seat(from) {
                    self.arcade_changed();
                }
                self.player_gone(from, "Quem hospeda fechou o fliperama ou tirou você dele.");
            }
            Signal::ArcadeKbm { granted } => self.arcade_kbm(from, granted),
            other => return Err(other),
        }
        Ok(())
    }

    /// Alguém saiu da sala: libera a vaga dele, ou, se era quem hospedava, saímos do fliperama.
    pub(super) fn arcade_member_left(&self, member: &str) {
        if self.free_seat(member) {
            self.arcade_changed();
        }
        self.player_gone(member, "Quem hospeda saiu da sala.");
    }
}

impl Player {
    fn send_kbm(&mut self, event: KbmEvent) {
        // O mesmo filtro do host, já aqui: Windows e os atalhos do sistema nem saem do PC.
        if let Some(event) = self.kbm_guard.filter(event)
            && let Some(tx) = &self.kbm_tx
        {
            let _ = tx.send(event);
        }
    }
}

/// Host: aplica os estados do controle e, com o controle total, o teclado e o mouse do jogador.
async fn serve_player(
    inner: &Arc<Inner>,
    member: &str,
    conn: &Connection,
    pad: &Arc<VirtualPad>,
    injector: &Arc<Mutex<Option<Injector>>>,
) {
    let kbm_task = {
        let conn = conn.clone();
        let injector = injector.clone();
        let inner = Arc::downgrade(inner);
        let member = member.to_owned();
        tokio::spawn(async move {
            while let Ok(mut recv) = conn.accept_uni().await {
                let injector = injector.clone();
                let inner = inner.clone();
                let member = member.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; KBM_EVENT_LEN];
                    while recv.read_exact(&mut buf).await.is_ok() {
                        // Só o jogador com o controle total; os outros eventos são ignorados.
                        let holder = inner
                            .upgrade()
                            .and_then(|i| lock(&i.arcade.host).as_ref().and_then(|h| h.kbm.clone()));
                        if holder.as_deref() != Some(member.as_str()) {
                            continue;
                        }
                        if let Some(event) = KbmEvent::decode(&buf)
                            && let Some(injector) = lock(&injector).as_mut()
                        {
                            injector.handle(event);
                        }
                    }
                });
            }
        })
    };
    let mut last: Option<u32> = None;
    let mut released = true;
    loop {
        match tokio::time::timeout(SILENCE_RELEASE, conn.read_datagram()).await {
            Ok(Ok(datagram)) => {
                if let Some((seq, state)) = decode_pad(&datagram)
                    && is_newer(seq, last)
                {
                    last = Some(seq);
                    released = state == PadState::default();
                    let _ = pad.update(&state);
                }
            }
            // Calado por 1 s: solta o controle (o jogo não fica com o botão preso).
            Err(_) => {
                if !released {
                    released = true;
                    let _ = pad.update(&PadState::default());
                }
            }
            Ok(Err(_)) => break,
        }
    }
    kbm_task.abort();
}

/// Jogador: manda o controle (físico + teclado e mouse) quando muda e, parado, a cada 100 ms.
async fn send_pads(
    conn: &Connection,
    physical: &Arc<Mutex<PadState>>,
    keyboard: &Arc<Mutex<KeyboardPad>>,
    stop: &Arc<AtomicBool>,
) {
    let mut seq: u32 = 0;
    let mut last: Option<PadState> = None;
    let mut last_sent = Instant::now();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous_tick = Instant::now();
    while !stop.load(Ordering::Relaxed) && conn.close_reason().is_none() {
        ticker.tick().await;
        let now = Instant::now();
        let state = {
            let mut kb = lock(keyboard);
            kb.tick(now - previous_tick);
            (*lock(physical)).merge(kb.state())
        };
        previous_tick = now;
        if last != Some(state) || now - last_sent >= RESEND_EVERY {
            seq = seq.wrapping_add(1);
            let datagram = encode_pad(seq, &state);
            if conn.send_datagram(bytes::Bytes::copy_from_slice(&datagram)).is_err() {
                break;
            }
            last = Some(state);
            last_sent = now;
        }
    }
}

/// Jogador com o controle total: manda os eventos de teclado e mouse em ordem, sem perder nenhum.
async fn send_kbm(conn: Connection, mut events: tokio::sync::mpsc::UnboundedReceiver<KbmEvent>) {
    let Ok(mut send) = conn.open_uni().await else { return };
    while let Some(event) = events.recv().await {
        if send.write_all(&event.encode()).await.is_err() {
            return;
        }
    }
    let _ = send.finish();
}
