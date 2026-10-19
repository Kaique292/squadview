import { create } from "zustand";

/**
 * Animação de saída do vídeo, como o "Ocultar itens que distraem" do Safari no iOS: a última
 * imagem brilha na borda e se desfaz em poeira que sobe e some, numa varredura da esquerda para
 * a direita. O vídeo de verdade é uma janela nativa; a animação usa a foto do último quadro.
 */

/** Foto parada enquanto o vídeo nativo ainda está por cima (o Rust espera isso antes de fechá-lo). */
export const HOLD_MS = 160;
/** Brilho antes de começar a desfazer. */
export const GLOW_MS = 260;
/** Duração da poeira (depois do brilho). */
export const DUST_MS = 1300;
/** Quanto a poeira pode se afastar da imagem (px), para o canvas ter espaço. */
export const MARGIN = 220;
/** Teto de partículas: acima disso o passo da grade cresce (o efeito continua leve). */
const MAX_PARTICLES = 14_000;

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Particle {
  /** Posição de origem (canto do bloco), relativa à imagem. */
  x: number;
  y: number;
  color: string;
  /** Quando começa a voar, de 0 a 1 do tempo da poeira. */
  delay: number;
  /** Quanto dura o voo, de 0 a 1 do tempo da poeira. */
  life: number;
  /** Deslocamento total do voo (px). */
  dx: number;
  dy: number;
  /** Fase da oscilação lateral. */
  wobble: number;
}

/** Maior retângulo com a proporção da imagem que cabe na área, centralizado (como o vídeo nativo). */
export function fitInside(imageW: number, imageH: number, areaW: number, areaH: number): Rect {
  if (imageW <= 0 || imageH <= 0) return { x: 0, y: 0, width: areaW, height: areaH };
  const scale = Math.min(areaW / imageW, areaH / imageH);
  const width = imageW * scale;
  const height = imageH * scale;
  return { x: (areaW - width) / 2, y: (areaH - height) / 2, width, height };
}

/** Lado do bloco de cada partícula, para não passar do teto. */
export function gridStep(width: number, height: number, max = MAX_PARTICLES): number {
  return Math.max(3, Math.ceil(Math.sqrt((width * height) / max)));
}

/**
 * Monta as partículas a partir dos pixels da imagem (`rgba`, `width` × `height`). Cada bloco de
 * `step` px vira uma partícula com a cor do centro dele. `random` é injetável para os testes.
 */
export function makeParticles(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  step: number,
  random: () => number = Math.random,
): Particle[] {
  const particles: Particle[] = [];
  for (let y = 0; y < height; y += step) {
    for (let x = 0; x < width; x += step) {
      const cx = Math.min(width - 1, x + (step >> 1));
      const cy = Math.min(height - 1, y + (step >> 1));
      const i = (cy * width + cx) * 4;
      if (rgba[i + 3] < 16) continue;
      // A varredura anda da esquerda para a direita, com um pouco de bagunça.
      const delay = (x / width) * 0.55 + random() * 0.18;
      particles.push({
        x,
        y,
        color: `rgb(${rgba[i]},${rgba[i + 1]},${rgba[i + 2]})`,
        delay,
        life: 0.3 + random() * 0.22,
        dx: 60 + random() * 160,
        dy: -(50 + random() * 170),
        wobble: random() * Math.PI * 2,
      });
    }
  }
  return particles;
}

/** Onde a partícula está em `t` (0 a 1 do tempo da poeira): parada, voando ou já sumiu. */
export function particleAt(
  p: Particle,
  t: number,
): "waiting" | "gone" | { x: number; y: number; alpha: number; scale: number } {
  const u = (t - p.delay) / p.life;
  if (u < 0) return "waiting";
  if (u >= 1) return "gone";
  // Começa devagar e acelera, como poeira levada pelo vento.
  const ease = u * u;
  return {
    x: p.x + p.dx * ease + Math.sin(p.wobble + u * 6) * 4 * u,
    y: p.y + p.dy * ease,
    alpha: 1 - u ** 1.3,
    scale: 1 - 0.65 * u,
  };
}

interface DissolveState {
  effect: { id: number; src: string; rect: Rect } | null;
  play: (src: string, rect?: Rect | null) => void;
  done: (id: number) => void;
}

let nextId = 1;
/** Último lugar do vídeo na tela (px da interface), guardado pelo `useVideoSlot`. */
let lastVideoRect: Rect | null = null;

export function rememberVideoRect(rect: Rect) {
  if (rect.width > 0 && rect.height > 0) lastVideoRect = rect;
}

export const useDissolveStore = create<DissolveState>((set, get) => ({
  effect: null,
  play(src, rect = lastVideoRect) {
    if (!rect) return;
    set({ effect: { id: nextId++, src, rect } });
  },
  done(id) {
    if (get().effect?.id === id) set({ effect: null });
  },
}));

/** Toca a animação de saída com a foto do último quadro (se houver). */
export function playVideoExit(photo: string | null | undefined) {
  if (photo) useDissolveStore.getState().play(photo);
}
