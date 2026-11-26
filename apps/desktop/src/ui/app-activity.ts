import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();

/**
 * O app está em uso? Com a janela minimizada, escondida ou sem foco (o jogo na frente), marca
 * `data-idle` no `<html>`, e o CSS pausa o que se repete (spinners, pulsos, o efeito de fala, a
 * classe `loop`): nada de animação contínua gastando GPU ao lado do jogo (ADR 0034).
 */
export function appIdle(doc: Document = document): boolean {
  return doc.visibilityState === "hidden" || !doc.hasFocus();
}

/** Passa a acompanhar o foco e a visibilidade da janela. Devolve como parar. */
export function watchAppActivity(win: Window = window): () => void {
  const doc = win.document;
  const update = () => {
    const was = "idle" in doc.documentElement.dataset;
    if (appIdle(doc)) doc.documentElement.dataset.idle = "";
    else delete doc.documentElement.dataset.idle;
    if (was !== "idle" in doc.documentElement.dataset) listeners.forEach((fn) => fn());
  };
  update();
  win.addEventListener("focus", update);
  win.addEventListener("blur", update);
  doc.addEventListener("visibilitychange", update);
  return () => {
    win.removeEventListener("focus", update);
    win.removeEventListener("blur", update);
    doc.removeEventListener("visibilitychange", update);
    delete doc.documentElement.dataset.idle;
    listeners.forEach((fn) => fn());
  };
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/** O app está em uso agora (sem a marca `data-idle`): para componentes que ligam efeitos caros. */
export function useAppInUse(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => !("idle" in document.documentElement.dataset),
    () => true,
  );
}
