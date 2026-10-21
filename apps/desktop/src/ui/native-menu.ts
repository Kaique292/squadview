/** Onde o menu do próprio Windows ainda vale: nas caixas de texto ele traz Copiar e Colar. */
function editable(target: EventTarget | null) {
  return target instanceof HTMLElement && target.closest("input, textarea, [contenteditable='true']") !== null;
}

/**
 * O botão direito só abre os menus do app: o menu do navegador (Voltar, Atualizar, Imprimir…)
 * não faz sentido aqui. Os menus do app (Radix) tratam o evento antes, no React; este ouvinte, no
 * documento, só barra o que ninguém tratou. Devolve a função que desliga.
 */
export function blockNativeContextMenu(root: Document = document) {
  const onContextMenu = (e: MouseEvent) => {
    if (!editable(e.target)) e.preventDefault();
  };
  root.addEventListener("contextmenu", onContextMenu);
  return () => root.removeEventListener("contextmenu", onContextMenu);
}
