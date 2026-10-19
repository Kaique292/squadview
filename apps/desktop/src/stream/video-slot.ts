import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { rememberVideoRect } from "../ui/dissolve-effect";

/**
 * Informa ao Rust onde está a área do vídeo, em pixels físicos, sempre que ela muda de lugar ou de
 * tamanho (janela redimensionada, gaveta aberta, tela cheia). Ao sair, esconde o vídeo.
 */
export function useVideoSlot() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || !("__TAURI_INTERNALS__" in window)) return;
    let last = "";
    const report = () => {
      const r = el.getBoundingClientRect();
      // Guarda onde o vídeo está: a animação de saída aparece exatamente ali.
      rememberVideoRect({ x: r.left, y: r.top, width: r.width, height: r.height });
      const dpr = window.devicePixelRatio || 1;
      const b = {
        x: Math.round(r.left * dpr),
        y: Math.round(r.top * dpr),
        width: Math.round(r.width * dpr),
        height: Math.round(r.height * dpr),
      };
      const key = `${b.x},${b.y},${b.width},${b.height}`;
      if (key === last) return;
      last = key;
      invoke("set_video_bounds", b).catch(() => {});
    };
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    report();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
      invoke("set_video_bounds", { x: 0, y: 0, width: 0, height: 0 }).catch(() => {});
    };
  }, []);
  return ref;
}
