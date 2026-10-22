/**
 * O menu fica num portal, mas no React ele é filho do quadro: o clique num item subiria até o
 * quadro e o poria em foco (e dois cliques, em tela cheia). Ele para no menu.
 */
export function stopInsideMenu(e: { stopPropagation: () => void }) {
  e.stopPropagation();
}
