import { invoke } from "@tauri-apps/api/core";
import { AppWindow, Download, Gamepad2, Gauge, Keyboard, Loader2, UserX } from "lucide-react";
import { useEffect, useState } from "react";
import { memberPhoto, useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { Avatar } from "../ui/Avatar";
import { Modal } from "../ui/Modal";
import { Switch } from "../ui/Switch";
import { type DriverInfo, useArcadeStore } from "./arcade-store";

/** Quantos jogadores um fliperama aceita (P2 a P4). */
const SLOT_CHOICES = [1, 2, 3];

/**
 * O fliperama de quem transmite (ADR 0023 e 0024): abre sobre a live atual, mostra quem joga em
 * cada P, tira alguém e liga o controle total de teclado e mouse para um jogador por vez.
 */
export function ArcadeHostDialog() {
  const open = useArcadeStore((s) => s.hostDialog);
  const setOpen = useArcadeStore((s) => s.setHostDialog);
  const me = useRoomStore((s) => s.members[0] ?? null);
  const arcadeOpen = !!me?.arcade;

  return (
    <Modal
      open={open}
      onOpenChange={setOpen}
      title="Fliperama"
      description="Seu jogo com vários controles, para a galera da call jogar junto como P2, P3 e P4."
      width={480}
    >
      {/* Sem rodapé: o X do canto já fecha. */}
      {open && <div className="pb-6">{arcadeOpen ? <Running /> : <Setup />}</div>}
    </Modal>
  );
}

/** Antes de abrir: o driver de controle e quantas vagas. */
function Setup() {
  const [driver, setDriver] = useState<DriverInfo | null>(null);
  const [slots, setSlots] = useState(3);
  const [reserveP1, setReserveP1] = useState(true);
  const [busy, setBusy] = useState(false);
  const live = useRoomStore((s) => s.members[0]?.live ?? false);
  const openArcade = useArcadeStore((s) => s.open);

  useEffect(() => {
    void invoke<DriverInfo>("arcade_driver").then(setDriver, () =>
      setDriver({ status: "unavailable", detail: null, version: null, removable: false }),
    );
  }, []);

  if (!driver) {
    return (
      <p className="flex items-center justify-center gap-2 py-6 text-sm text-text-muted">
        <Loader2 size={16} className="animate-spin" /> Conferindo o driver de controle…
      </p>
    );
  }
  if (driver.status !== "installed") return <DriverMissing driver={driver} />;

  return (
    <div className="flex flex-col gap-4 text-left">
      {!live && (
        <p role="alert" className="rounded-md bg-warning/15 px-3 py-2 text-sm">
          Comece a transmitir a janela do jogo antes de abrir o fliperama: os jogadores veem o jogo pela sua live.
        </p>
      )}
      <SmoothnessHint />
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-xs font-semibold tracking-wide text-text-muted uppercase">Vagas</legend>
        <div role="radiogroup" aria-label="Vagas" className="flex gap-2">
          {SLOT_CHOICES.map((n) => (
            <button
              key={n}
              type="button"
              role="radio"
              aria-checked={slots === n}
              onClick={() => setSlots(n)}
              className={`flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors ${
                slots === n ? "bg-brand text-white" : "bg-surface-0 text-text-muted hover:text-text"
              }`}
            >
              {n === 1 ? "1 jogador" : `${n} jogadores`}
            </button>
          ))}
        </div>
        <p className="text-xs text-text-faint">
          Cada jogador vira um controle de Xbox no seu PC. Você continua sendo o P1 com o seu controle.
        </p>
      </fieldset>
      <Switch
        checked={reserveP1}
        onChange={setReserveP1}
        label="Guardar o P1 para mim"
        hint="Sem controle ligado no seu PC, um controle parado fica com o P1, e os amigos viram P2 a P4 no jogo. Desligue se o jogo já trata o teclado como P1 e o primeiro controle como P2."
      />
      <button
        type="button"
        disabled={!live || busy}
        onClick={async () => {
          setBusy(true);
          await openArcade(slots, null, reserveP1);
          setBusy(false);
        }}
        className="flex items-center justify-center gap-2 rounded-md bg-brand px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-brand-hover disabled:opacity-50"
      >
        {busy ? <Loader2 size={16} className="animate-spin" /> : <Gamepad2 size={18} />}
        Abrir fliperama
      </button>
    </div>
  );
}

/**
 * Abaixo de 60 fps, cada quadro do jogo espera até 33 ms para sair (a 30 fps), e o jogador sente
 * o atraso entre apertar e ver. Avisa e troca com um clique (a live continua no ar).
 */
function SmoothnessHint() {
  const quality = useStreamStore((s) => s.quality);
  const changeQuality = useStreamStore((s) => s.changeQuality);
  if (quality.fps >= 60) return null;
  return (
    <div className="flex items-center gap-3 rounded-md bg-warning/15 px-3 py-2">
      <Gauge size={18} className="shrink-0 text-warning" />
      <p className="min-w-0 flex-1 text-xs">
        Sua live está a {quality.fps} fps. Para jogar com fluidez, transmita a 60 fps: cada quadro sai mais rápido e o
        atraso entre apertar e ver cai.
      </p>
      <button
        type="button"
        onClick={() => void changeQuality({ ...quality, fps: 60 })}
        className="shrink-0 rounded-md bg-brand px-3 py-1.5 text-xs font-semibold text-white hover:bg-brand-hover"
      >
        Mudar para 60 fps
      </button>
    </div>
  );
}

/** Sem o ViGEmBus: explica e leva ao instalador oficial (o app não embute o driver). */
export function DriverMissing(props: { driver: DriverInfo }) {
  const incompatible = props.driver.status === "incompatible";
  return (
    <div className="flex flex-col gap-3 text-left">
      <p className="text-sm">
        {incompatible
          ? "O driver de controle instalado está numa versão que o SquadView não reconhece."
          : "Para ser o host do fliperama, o seu PC precisa do driver de controle virtual (ViGEmBus). Só quem hospeda instala; quem joga não precisa de nada."}
      </p>
      <p className="text-xs text-text-faint">
        A instalação pede permissão de administrador uma vez. Dá para remover quando quiser em Configurações ›
        Fliperama.
      </p>
      <button
        type="button"
        onClick={() => void invoke("arcade_install_driver").catch(() => {})}
        className="flex items-center justify-center gap-2 rounded-md bg-brand px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-brand-hover"
      >
        <Download size={18} />
        Baixar o instalador oficial
      </button>
      <p className="text-xs text-text-faint">Depois de instalar, abra o fliperama de novo.</p>
    </div>
  );
}

/** Aberto: quem joga em cada P, tirar alguém e o controle total. */
function Running() {
  const seats = useArcadeStore((s) => s.seats);
  const { close, kick, grantKbm } = useArcadeStore();
  const members = useRoomStore((s) => s.members);
  const arcade = useRoomStore((s) => s.members[0]?.arcade);
  const free = Math.max(0, (arcade?.slots ?? 0) - seats.length);
  const gameFocused = useArcadeStore((s) => s.gameFocused);

  return (
    <div className="flex flex-col gap-4 text-left">
      <SmoothnessHint />
      {gameFocused === false && seats.some((s) => s.connected) && (
        <p role="alert" className="flex items-start gap-2 rounded-md bg-warning/15 px-3 py-2 text-xs">
          <AppWindow size={16} className="mt-0.5 shrink-0" />O jogo não está em primeiro plano. Muitos jogos ignoram os
          controles dos amigos assim: deixe a janela do jogo na frente enquanto jogam.
        </p>
      )}
      <p className="text-sm text-text-muted">
        {arcade?.game ? `${arcade.game} · ` : ""}
        {free > 0 ? `${free} ${free === 1 ? "vaga livre" : "vagas livres"}` : "Vagas cheias"}. A galera entra em “Entrar
        no fliperama”, no botão direito no seu nome ou nos “...” da sua live.
      </p>
      <ul aria-label="Jogadores do fliperama" className="flex flex-col gap-2">
        {seats.length === 0 && <li className="text-sm text-text-faint">Ninguém entrou ainda.</li>}
        {seats.map((seat) => {
          const member = members.find((m) => m.id === seat.memberId);
          const name = member?.name ?? "Alguém";
          return (
            <li key={seat.memberId} className="flex flex-col gap-2 rounded-md bg-surface-0 px-3 py-2">
              <div className="flex items-center gap-2">
                <span className="rounded bg-brand px-1.5 py-0.5 text-xs font-bold text-white">P{seat.slot}</span>
                <Avatar name={name} size={24} src={member ? memberPhoto(member, false, null, 48) : undefined} />
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{name}</span>
                <span className={`text-xs ${seat.connected ? "text-online" : "text-text-faint"}`}>
                  {seat.connected ? "Conectado" : "Conectando…"}
                </span>
                <button
                  type="button"
                  aria-label={`Tirar ${name} do fliperama`}
                  onClick={() => void kick(seat.memberId)}
                  className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-live"
                >
                  <UserX size={16} />
                </button>
              </div>
              {seat.xinput === false && (
                <p role="alert" className="rounded-md bg-warning/15 px-2 py-1.5 text-xs">
                  O Windows não deu vaga a este controle, e o jogo não o enxerga. O Windows aceita 4 controles: desligue
                  um do seu PC, tire {name} e peça para entrar de novo.
                </p>
              )}
              <Switch
                checked={seat.kbm}
                onChange={(on) => void grantKbm(on ? seat.memberId : null)}
                label="Controle total (teclado e mouse)"
                hint="Para jogos que só aceitam teclado e mouse. Só um jogador por vez, só com o jogo em primeiro plano."
              />
            </li>
          );
        })}
      </ul>
      {seats.some((s) => s.kbm) && (
        <p className="flex items-start gap-2 rounded-md bg-warning/15 px-3 py-2 text-xs">
          <Keyboard size={16} className="mt-0.5 shrink-0" />
          Com o controle total, o teclado e o mouse desse jogador mexem no seu PC dentro do jogo. Corte na hora com
          Ctrl+Alt+F12 (funciona com o jogo em tela cheia).
        </p>
      )}
      <button
        type="button"
        onClick={() => void close()}
        className="rounded-md bg-live/15 px-4 py-2 text-sm font-semibold text-live transition-colors hover:bg-live hover:text-white"
      >
        Fechar fliperama
      </button>
    </div>
  );
}
