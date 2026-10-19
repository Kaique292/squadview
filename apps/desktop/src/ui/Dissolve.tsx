import { useEffect, useRef } from "react";
import {
  DUST_MS,
  GLOW_MS,
  MARGIN,
  type Rect,
  gridStep,
  makeParticles,
  particleAt,
  useDissolveStore,
} from "./dissolve-effect";

/** Desenha a animação de saída do vídeo por cima de tudo (menos modais e notificações). */
export function Dissolve() {
  const effect = useDissolveStore((s) => s.effect);
  if (!effect) return null;
  return <DissolveCanvas key={effect.id} id={effect.id} src={effect.src} rect={effect.rect} />;
}

function DissolveCanvas(props: { id: number; src: string; rect: Rect }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const done = useDissolveStore((s) => s.done);
  const { id, src, rect } = props;

  useEffect(() => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    // Sem canvas (testes) ou com menos movimento pedido no Windows: termina na hora.
    if (!el || !ctx || reduced) {
      done(id);
      return;
    }
    let frame = 0;
    let cancelled = false;
    const image = new Image();
    image.onerror = () => done(id);
    image.onload = () => {
      if (cancelled) return;
      const dpr = window.devicePixelRatio || 1;
      const w = Math.round(rect.width);
      const h = Math.round(rect.height);
      el.width = Math.round((w + MARGIN) * dpr);
      el.height = Math.round((h + MARGIN) * dpr);

      // Pixels da foto no tamanho em que ela aparece, para tirar a cor de cada partícula.
      const sample = document.createElement("canvas");
      sample.width = w;
      sample.height = h;
      const sctx = sample.getContext("2d", { willReadFrequently: true });
      if (!sctx) return done(id);
      sctx.drawImage(image, 0, 0, w, h);
      const step = gridStep(w, h);
      const particles = makeParticles(sctx.getImageData(0, 0, w, h).data, w, h, step);

      ctx.scale(dpr, dpr);
      // A imagem fica embaixo, com a margem livre em cima e à direita para a poeira voar.
      const ox = 0;
      const oy = MARGIN;
      const start = performance.now();
      const draw = (now: number) => {
        if (cancelled) return;
        const elapsed = now - start;
        ctx.clearRect(0, 0, w + MARGIN, h + MARGIN);
        if (elapsed < GLOW_MS) {
          // Brilho: a imagem inteira, um pouco mais clara, com a borda acesa.
          const k = elapsed / GLOW_MS;
          ctx.globalAlpha = 1;
          ctx.filter = `brightness(${1 + 0.25 * Math.sin(k * Math.PI)})`;
          ctx.drawImage(image, ox, oy, w, h);
          ctx.filter = "none";
          ctx.strokeStyle = `rgba(255,255,255,${0.75 * Math.sin(k * Math.PI)})`;
          ctx.lineWidth = 3;
          ctx.strokeRect(ox + 1.5, oy + 1.5, w - 3, h - 3);
          frame = requestAnimationFrame(draw);
          return;
        }
        const t = (elapsed - GLOW_MS) / DUST_MS;
        if (t >= 1) return done(id);
        // O que ainda não voou continua nítido: desenha a foto e apaga os blocos que já saíram.
        ctx.globalAlpha = 1;
        ctx.drawImage(image, ox, oy, w, h);
        for (const p of particles) {
          if (particleAt(p, t) !== "waiting") ctx.clearRect(ox + p.x, oy + p.y, step, step);
        }
        // A poeira mantém a cor da imagem, como no iOS.
        for (const p of particles) {
          const s = particleAt(p, t);
          if (typeof s === "string") continue;
          const size = step * s.scale;
          ctx.globalAlpha = s.alpha;
          ctx.fillStyle = p.color;
          ctx.fillRect(ox + s.x, oy + s.y, size, size);
        }
        frame = requestAnimationFrame(draw);
      };
      frame = requestAnimationFrame(draw);
    };
    image.src = src;
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [id, src, rect, done]);

  return (
    <canvas
      ref={canvas}
      aria-hidden
      data-testid="animacao-de-saida"
      className="pointer-events-none fixed z-30"
      style={{
        left: rect.x,
        top: rect.y - MARGIN,
        width: rect.width + MARGIN,
        height: rect.height + MARGIN,
      }}
    />
  );
}
