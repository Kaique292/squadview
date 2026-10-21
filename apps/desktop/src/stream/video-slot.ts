import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { rememberVideoRect } from "../ui/dissolve-effect";
import { watchVideoHoles } from "../ui/overlay";
import { OWN_VIDEO, setVideoRect } from "./video-pointer";

/** Manda ao Rust onde fica um vídeo: a nossa live (sem `memberId`) ou a de quem assistimos. */
function sendBounds(memberId: string | undefined, b: { x: number; y: number; width: number; height: number }) {
  const sent = memberId ? invoke("set_watch_bounds", { memberId, ...b }) : invoke("set_video_bounds", b);
  sent.catch(() => {});
}

/**
 * Informa ao Rust onde está a área de um vídeo, em pixels físicos, sempre que ela muda de lugar ou
 * de tamanho (janela redimensionada, gaveta aberta, tela cheia). Ao sair, esconde o vídeo.
 *
 * `memberId`: a live de quem assistimos (sem ele, a nossa). `hidden`: o quadro está escondido
 * (a faixa recolhida), e o vídeo nativo, que não respeita o CSS, some junto.
 */
export function useVideoSlot(memberId?: string, hidden = false) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || !("__TAURI_INTERNALS__" in window)) return;
    const of = memberId ?? OWN_VIDEO;
    let last = "";
    const report = () => {
      const r = el.getBoundingClientRect();
      // Guarda onde o vídeo está: a animação de saída aparece exatamente ali.
      rememberVideoRect({ x: r.left, y: r.top, width: r.width, height: r.height }, of);
      // E onde o mouse que o vídeo repassa cai na página.
      setVideoRect(r, of);
      const dpr = window.devicePixelRatio || 1;
      const b = hidden
        ? { x: 0, y: 0, width: 0, height: 0 }
        : {
            x: Math.round(r.left * dpr),
            y: Math.round(r.top * dpr),
            width: Math.round(r.width * dpr),
            height: Math.round(r.height * dpr),
          };
      const key = `${b.x},${b.y},${b.width},${b.height}`;
      if (key === last) return;
      last = key;
      sendBounds(memberId, b);
    };
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    report();
    // Menus e dicas por cima do vídeo: ele abre buracos para eles, em vez de cobri-los.
    const stopHoles = watchVideoHoles();
    return () => {
      stopHoles();
      observer.disconnect();
      window.removeEventListener("resize", report);
      setVideoRect(null, of);
      sendBounds(memberId, { x: 0, y: 0, width: 0, height: 0 });
    };
  }, [memberId, hidden]);
  return ref;
}
