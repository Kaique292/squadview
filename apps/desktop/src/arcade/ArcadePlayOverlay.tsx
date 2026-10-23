import { invoke } from "@tauri-apps/api/core";
import { Gamepad2, Keyboard, MousePointer2 } from "lucide-react";
import { type KeyboardEvent, type MouseEvent, type WheelEvent, useEffect, useRef, useState } from "react";
import { type KeyMap, useArcadeStore } from "./arcade-store";
import { keyLegend } from "./key-legend";

/** O navegador tem o evento cru do mouse (o WebView2 do app tem; o jsdom dos testes, não). */
const RAW_POINTER = typeof window !== "undefined" && "onpointerrawupdate" in window;

/** Movimento do mouse ainda não mandado ao Rust, e se uma chamada está a caminho. */
interface PendingMove {
  dx: number;
  dy: number;
  inflight: boolean;
}

/** Soma o movimento e manda na hora (ou assim que a chamada anterior voltar). */
function pushMove(p: PendingMove, dx: number, dy: number) {
  p.dx += dx;
  p.dy += dy;
  flushMove(p);
}

function flushMove(p: PendingMove) {
  if (p.inflight || (!p.dx && !p.dy)) return;
  const { dx, dy } = p;
  p.dx = 0;
  p.dy = 0;
  p.inflight = true;
  void invoke("arcade_mouse", { dx, dy })
    .catch(() => {})
    .finally(() => {
      p.inflight = false;
      flushMove(p);
    });
}

/** Botão do mouse do DOM → o nome que o Rust entende. */
const BUTTONS: Record<number, "left" | "middle" | "right"> = { 0: "left", 1: "middle", 2: "right" };

/**
 * Por cima da live de quem hospeda o fliperama em que jogamos (ADR 0023 e 0024): o selo do P e a
 * captura do teclado e do mouse. Clicar no jogo prende o ponteiro; daí em diante as teclas viram o
 * controle (ou vão cruas, com o controle total) e o mouse vira o analógico direito. Esc solta o
 * ponteiro; perder o foco solta tudo, para nada ficar apertado no jogo. O controle físico do
 * jogador é lido pelo Rust, sem depender do foco.
 */
export function ArcadePlayOverlay(props: { hostId: string; compact?: boolean }) {
  const playing = useArcadeStore((s) => s.playing);
  const area = useRef<HTMLDivElement>(null);
  const [captured, setCaptured] = useState(false);
  // Movimento do mouse ainda não mandado ao Rust. Vai na hora; só se junta enquanto a chamada
  // anterior não voltou (antes esperava o próximo quadro da página: até 16 ms a mais).
  const pending = useRef<PendingMove>({ dx: 0, dy: 0, inflight: false });

  const mine = playing?.hostId === props.hostId && playing.slot !== null;
  // As teclas que viram o controle, para mostrar ao jogador (antes ele não sabia o que apertar).
  const [legend, setLegend] = useState<string | null>(null);
  useEffect(() => {
    if (!mine) return;
    void invoke<KeyMap>("arcade_keymap").then(
      (map) => setLegend(map?.keys ? keyLegend(map) : null),
      () => setLegend(null),
    );
  }, [mine]);

  useEffect(() => {
    if (!mine) return;
    const element = area.current;
    const onLockChange = () => {
      const locked = document.pointerLockElement === element;
      setCaptured(locked);
      if (!locked) void invoke("arcade_release").catch(() => {});
    };
    document.addEventListener("pointerlockchange", onLockChange);
    return () => {
      document.removeEventListener("pointerlockchange", onLockChange);
      if (element && document.pointerLockElement === element) document.exitPointerLock();
    };
  }, [mine]);

  // O movimento cru do mouse (`pointerrawupdate`, no WebView2): chega assim que o mouse manda,
  // sem esperar o quadro da página como o `mousemove`.
  useEffect(() => {
    const element = area.current;
    if (!captured || !element || !RAW_POINTER) return;
    const onRaw = (e: Event) => pushMove(pending.current, (e as PointerEvent).movementX, (e as PointerEvent).movementY);
    element.addEventListener("pointerrawupdate", onRaw);
    return () => element.removeEventListener("pointerrawupdate", onRaw);
  }, [captured]);

  if (!mine || !playing) return null;

  function capture(e: MouseEvent<HTMLDivElement>) {
    e.stopPropagation();
    area.current?.focus();
    if (document.pointerLockElement !== area.current) void area.current?.requestPointerLock();
  }

  function onKey(e: KeyboardEvent<HTMLDivElement>, down: boolean) {
    // Esc fica com o navegador (solta o ponteiro); repetições da tecla segurada não mudam nada.
    if (e.code === "Escape" || e.repeat) return;
    e.preventDefault();
    e.stopPropagation();
    void invoke("arcade_key", { code: e.code, down }).catch(() => {});
  }

  function onMove(e: MouseEvent<HTMLDivElement>) {
    // Com o evento cru disponível, o movimento já foi por ele.
    if (!captured || RAW_POINTER) return;
    pushMove(pending.current, e.movementX, e.movementY);
  }

  function onButton(e: MouseEvent<HTMLDivElement>, down: boolean) {
    if (!captured) return;
    const button = BUTTONS[e.button];
    if (!button) return;
    e.preventDefault();
    e.stopPropagation();
    void invoke("arcade_mouse_button", { button, down }).catch(() => {});
  }

  function onWheel(e: WheelEvent<HTMLDivElement>) {
    if (!captured || !playing?.kbm || e.deltaY === 0) return;
    void invoke("arcade_wheel", { delta: e.deltaY < 0 ? 120 : -120 }).catch(() => {});
  }

  const hint = captured
    ? playing.kbm
      ? "Teclado e mouse vão direto para o jogo · Esc solta o mouse"
      : "Teclado e mouse viram controle · Esc solta o mouse"
    : "Clique no jogo para jogar com teclado e mouse";

  return (
    <div
      ref={area}
      role="application"
      aria-label={`Jogando como P${playing.slot}`}
      tabIndex={-1}
      onClick={capture}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => onKey(e, true)}
      onKeyUp={(e) => onKey(e, false)}
      onMouseMove={onMove}
      onMouseDown={(e) => onButton(e, true)}
      onMouseUp={(e) => onButton(e, false)}
      onContextMenu={(e) => captured && e.preventDefault()}
      onWheel={onWheel}
      onBlur={() => void invoke("arcade_release").catch(() => {})}
      className={`absolute inset-0 outline-none ${captured ? "cursor-none" : "cursor-pointer"}`}
    >
      <span
        className={`absolute top-2.5 left-2.5 flex items-center gap-1.5 rounded-md bg-black/70 font-semibold text-white ${
          props.compact ? "px-1.5 py-0.5 text-xs" : "px-2 py-1 text-sm"
        }`}
      >
        <Gamepad2 size={props.compact ? 13 : 16} className="text-brand" />
        Você é o P{playing.slot}
        {playing.kbm && (
          <Keyboard size={props.compact ? 12 : 14} aria-label="Controle total" className="text-warning" />
        )}
      </span>
      {!props.compact && (
        <span className="absolute bottom-12 left-1/2 flex max-w-[90%] -translate-x-1/2 flex-col items-center gap-0.5 rounded-md bg-black/70 px-2 py-1 text-center text-xs text-white/90">
          <span className="flex items-center gap-1.5">
            <MousePointer2 size={13} />
            {hint}
          </span>
          {legend && !playing.kbm && <span className="text-white/70">{legend}</span>}
        </span>
      )}
    </div>
  );
}
