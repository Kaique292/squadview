import { type CSSProperties, type KeyboardEvent, type PointerEvent, useRef, useState } from "react";

/** Cor em HSV: matiz de 0 a 360, saturação e brilho de 0 a 1. */
export interface Hsv {
  h: number;
  s: number;
  v: number;
}

const clamp = (n: number, min = 0, max = 1) => Math.min(max, Math.max(min, n));

/** `#rrggbb` → HSV (`null` fora do formato). */
export function hexToHsv(hex: string): Hsv | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => c / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d > 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
  }
  return { h: (h * 60 + 360) % 360, s: max === 0 ? 0 : d / max, v: max };
}

/** HSV → `#rrggbb`. */
export function hsvToHex({ h, s, v }: Hsv): string {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return `#${[f(5), f(3), f(1)]
    .map((c) =>
      Math.round(c * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/**
 * Seletor de cor no visual do app (o do Windows destoa): a área de saturação e brilho, a faixa de
 * matiz e o código `#rrggbb`. Tudo funciona com o mouse e com as setas do teclado.
 */
export function ColorPicker(props: { value: string; onChange: (hex: string) => void }) {
  // A matiz fica guardada aqui: num cinza ou no preto ela não sai da cor e se perderia.
  const [hsv, setHsv] = useState<Hsv>(() => hexToHsv(props.value) ?? { h: 0, s: 1, v: 1 });
  const [text, setText] = useState(props.value);
  const [seen, setSeen] = useState(props.value);

  // A cor mudou por fora (outra bolinha escolhida): acompanha, sem perder a matiz num cinza.
  if (props.value !== seen) {
    setSeen(props.value);
    const next = hexToHsv(props.value);
    if (next) setHsv(next.s === 0 || next.v === 0 ? { ...next, h: hsv.h } : next);
    setText(props.value);
  }

  function change(next: Hsv) {
    setHsv(next);
    const hex = hsvToHex(next);
    setSeen(hex);
    setText(hex);
    props.onChange(hex);
  }

  function commitText() {
    const value = text.trim().toLowerCase();
    const hex = value.startsWith("#") ? value : `#${value}`;
    const next = hexToHsv(hex);
    if (!next) return setText(props.value);
    setHsv(next.s === 0 || next.v === 0 ? { ...next, h: hsv.h } : next);
    setSeen(hex);
    setText(hex);
    props.onChange(hex);
  }

  const hue = `hsl(${Math.round(hsv.h)} 100% 50%)`;
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-white/[0.06] bg-surface-0 p-3">
      <Area
        label="Saturação e brilho"
        valueText={`saturação ${Math.round(hsv.s * 100)}%, brilho ${Math.round(hsv.v * 100)}%`}
        x={hsv.s}
        y={1 - hsv.v}
        onMove={(x, y) => change({ ...hsv, s: x, v: 1 - y })}
        className="h-36 rounded-md"
        style={{ background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, ${hue})` }}
        thumb={hsvToHex(hsv)}
      />
      <Area
        label="Matiz"
        valueText={`${Math.round(hsv.h)} graus`}
        x={hsv.h / 360}
        onMove={(x) => change({ ...hsv, h: x * 360 })}
        className="h-3 rounded-full"
        style={{
          background: "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)",
        }}
        thumb={hue}
      />
      <div className="flex items-center gap-2">
        <span className="size-8 shrink-0 rounded-md border border-white/10" style={{ background: hsvToHex(hsv) }} />
        <input
          aria-label="Código da cor"
          value={text}
          maxLength={7}
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onBlur={commitText}
          onKeyDown={(e) => e.key === "Enter" && commitText()}
          className="h-8 w-28 rounded-md border border-white/[0.06] bg-surface-1 px-2 font-mono text-sm text-text uppercase outline-none focus:border-brand"
        />
      </div>
    </div>
  );
}

/**
 * Área que se arrasta (2D, ou só na horizontal sem `y`). As setas andam 1% (10% com Shift).
 */
function Area(props: {
  label: string;
  valueText: string;
  x: number;
  y?: number;
  onMove: (x: number, y: number) => void;
  className: string;
  style: CSSProperties;
  thumb: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const twoD = props.y !== undefined;

  function at(e: PointerEvent<HTMLDivElement>) {
    const r = ref.current!.getBoundingClientRect();
    props.onMove(clamp((e.clientX - r.left) / (r.width || 1)), clamp((e.clientY - r.top) / (r.height || 1)));
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const step = e.shiftKey ? 0.1 : 0.01;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, twoD ? -step : 0],
      ArrowDown: [0, twoD ? step : 0],
    };
    const move = moves[e.key];
    if (!move) return;
    e.preventDefault();
    props.onMove(clamp(props.x + move[0]), clamp((props.y ?? 0) + move[1]));
  }

  return (
    <div
      ref={ref}
      role="slider"
      tabIndex={0}
      aria-label={props.label}
      aria-valuetext={props.valueText}
      aria-valuenow={Math.round(props.x * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture?.(e.pointerId);
        at(e);
      }}
      onPointerMove={(e) => e.buttons === 1 && at(e)}
      onKeyDown={onKeyDown}
      className={`relative cursor-crosshair touch-none outline-none focus-visible:ring-2 focus-visible:ring-brand ${props.className}`}
      style={props.style}
    >
      <span
        className="pointer-events-none absolute size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgba(0,0,0,0.4),0_2px_6px_rgba(0,0,0,0.5)]"
        style={{ left: `${props.x * 100}%`, top: twoD ? `${props.y! * 100}%` : "50%", background: props.thumb }}
      />
    </div>
  );
}
