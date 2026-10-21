/**
 * O vídeo nativo fica por cima da interface e recebe o mouse no lugar dela. O Rust repassa o
 * clique, o botão direito e a entrada e a saída do mouse (`video-pointer`); aqui eles viram eventos
 * normais no elemento que está embaixo do vídeo, e o resto da interface nem percebe a diferença:
 * o botão direito abre o menu do quadro no lugar do clique, o clique põe a live em foco.
 */

/** O que o Rust manda: pixels físicos a partir do canto do vídeo. */
export type VideoPointer =
  { kind: "click" | "menu"; x: number; y: number } | { kind: "enter" | "leave"; x?: undefined; y?: undefined };

/** Espera de um clique para ver se não é o primeiro de dois (dois cliques são a tela cheia). */
export const DOUBLE_CLICK_MS = 250;

let videoRect: { left: number; top: number } | null = null;
let pendingClick: ReturnType<typeof setTimeout> | null = null;

/** Onde a área do vídeo está agora (pixels CSS), vinda do `useVideoSlot`. */
export function setVideoRect(rect: { left: number; top: number } | null) {
  videoRect = rect;
}

/** O ponto do vídeo na página (pixels CSS), ou `null` se não sabemos onde o vídeo está. */
export function toClient(x: number, y: number): { clientX: number; clientY: number } | null {
  if (!videoRect) return null;
  const dpr = window.devicePixelRatio || 1;
  return { clientX: videoRect.left + x / dpr, clientY: videoRect.top + y / dpr };
}

function fire(type: "click" | "contextmenu", x: number, y: number) {
  const point = toClient(x, y);
  if (!point) return;
  const target = document.elementFromPoint(point.clientX, point.clientY);
  target?.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, button: type === "click" ? 0 : 2, ...point }),
  );
}

/** Entrega o evento do vídeo à interface. `onHover` recebe a entrada e a saída do mouse. */
export function handleVideoPointer(event: VideoPointer, onHover: (inside: boolean) => void) {
  switch (event.kind) {
    case "enter":
    case "leave":
      onHover(event.kind === "enter");
      return;
    case "menu":
      fire("contextmenu", event.x, event.y);
      return;
    case "click": {
      // Num duplo clique chegam dois cliques e a tela cheia: o clique espera, e a tela cheia o cancela.
      cancelVideoClick();
      const { x, y } = event;
      pendingClick = setTimeout(() => {
        pendingClick = null;
        fire("click", x, y);
      }, DOUBLE_CLICK_MS);
    }
  }
}

/** Dois cliques no vídeo (tela cheia): o clique que estava esperando não vale. */
export function cancelVideoClick() {
  if (pendingClick) clearTimeout(pendingClick);
  pendingClick = null;
}
