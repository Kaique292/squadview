/**
 * O vídeo nativo fica por cima da interface e recebe o mouse no lugar dela. O Rust repassa o
 * clique, o botão direito e a entrada e a saída do mouse (`video-pointer`); aqui eles viram eventos
 * normais no elemento que está embaixo do vídeo, e o resto da interface nem percebe a diferença:
 * o botão direito abre o menu do quadro no lugar do clique, o clique põe a live em foco.
 */

/**
 * O que o Rust manda: pixels físicos a partir do canto do vídeo, e de qual vídeo (`source`: `null`
 * é a nossa live; um id, a live de quem assistimos). Apps antigos não mandam `source`.
 */
export type VideoPointer = (
  { kind: "click" | "menu"; x: number; y: number } | { kind: "enter" | "leave"; x?: undefined; y?: undefined }
) & { source?: string | null };

/** A chave da nossa live entre os vídeos (não colide com o id de ninguém). */
export const OWN_VIDEO = "transmissao:minha";

/** A chave de um vídeo a partir do `source` que o Rust manda. */
export function videoKey(source: string | null | undefined): string {
  return source ?? OWN_VIDEO;
}

/** Espera de um clique para ver se não é o primeiro de dois (dois cliques são a tela cheia). */
export const DOUBLE_CLICK_MS = 250;

const videoRects = new Map<string, { left: number; top: number }>();
const pendingClicks = new Map<string, ReturnType<typeof setTimeout>>();

/** Onde a área de um vídeo está agora (pixels CSS), vinda do `useVideoSlot`. `null`: saiu da tela. */
export function setVideoRect(rect: { left: number; top: number } | null, of = OWN_VIDEO) {
  if (rect) videoRects.set(of, rect);
  else videoRects.delete(of);
}

/** O ponto do vídeo `of` na página (pixels CSS), ou `null` se não sabemos onde ele está. */
export function toClient(x: number, y: number, of = OWN_VIDEO): { clientX: number; clientY: number } | null {
  const rect = videoRects.get(of);
  if (!rect) return null;
  const dpr = window.devicePixelRatio || 1;
  return { clientX: rect.left + x / dpr, clientY: rect.top + y / dpr };
}

function fire(type: "click" | "contextmenu", x: number, y: number, of: string) {
  const point = toClient(x, y, of);
  if (!point) return;
  const target = document.elementFromPoint(point.clientX, point.clientY);
  target?.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, button: type === "click" ? 0 : 2, ...point }),
  );
}

/**
 * Entrega o evento do vídeo à interface. `onHover` recebe de qual vídeo e se o mouse entrou ou saiu.
 */
export function handleVideoPointer(event: VideoPointer, onHover: (of: string, inside: boolean) => void) {
  const of = videoKey(event.source);
  switch (event.kind) {
    case "enter":
    case "leave":
      onHover(of, event.kind === "enter");
      return;
    case "menu":
      fire("contextmenu", event.x, event.y, of);
      return;
    case "click": {
      // Num duplo clique chegam dois cliques e a tela cheia: o clique espera, e a tela cheia o cancela.
      cancelVideoClick(of);
      const { x, y } = event;
      pendingClicks.set(
        of,
        setTimeout(() => {
          pendingClicks.delete(of);
          fire("click", x, y, of);
        }, DOUBLE_CLICK_MS),
      );
    }
  }
}

/** Dois cliques no vídeo `of` (tela cheia): o clique que estava esperando não vale. */
export function cancelVideoClick(of = OWN_VIDEO) {
  clearTimeout(pendingClicks.get(of));
  pendingClicks.delete(of);
}
