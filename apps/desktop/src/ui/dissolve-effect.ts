import { create } from "zustand";

/**
 * Animação de saída do vídeo, como o "Ocultar itens que distraem" do Safari no iOS: a última
 * imagem escurece e some por inteiro enquanto vira uma nuvem de poeira fina e brilhante, com as
 * cores da imagem, que sobe para a direita e se apaga. O vídeo de verdade é uma janela nativa; a
 * animação usa a foto do último quadro.
 */

/** Foto parada enquanto o vídeo nativo ainda está por cima (o Rust espera isso antes de fechá-lo). */
export const HOLD_MS = 160;
/** Duração da poeira. */
export const DUST_MS = 1400;
/** Quanto a poeira pode se afastar da imagem (px), para o canvas ter espaço. */
export const MARGIN = 220;
/** Lado de cada grão de poeira (px da interface): fino, para ler como poeira e não como pixel. */
export const GRAIN = 1.8;
/** Teto de partículas: acima disso o passo da grade cresce (o efeito continua leve). */
const MAX_PARTICLES = 24_000;
/** Até quando (fração da poeira) a imagem leva para sumir. */
const IMAGE_FADE = 0.45;
/** Quanto cada grão clareia em direção ao branco (o brilho da poeira do iOS). */
const SHINE = 0.55;

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Particle {
  /** Posição de origem do grão, relativa à imagem (px físicos). */
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

/**
 * Onde fica o canvas (px da interface) e o tamanho dele em pixels físicos. Posição e tamanho caem
 * em pixels físicos inteiros: assim o navegador não reamostra o canvas com a escala do Windows
 * (125%, 150%), o que borrava a foto e desenhava uma grade de frestas por cima dela.
 */
export function canvasBox(rect: Rect, dpr: number) {
  const margin = Math.round(MARGIN * dpr);
  const pixelWidth = Math.round(rect.width * dpr) + margin;
  const pixelHeight = Math.round(rect.height * dpr) + margin;
  return {
    margin,
    pixelWidth,
    pixelHeight,
    left: Math.round(rect.x * dpr) / dpr,
    top: (Math.round(rect.y * dpr) - margin) / dpr,
    width: pixelWidth / dpr,
    height: pixelHeight / dpr,
  };
}

/** Lado do bloco da imagem que vira um grão, para não passar do teto. */
export function gridStep(width: number, height: number, max = MAX_PARTICLES): number {
  return Math.max(3, Math.ceil(Math.sqrt((width * height) / max)));
}

/**
 * Monta as partículas a partir dos pixels da imagem (`rgba`, `width` × `height`). Cada bloco de
 * `step` px vira um grão num ponto ao acaso dentro dele, com a cor do centro do bloco um pouco
 * mais clara. Todos saem quase juntos (sem varredura). `random` é injetável para os testes.
 */
export function makeParticles(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  step: number,
  random: () => number = Math.random,
): Particle[] {
  const particles: Particle[] = [];
  const shine = (c: number) => Math.round(c + (255 - c) * SHINE);
  for (let y = 0; y < height; y += step) {
    for (let x = 0; x < width; x += step) {
      const cx = Math.min(width - 1, x + (step >> 1));
      const cy = Math.min(height - 1, y + (step >> 1));
      const i = (cy * width + cx) * 4;
      if (rgba[i + 3] < 16) continue;
      const delay = random() * 0.35;
      particles.push({
        x: Math.min(width - 1, x + random() * step),
        y: Math.min(height - 1, y + random() * step),
        color: `rgb(${shine(rgba[i])},${shine(rgba[i + 1])},${shine(rgba[i + 2])})`,
        delay,
        life: Math.min(1 - delay, 0.45 + random() * 0.25),
        // Dentro da margem do canvas (MARGIN), para nenhum grão ser cortado na borda.
        dx: 60 + random() * 150,
        dy: -(40 + random() * 160),
        wobble: random() * Math.PI * 2,
      });
    }
  }
  return particles;
}

/** Quanto da imagem ainda aparece em `t` (0 a 1 do tempo da poeira): some suave, por inteiro. */
export function imageAlpha(t: number): number {
  const u = Math.min(1, Math.max(0, t / IMAGE_FADE));
  return 1 - u * u * (3 - 2 * u);
}

/** Onde o grão está em `t` (0 a 1 do tempo da poeira): ainda na imagem, voando ou já sumiu. */
export function particleAt(p: Particle, t: number): "waiting" | "gone" | { x: number; y: number; alpha: number } {
  const u = (t - p.delay) / p.life;
  if (u < 0) return "waiting";
  if (u >= 1) return "gone";
  // Começa devagar e acelera, como poeira levada pelo vento.
  const ease = u ** 1.5;
  return {
    x: p.x + p.dx * ease + Math.sin(p.wobble + u * 6) * 6 * u,
    y: p.y + p.dy * ease,
    // Acende rápido ao se soltar e vai se apagando.
    alpha: u < 0.12 ? u / 0.12 : 1 - ((u - 0.12) / 0.88) ** 2,
  };
}
interface DissolveState {
  effect: { id: number; src: string; rect: Rect } | null;
  play: (src: string, rect?: Rect | null) => void;
  done: (id: number) => void;
}

let nextId = 1;
/** Último lugar de cada vídeo na tela (px da interface), guardado pelo `useVideoSlot`. */
const lastVideoRects = new Map<string, Rect>();
/** A chave da nossa live (a mesma do `video-pointer`; repetida aqui para não criar um ciclo). */
const OWN = "transmissao:minha";

export function rememberVideoRect(rect: Rect, of = OWN) {
  if (rect.width > 0 && rect.height > 0) lastVideoRects.set(of, rect);
}

export const useDissolveStore = create<DissolveState>((set, get) => ({
  effect: null,
  play(src, rect = lastVideoRects.get(OWN) ?? null) {
    if (!rect) return;
    set({ effect: { id: nextId++, src, rect } });
  },
  done(id) {
    if (get().effect?.id === id) set({ effect: null });
  },
}));

/** Toca a animação de saída com a foto do último quadro do vídeo `of` (se houver). */
export function playVideoExit(photo: string | null | undefined, of = OWN) {
  if (photo) useDissolveStore.getState().play(photo, lastVideoRects.get(of) ?? null);
}
