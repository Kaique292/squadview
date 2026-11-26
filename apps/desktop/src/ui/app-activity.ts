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
    if (appIdle(doc)) doc.documentElement.dataset.idle = "";
    else delete doc.documentElement.dataset.idle;
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
  };
}
