import { invoke } from "@tauri-apps/api/core";
import { type RefObject, useEffect } from "react";
import { useAppInUse } from "../ui/app-activity";
import { setSeeThrough } from "../ui/see-through";

/**
 * A chave da luz ambiente entre as áreas transparentes da página (ver `see-through.ts`): a sala
 * inteira fica transparente para a luz, desenhada por baixo de tudo, aparecer.
 */
export const AMBIENT_KEY = "luz-ambiente";
/** A vinheta escurece a luz, mas não abre buraco para ela (só para os quadros de vídeo). */
export const AMBIENT_EXCEPT = [AMBIENT_KEY] as const;

/**
 * A luz ambiente da sala (ADR 0034): a live em foco, minúscula e esticada, por baixo da sala,
 * com a vinheta da página por cima. Liga só com uma live de alguém em foco, já com imagem, e com o
 * app em uso (fora de uso, nada de efeito gastando GPU ao lado do jogo). Fora disso, a sala volta
 * ao fundo preto. Devolve se a luz está ligada (para a página pintar a vinheta).
 *
 * `room`: a área da sala. `memberId`: de quem é a live em foco, se ela já tem imagem.
 */
export function useAmbientLight(room: RefObject<HTMLElement | null>, memberId: string | null): boolean {
  const inUse = useAppInUse();
  const active = memberId !== null && inUse && "__TAURI_INTERNALS__" in window;
  useEffect(() => {
    const el = room.current;
    if (!active || !el || !memberId) return;
    let last = "";
    const report = () => {
      const r = el.getBoundingClientRect();
      setSeeThrough(AMBIENT_KEY, { x: r.left, y: r.top, width: r.width, height: r.height, radius: 0 });
      const dpr = window.devicePixelRatio || 1;
      const area = {
        x: Math.round(r.left * dpr),
        y: Math.round(r.top * dpr),
        width: Math.round(r.width * dpr),
        height: Math.round(r.height * dpr),
      };
      const key = `${area.x},${area.y},${area.width},${area.height}`;
      if (key === last) return;
      last = key;
      invoke("set_ambient_area", area).catch(() => {});
    };
    invoke("set_watch_ambient", { memberId, on: true }).catch(() => {});
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    report();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
      setSeeThrough(AMBIENT_KEY, null);
      invoke("set_ambient_area", { x: 0, y: 0, width: 0, height: 0 }).catch(() => {});
      invoke("set_watch_ambient", { memberId, on: false }).catch(() => {});
    };
  }, [active, memberId, room]);
  return active;
}
