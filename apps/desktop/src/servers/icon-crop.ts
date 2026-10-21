/**
 * Recorte do ícone do servidor: a imagem fica atrás de uma janela quadrada de `view` px e a pessoa
 * arrasta (posição) e dá zoom. A imagem sempre cobre a janela inteira, sem sobrar borda vazia.
 * Funções puras (testadas); quem desenha é o `IconCropDialog`.
 */

/** Lado do ícone salvo (2× os 48 px da barra de salas). */
export const ICON_SIZE = 96;
/** Tamanho máximo do ícone em data URL (o Worker recusa acima disso; ver ADR 0016). */
export const MAX_ICON_LENGTH = 12 * 1024;
export const MAX_ZOOM = 4;

export interface Crop {
  /** Canto da imagem relativo à janela (px da janela; ≤ 0 enquanto a imagem cobre a janela). */
  x: number;
  y: number;
  /** 1 = a imagem só cobre a janela; até `MAX_ZOOM`. */
  zoom: number;
}

/** Escala em que o lado menor da imagem cabe exatamente na janela. */
export function coverScale(width: number, height: number, view: number): number {
  return view / Math.min(width, height);
}

/** Mantém a imagem cobrindo a janela (sem sobrar fundo em nenhum lado). */
export function clampCrop(crop: Crop, width: number, height: number, view: number): Crop {
  const zoom = Math.min(MAX_ZOOM, Math.max(1, crop.zoom));
  const scale = coverScale(width, height, view) * zoom;
  const clamp = (v: number, size: number) => Math.min(0, Math.max(view - size * scale, v));
  return { zoom, x: clamp(crop.x, width), y: clamp(crop.y, height) };
}

/** Começo: a imagem centralizada, sem zoom. */
export function centeredCrop(width: number, height: number, view: number): Crop {
  const scale = coverScale(width, height, view);
  return { zoom: 1, x: (view - width * scale) / 2, y: (view - height * scale) / 2 };
}

/** Zoom novo mantendo parado o ponto (`px`, `py` da janela) que está sob o cursor ou no centro. */
export function zoomAround(
  crop: Crop,
  zoom: number,
  width: number,
  height: number,
  view: number,
  px = view / 2,
  py = view / 2,
): Crop {
  const next = Math.min(MAX_ZOOM, Math.max(1, zoom));
  const ratio = next / crop.zoom;
  return clampCrop({ zoom: next, x: px - (px - crop.x) * ratio, y: py - (py - crop.y) * ratio }, width, height, view);
}

/** O pedaço da imagem original (px dela) que aparece na janela. */
export function sourceRect(crop: Crop, width: number, height: number, view: number) {
  const scale = coverScale(width, height, view) * crop.zoom;
  const side = view / scale;
  return { sx: -crop.x / scale, sy: -crop.y / scale, side };
}

/**
 * Comprime o canvas do ícone até caber no limite: WebP com qualidade caindo e, se o navegador não
 * fizer WebP, JPEG. `null` se nem na menor qualidade couber (não deve acontecer com 96 px).
 */
export function encodeIcon(canvas: HTMLCanvasElement): string | null {
  for (const type of ["image/webp", "image/jpeg"]) {
    for (const quality of [0.9, 0.8, 0.7, 0.55, 0.4]) {
      const url = canvas.toDataURL(type, quality);
      // Sem suporte ao formato, o navegador devolve PNG: tenta o próximo.
      if (!url.startsWith(`data:${type};`)) break;
      if (url.length <= MAX_ICON_LENGTH) return url;
    }
  }
  return null;
}
