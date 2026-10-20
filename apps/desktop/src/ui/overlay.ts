import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";

/**
 * O vídeo do stream é uma janela nativa dentro do app: ela sempre fica por cima da interface.
 * Enquanto houver um modal (ou outro overlay) aberto, o vídeo some para não cobrir o modal.
 */
let open = 0;

function sync() {
  if (!("__TAURI_INTERNALS__" in window)) return;
  invoke("set_video_hidden", { hidden: open > 0 }).catch(() => {});
}

/** Chame ao abrir um overlay; a função devolvida fecha (use no cleanup de um effect). */
export function holdOverlay() {
  open += 1;
  sync();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    open = Math.max(0, open - 1);
    sync();
  };
}

/** Segura o overlay enquanto `open` for verdadeiro (e solta ao desmontar). */
export function useOverlayWhile(open: boolean) {
  useEffect(() => (open ? holdOverlay() : undefined), [open]);
}

/**
 * Para menus, listas suspensas e popovers do Radix: devolve o `onOpenChange` do `Root`.
 * O menu não esconde mais o vídeo: ele aparece por cima num buraco (ver `watchVideoHoles`), e a
 * live continua passando em volta. `onChange` (opcional) recebe o valor.
 */
export function useOverlayMenu(onChange?: (open: boolean) => void) {
  return (next: boolean) => onChange?.(next);
}

/**
 * O que abre um buraco no vídeo: tudo que o Radix põe num portal (menus, dicas, listas suspensas)
 * e o que for marcado com `data-video-hole` (avisos no canto, pedidos para entrar).
 */
const HOLE_SELECTOR = "[data-radix-popper-content-wrapper], [data-video-hole]";
/** Folga em volta, para a sombra e a setinha da dica não ficarem cortadas pelo vídeo. */
const HOLE_MARGIN = 6;

export interface Hole {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Os retângulos (pixels físicos, relativos à janela) da interface que deve ficar por cima do vídeo. */
export function measureHoles(root: ParentNode = document): Hole[] {
  const dpr = window.devicePixelRatio || 1;
  const holes: Hole[] = [];
  for (const el of root.querySelectorAll(HOLE_SELECTOR)) {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) continue;
    holes.push({
      x: Math.floor((r.left - HOLE_MARGIN) * dpr),
      y: Math.floor((r.top - HOLE_MARGIN) * dpr),
      width: Math.ceil((r.width + 2 * HOLE_MARGIN) * dpr),
      height: Math.ceil((r.height + 2 * HOLE_MARGIN) * dpr),
    });
  }
  return holes;
}

/**
 * Enquanto há vídeo na tela: vigia menus, dicas e avisos e manda ao Rust onde eles estão, para o
 * vídeo abrir um buraco ali em vez de cobri-los. Mede uma vez por quadro no máximo (e de novo logo
 * depois, quando o Radix termina de posicionar e animar). Devolve a função que para de vigiar.
 */
export function watchVideoHoles(): () => void {
  if (!("__TAURI_INTERNALS__" in window)) return () => {};
  let last = "[]";
  let frame = 0;
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const send = () => {
    frame = 0;
    const holes = measureHoles();
    const key = JSON.stringify(holes);
    if (key === last) return;
    last = key;
    invoke("set_video_holes", { holes }).catch(() => {});
  };
  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(send);
    // O Radix posiciona e anima depois de montar: confere de novo quando assenta.
    for (const ms of [150, 350]) {
      const t = setTimeout(() => {
        timers.delete(t);
        send();
      }, ms);
      timers.add(t);
    }
  };
  const observer = new MutationObserver(schedule);
  observer.observe(document.body, { childList: true, subtree: true });
  window.addEventListener("resize", schedule);
  schedule();
  return () => {
    observer.disconnect();
    window.removeEventListener("resize", schedule);
    if (frame) cancelAnimationFrame(frame);
    timers.forEach(clearTimeout);
    if (last !== "[]") invoke("set_video_holes", { holes: [] }).catch(() => {});
  };
}

/** Só para testes. */
export function openOverlays() {
  return open;
}
