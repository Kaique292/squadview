/**
 * O vídeo é desenhado por baixo da interface (ADR 0021): a página só o mostra onde ela é
 * transparente. Cada quadro com imagem registra aqui onde está (pixels da interface, relativos à
 * janela), e os fundos da página (`Backdrop`) abrem um buraco com a forma do quadro ali.
 */
export interface SeeThrough {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Raio dos cantos do quadro. */
  radius: number;
}

/**
 * Quanto o buraco fica para dentro do quadro: as bordas suavizadas do fundo e do vídeo não caem
 * exatamente no mesmo pixel, e numa folga o que aparece é o que está atrás da janela.
 */
export const SEE_THROUGH_INSET = 1;

const areas = new Map<string, SeeThrough>();
const listeners = new Set<() => void>();

function same(a: SeeThrough | undefined, b: SeeThrough | null) {
  if (!a || !b) return !a && !b;
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height && a.radius === b.radius;
}

/** Registra (ou tira, com `null`) a área onde o vídeo `key` aparece. */
export function setSeeThrough(key: string, area: SeeThrough | null) {
  if (same(areas.get(key), area)) return;
  if (area && area.width > 0 && area.height > 0) areas.set(key, area);
  else if (!areas.delete(key)) return;
  listeners.forEach((fn) => fn());
}

/** As áreas registradas agora. */
export function seeThroughAreas(): SeeThrough[] {
  return [...areas.values()];
}

/** Avisa quando as áreas mudam; devolve a função que deixa de avisar. */
export function onSeeThroughChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

const n = (v: number) => Math.round(v * 100) / 100;

/** Um retângulo de cantos arredondados como trecho de `path()`. */
function roundedRect(x: number, y: number, w: number, h: number, radius: number) {
  const r = Math.max(0, Math.min(radius, w / 2, h / 2));
  if (r === 0) return `M${n(x)} ${n(y)}H${n(x + w)}V${n(y + h)}H${n(x)}Z`;
  const a = `A${n(r)} ${n(r)} 0 0 1`;
  return (
    `M${n(x + r)} ${n(y)}H${n(x + w - r)}${a} ${n(x + w)} ${n(y + r)}V${n(y + h - r)}` +
    `${a} ${n(x + w - r)} ${n(y + h)}H${n(x + r)}${a} ${n(x)} ${n(y + h - r)}V${n(y + r)}${a} ${n(x + r)} ${n(y)}Z`
  );
}

/**
 * O `clip-path` de um fundo que ocupa `box` (relativo à janela): ele inteiro, menos os buracos dos
 * vídeos. Sem vídeo, `none` (o fundo inteiro aparece).
 */
export function clipPathFor(box: { left: number; top: number; width: number; height: number }, holes: SeeThrough[]) {
  const inside = holes
    .map((h) => ({
      x: h.x - box.left + SEE_THROUGH_INSET,
      y: h.y - box.top + SEE_THROUGH_INSET,
      width: h.width - 2 * SEE_THROUGH_INSET,
      height: h.height - 2 * SEE_THROUGH_INSET,
      radius: Math.max(0, h.radius - SEE_THROUGH_INSET),
    }))
    // Só os buracos que caem neste fundo.
    .filter(
      (h) =>
        h.width > 0 && h.height > 0 && h.x < box.width && h.y < box.height && h.x + h.width > 0 && h.y + h.height > 0,
    );
  if (inside.length === 0) return "none";
  const outer = roundedRect(0, 0, box.width, box.height, 0);
  return `path(evenodd, "${outer}${inside.map((h) => roundedRect(h.x, h.y, h.width, h.height, h.radius)).join("")}")`;
}
