import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { rememberVideoRect } from "../ui/dissolve-effect";
import { setSeeThrough } from "../ui/see-through";
import { OWN_VIDEO } from "./video-key";

type PhysicalBounds = { x: number; y: number; width: number; height: number; radius: number };

/** Animações e transições que podem mover o quadro sem mudar o tamanho dele. */
const MOVE_EVENTS = [
  "transitionrun",
  "transitionend",
  "transitioncancel",
  "animationstart",
  "animationend",
  "animationcancel",
] as const;
/** Depois do último evento de animação em volta do vídeo, quanto tempo seguir o quadro. */
const FOLLOW_MS = 300;

/** Manda ao Rust onde fica um vídeo: a nossa live (sem `memberId`) ou a de quem assistimos. */
function sendBounds(memberId: string | undefined, b: PhysicalBounds) {
  const sent = memberId ? invoke("set_watch_bounds", { memberId, ...b }) : invoke("set_video_bounds", b);
  sent.catch(() => {});
}

/** O raio dos cantos do quadro em volta da área do vídeo (pixels da interface). */
function tileRadius(el: HTMLElement) {
  const tile = el.parentElement;
  return tile ? parseFloat(getComputedStyle(tile).borderTopLeftRadius) || 0 : 0;
}

/**
 * Informa ao Rust onde está a área de um vídeo, em pixels físicos, sempre que ela muda de lugar ou
 * de tamanho (janela redimensionada, gaveta aberta, tela cheia). Ao sair, esconde o vídeo.
 *
 * O vídeo é desenhado por baixo da interface (ADR 0021): com `showing` (a imagem está chegando),
 * os fundos da página ficam transparentes na área do quadro e ele aparece; sem, o quadro continua
 * preto, com o aviso dele por cima.
 *
 * `memberId`: a live de quem assistimos (sem ele, a nossa). `hidden`: o quadro está escondido
 * (a faixa recolhida), e o vídeo some junto.
 */
export function useVideoSlot(memberId?: string, hidden = false, showing = true) {
  const ref = useRef<HTMLDivElement>(null);
  const showingRef = useRef(showing);
  const reportRef = useRef<() => void>(() => {});
  useEffect(() => {
    const el = ref.current;
    if (!el || !("__TAURI_INTERNALS__" in window)) return;
    const of = memberId ?? OWN_VIDEO;
    let last = "";
    const report = () => {
      const r = el.getBoundingClientRect();
      // Guarda onde o vídeo está: a animação de saída aparece exatamente ali.
      rememberVideoRect({ x: r.left, y: r.top, width: r.width, height: r.height }, of);
      const radius = hidden ? 0 : tileRadius(el);
      setSeeThrough(
        of,
        hidden || !showingRef.current ? null : { x: r.left, y: r.top, width: r.width, height: r.height, radius },
      );
      const dpr = window.devicePixelRatio || 1;
      const b = hidden
        ? { x: 0, y: 0, width: 0, height: 0, radius: 0 }
        : {
            x: Math.round(r.left * dpr),
            y: Math.round(r.top * dpr),
            width: Math.round(r.width * dpr),
            height: Math.round(r.height * dpr),
            radius: Math.round(radius * dpr),
          };
      const key = `${b.x},${b.y},${b.width},${b.height},${b.radius}`;
      if (key === last) return;
      last = key;
      sendBounds(memberId, b);
    };
    reportRef.current = report;
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    // A faixa abrindo ou o quadro entrando animado mudam o lugar sem mudar o tamanho: o
    // ResizeObserver não vê, e o vídeo ficava fora do quadro. Enquanto algo em volta dele anima,
    // segue o quadro a cada quadro da página (só nesse meio-tempo).
    let until = 0;
    let frame = 0;
    const follow = () => {
      report();
      frame = performance.now() < until ? requestAnimationFrame(follow) : 0;
    };
    const onMove = (e: Event) => {
      if (!(e.target instanceof Node) || !e.target.contains(el)) return;
      until = performance.now() + FOLLOW_MS;
      report();
      if (!frame) frame = requestAnimationFrame(follow);
    };
    for (const type of MOVE_EVENTS) document.addEventListener(type, onMove, true);
    report();
    return () => {
      reportRef.current = () => {};
      observer.disconnect();
      window.removeEventListener("resize", report);
      for (const type of MOVE_EVENTS) document.removeEventListener(type, onMove, true);
      if (frame) cancelAnimationFrame(frame);
      setSeeThrough(of, null);
      sendBounds(memberId, { x: 0, y: 0, width: 0, height: 0, radius: 0 });
    };
  }, [memberId, hidden]);
  // A imagem começou (ou parou) de chegar: só a transparência muda, o vídeo fica onde está.
  useEffect(() => {
    showingRef.current = showing;
    reportRef.current();
  }, [showing]);
  return ref;
}
