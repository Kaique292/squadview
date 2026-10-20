import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

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
 * Para menus, listas suspensas e popovers do Radix: devolve o `onOpenChange` do `Root`, que
 * esconde o vídeo enquanto o menu está aberto. `onChange` (opcional) recebe o mesmo valor.
 */
export function useOverlayMenu(onChange?: (open: boolean) => void) {
  const [isOpen, setOpen] = useState(false);
  useOverlayWhile(isOpen);
  return (next: boolean) => {
    setOpen(next);
    onChange?.(next);
  };
}

/** Só para testes. */
export function openOverlays() {
  return open;
}
