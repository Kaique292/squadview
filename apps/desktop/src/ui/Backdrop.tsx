import { useEffect, useRef } from "react";
import { clipPathFor, onSeeThroughChange, seeThroughAreas } from "./see-through";

/**
 * O fundo de uma área que pode ter vídeo por cima dela (ADR 0021): pinta a cor de `className` em
 * toda a área, menos onde há uma live passando, que fica transparente para o vídeo, desenhado por
 * baixo da página, aparecer. O pai precisa de `relative isolate` e não pode ter fundo próprio.
 */
export function Backdrop({ className }: { className: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = "";
    const update = () => {
      const clip = clipPathFor(el.getBoundingClientRect(), seeThroughAreas());
      if (clip === last) return;
      last = clip;
      el.style.clipPath = clip;
    };
    const stop = onSeeThroughChange(update);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(el);
    update();
    return () => {
      stop();
      observer?.disconnect();
    };
  }, []);
  return (
    <div ref={ref} aria-hidden data-backdrop className={`pointer-events-none absolute inset-0 -z-10 ${className}`} />
  );
}
