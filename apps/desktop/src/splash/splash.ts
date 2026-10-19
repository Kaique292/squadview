import "@fontsource-variable/figtree";
import { listen } from "@tauri-apps/api/event";
import { SPLASH_EVENT, type SplashProgress } from "./boot";

/** Dicas que aparecem embaixo enquanto carrega (como as frases do Discord). */
const TIPS = [
  "Dica: dê dois cliques no vídeo para ver em tela cheia.",
  "Dica: o vídeo vai direto de um PC para o outro, sem servidor no meio.",
  "Dica: o código da sala tem 6 letras. Mande para a galera no Discord.",
  "Dica: em Configurações dá para ajustar o efeito de vidro.",
  "Dica: transmita só a janela do jogo para o som da call não ir junto.",
];

const fill = document.getElementById("fill")!;
const status = document.getElementById("status")!;
const bar = fill.parentElement!;
document.getElementById("tip")!.textContent = TIPS[Math.floor(Math.random() * TIPS.length)];

let current = 0.04;
function show(value: number, text?: string) {
  current = Math.max(current, Math.min(1, value));
  fill.style.width = `${current * 100}%`;
  bar.setAttribute("aria-valuenow", String(Math.round(current * 100)));
  if (text) status.textContent = text;
}

// Enquanto a janela principal não diz nada, a barra anda devagar sozinha (até 30%).
const creep = setInterval(() => show(Math.min(0.3, current + 0.03)), 250);

if ("__TAURI_INTERNALS__" in window) {
  void listen<SplashProgress>(SPLASH_EVENT, (e) => {
    clearInterval(creep);
    show(e.payload.value, e.payload.text);
  });
}
