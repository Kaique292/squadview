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

/** Só para testes. */
export function openOverlays() {
  return open;
}
