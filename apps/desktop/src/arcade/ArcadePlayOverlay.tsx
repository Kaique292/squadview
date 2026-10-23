import { invoke } from "@tauri-apps/api/core";
import { Gamepad2, Keyboard, MousePointer2 } from "lucide-react";
import { type KeyboardEvent, type MouseEvent, type WheelEvent, useEffect, useRef, useState } from "react";
import { useArcadeStore } from "./arcade-store";

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
  // Movimento do mouse somado até o próximo quadro da página (uma chamada ao Rust por quadro).
  const pending = useRef({ dx: 0, dy: 0, frame: 0 });

  const mine = playing?.hostId === props.hostId && playing.slot !== null;

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
      cancelAnimationFrame(pending.current.frame);
      if (element && document.pointerLockElement === element) document.exitPointerLock();
    };
  }, [mine]);

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
    if (!captured) return;
    const p = pending.current;
    p.dx += e.movementX;
    p.dy += e.movementY;
    if (p.frame) return;
    p.frame = requestAnimationFrame(() => {
      const { dx, dy } = pending.current;
      pending.current = { dx: 0, dy: 0, frame: 0 };
      if (dx || dy) void invoke("arcade_mouse", { dx, dy }).catch(() => {});
    });
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
        <span className="absolute bottom-12 left-1/2 flex -translate-x-1/2 items-center gap-1.5 rounded-md bg-black/70 px-2 py-1 text-xs text-white/90">
          <MousePointer2 size={13} />
          {hint}
        </span>
      )}
    </div>
  );
}
