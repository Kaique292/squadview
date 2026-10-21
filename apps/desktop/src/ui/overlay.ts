import { invoke } from "@tauri-apps/api/core";

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
 * e o que for marcado com `data-video-hole` (avisos no canto, os selos e botões em cima da live).
 */
const HOLE_SELECTOR = "[data-radix-popper-content-wrapper], [data-video-hole]";
/** Quanto o buraco fica para dentro do que está por cima (pixels físicos). */
const HOLE_INSET = 1;

export interface Hole {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Raio dos cantos (pixels físicos): o buraco tem a forma do menu ou da pílula. */
  radius: number;
}

/** O raio do canto de cima à esquerda (todos os nossos menus e pílulas têm os quatro iguais). */
function radiusOf(el: HTMLElement) {
  const style = getComputedStyle(el);
  return parseFloat(style.borderTopLeftRadius || style.borderRadius) || 0;
}

/**
 * Os recortes (pixels físicos, relativos à janela) da interface que deve ficar por cima do vídeo.
 * Cada um tem o tamanho e os cantos exatos do que está por cima: embaixo do buraco não há vídeo,
 * só o fundo preto da área, então qualquer folga aparecia como uma moldura preta em volta.
 */
export function measureHoles(root: ParentNode = document): Hole[] {
  const dpr = window.devicePixelRatio || 1;
  const holes: Hole[] = [];
  const add = (r: DOMRect, radius: number) => {
    if (r.width <= 0 || r.height <= 0) return;
    // Um pixel para dentro de cada lado: o recorte do Windows tem os cantos em degraus, e onde ele
    // passava da borda suavizada de uma pílula aparecia o preto de baixo (as "manchas" no AO VIVO).
    holes.push({
      x: Math.ceil(r.left * dpr) + HOLE_INSET,
      y: Math.ceil(r.top * dpr) + HOLE_INSET,
      width: Math.max(0, Math.floor(r.width * dpr) - 2 * HOLE_INSET),
      height: Math.max(0, Math.floor(r.height * dpr) - 2 * HOLE_INSET),
      radius: Math.max(0, Math.round(Math.min(radius, r.width / 2, r.height / 2) * dpr) - HOLE_INSET),
    });
  };
  for (const el of root.querySelectorAll<HTMLElement>(HOLE_SELECTOR)) {
    // No portal do Radix, quem tem o fundo e os cantos é o conteúdo, não a caixa em volta.
    const target = el.matches("[data-radix-popper-content-wrapper]")
      ? ((el.firstElementChild as HTMLElement | null) ?? el)
      : el;
    add(target.getBoundingClientRect(), radiusOf(target));
    // A setinha da dica fica fora da caixa: ganha um buraco só dela.
    for (const arrow of target.querySelectorAll("svg")) {
      if (arrow.parentElement?.style.position === "absolute") add(arrow.getBoundingClientRect(), 0);
    }
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
