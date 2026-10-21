import { useEffect, useRef } from "react";
import {
  DUST_MS,
  GRAIN,
  HOLD_MS,
  type Rect,
  canvasBox,
  fitInside,
  gridStep,
  imageAlpha,
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
      // Tudo é desenhado em pixels físicos e inteiros, para a foto não ser reamostrada (borrada)
      // com a escala do Windows (125%, 150%).
      const dpr = window.devicePixelRatio || 1;
      const { margin, pixelWidth, pixelHeight } = canvasBox(rect, dpr);
      el.width = pixelWidth;
      el.height = pixelHeight;

      // A foto ocupa a área do vídeo do mesmo jeito que o vídeo nativo: inteira, centralizada.
      const area = fitInside(image.naturalWidth, image.naturalHeight, rect.width, rect.height);
      const w = Math.max(1, Math.round(area.width * dpr));
      const h = Math.max(1, Math.round(area.height * dpr));
      const ox = Math.round(area.x * dpr);
      // Em cima fica a margem livre para a poeira voar.
      const oy = margin + Math.round(area.y * dpr);

      // Pixels da foto no tamanho em que ela aparece, para tirar a cor de cada partícula.
      const sample = document.createElement("canvas");
      sample.width = w;
      sample.height = h;
      const sctx = sample.getContext("2d", { willReadFrequently: true });
      if (!sctx) return done(id);
      sctx.drawImage(image, 0, 0, w, h);
      const step = gridStep(w, h);
      const particles = makeParticles(sctx.getImageData(0, 0, w, h).data, w, h, step);

      const start = performance.now();
      const draw = (now: number) => {
        if (cancelled) return;
        const elapsed = now - start - HOLD_MS;
        ctx.clearRect(0, 0, el.width, el.height);
        ctx.globalAlpha = 1;
        if (elapsed < 0) {
          // Parada, igual ao último quadro: quando o vídeo nativo fechar, a troca não aparece.
          ctx.drawImage(image, ox, oy, w, h);
          frame = requestAnimationFrame(draw);
          return;
        }
        const t = elapsed / DUST_MS;
        if (t >= 1) return done(id);
        // A imagem inteira escurece e some suave enquanto a poeira se solta dela (sem buracos em
        // blocos nem moldura, que pareciam pixels bugados e um print da tela).
        const fade = imageAlpha(t);
        if (fade > 0) {
          ctx.globalAlpha = fade;
          ctx.filter = `brightness(${0.6 + 0.4 * fade})`;
          ctx.drawImage(image, ox, oy, w, h);
          ctx.filter = "none";
        }
        // Grãos finos com as cores da imagem, somando luz onde se juntam (o brilho da poeira do
        // iOS). O voo é em px da interface.
        const grain = GRAIN * dpr;
        ctx.globalCompositeOperation = "lighter";
        for (const p of particles) {
          const s = particleAt(p, t);
          if (typeof s === "string") continue;
          ctx.globalAlpha = s.alpha;
          ctx.fillStyle = p.color;
          ctx.fillRect(ox + p.x + (s.x - p.x) * dpr, oy + p.y + (s.y - p.y) * dpr, grain, grain);
        }
        ctx.globalCompositeOperation = "source-over";
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

  const box = canvasBox(rect, window.devicePixelRatio || 1);
  return (
    <canvas
      ref={canvas}
      aria-hidden
      data-testid="animacao-de-saida"
      className="pointer-events-none fixed z-30"
      style={{ left: box.left, top: box.top, width: box.width, height: box.height }}
    />
  );
}
