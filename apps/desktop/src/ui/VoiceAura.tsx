import { type CSSProperties, type ReactNode, memo, useId } from "react";
import type { SpeakingEffect, SpeakingEffectKind } from "../voice/speaking-effect";

/** Quanto o efeito passa do tamanho da foto (o efeito fica atrás dela, só a borda aparece). */
export const AURA_SCALE = 1.7;
/** Raio da foto no desenho (o SVG vai de -50 a 50). */
const INNER = 50 / AURA_SCALE;

/**
 * Efeito de fala em volta da foto (ADR 0015). É um SVG montado uma vez, animado só por CSS
 * (rotação e escala, que a GPU faz): nada é recalculado a cada quadro. Parado e invisível quando
 * a pessoa não fala; aparece e anima enquanto ela fala.
 */
export const VoiceAura = memo(function VoiceAura(props: { effect: SpeakingEffect; size: number; active: boolean }) {
  const box = Math.round(props.size * AURA_SCALE);
  return (
    <svg
      aria-hidden
      data-effect={props.effect.kind}
      data-active={props.active || undefined}
      viewBox="-50 -50 100 100"
      width={box}
      height={box}
      className="voice-aura"
      style={{ "--aura": props.effect.color } as CSSProperties}
    >
      <AuraShape kind={props.effect.kind} color={props.effect.color} weight={weightFor(props.size)} />
    </svg>
  );
});

/**
 * Quanto engrossar pontos e traços: numa foto pequena (a lista da sala, 24 px) o desenho encolhe
 * junto e os detalhes finos sumiriam.
 */
function weightFor(size: number) {
  return Math.min(2.4, Math.max(1, 56 / size));
}

const AuraShape = memo(function AuraShape(props: { kind: SpeakingEffectKind; color: string; weight: number }) {
  const id = useId();
  switch (props.kind) {
    case "dots":
      return <Dots weight={props.weight} />;
    case "waves":
      return <Waves color={props.color} weight={props.weight} />;
    case "equalizer":
      return <Equalizer weight={props.weight} />;
    case "rays":
      return <Rays id={id} color={props.color} weight={props.weight} />;
  }
});

/** Números "ao acaso", mas sempre os mesmos: o efeito não muda de cara a cada vez que aparece. */
function seeded(seed: number) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const f = (n: number) => n.toFixed(2);

function Spin(props: { seconds: number; reverse?: boolean; children: ReactNode }) {
  return (
    <g className={props.reverse ? "aura-spin-reverse" : "aura-spin"} style={{ animationDuration: `${props.seconds}s` }}>
      {props.children}
    </g>
  );
}

/** Esfera de pontos: anéis de pontos inclinados, ondulando e girando, com frente e fundo. */
function Dots(props: { weight: number }) {
  const rings = Array.from({ length: 6 }, (_, ring) => {
    const squash = 0.25 + (ring / 5) * 0.75;
    const tilt = ring * 31;
    const dots = Array.from({ length: 44 }, (_, i) => {
      const t = (i / 44) * Math.PI * 2;
      const r = 43 + 3.5 * Math.sin(3 * t + ring);
      // Os pontos "de trás" da esfera ficam mais apagados.
      const depth = 0.35 + 0.65 * ((Math.sin(t) + 1) / 2);
      return (
        <circle
          key={i}
          cx={f(r * Math.cos(t))}
          cy={f(r * squash * Math.sin(t))}
          r={f((0.55 + 0.45 * depth) * props.weight)}
          opacity={f(depth)}
        />
      );
    });
    return (
      <Spin key={ring} seconds={7 + ring * 1.7} reverse={ring % 2 === 1}>
        <g transform={`rotate(${tilt})`}>{dots}</g>
      </Spin>
    );
  });
  return (
    <g className="aura-pulse" fill="var(--aura)">
      {rings}
    </g>
  );
}

/** Ondas de linhas: dois feixes de linhas finas ondulando, girando em sentidos opostos. */
function Waves(props: { color: string; weight: number }) {
  const band = (shift: number) =>
    Array.from({ length: 12 }, (_, line) => {
      const p = line * 0.14 + shift;
      const points = Array.from({ length: 73 }, (_, i) => {
        const t = (i / 72) * Math.PI * 2;
        const r = 40 + 4 * Math.sin(3 * t + p) + 2.5 * Math.sin(5 * t - 2 * p);
        return `${f(r * Math.cos(t))},${f(r * Math.sin(t))}`;
      });
      return <polyline key={line} points={points.join(" ")} />;
    });
  return (
    <g className="aura-pulse" fill="none" strokeWidth={f(0.4 * props.weight)} strokeOpacity={0.75}>
      <Spin seconds={11}>
        <g stroke="var(--aura)">{band(0)}</g>
      </Spin>
      <Spin seconds={14} reverse>
        {/* O segundo feixe é a mesma cor, mais clara, para dar volume como nas fitas. */}
        <g stroke={`color-mix(in srgb, ${props.color} 55%, white)`}>{band(1.6)}</g>
      </Spin>
    </g>
  );
}

/** Equalizador: barrinhas saindo da foto, cada uma subindo e descendo no seu ritmo. */
function Equalizer(props: { weight: number }) {
  const random = seeded(7);
  // Barras mais grossas, menos barras: em foto pequena elas não viram um anel cheio.
  const count = props.weight > 1.6 ? 24 : 40;
  const bars = Array.from({ length: count }, (_, i) => {
    const length = 15 * (0.55 + random() * 0.45);
    return (
      <g key={i} transform={`rotate(${f((i * 360) / count)})`}>
        <rect
          className="aura-bar"
          x={f(-0.9 * props.weight)}
          y={f(-(INNER + 2 + length))}
          width={f(1.8 * props.weight)}
          height={f(length)}
          rx={f(0.9 * props.weight)}
          style={{ animationDuration: `${f(0.45 + random() * 0.5)}s`, animationDelay: `-${f(random())}s` }}
        />
      </g>
    );
  });
  return <g fill="var(--aura)">{bars}</g>;
}

/** Raios: riscos finos irradiando da foto e sumindo na ponta, girando devagar. */
function Rays(props: { id: string; color: string; weight: number }) {
  const random = seeded(11);
  const gradient = `${props.id}-raio`;
  const rays = Array.from({ length: 72 }, (_, i) => {
    const angle = (i / 72) * Math.PI * 2;
    const from = INNER + 1.5;
    const to = from + 8 + random() * 13;
    return (
      <line
        key={i}
        x1={f(from * Math.cos(angle))}
        y1={f(from * Math.sin(angle))}
        x2={f(to * Math.cos(angle))}
        y2={f(to * Math.sin(angle))}
      />
    );
  });
  return (
    <>
      <defs>
        <radialGradient id={gradient} gradientUnits="userSpaceOnUse" cx={0} cy={0} r={50}>
          <stop offset={f(INNER / 50)} stopColor={props.color} />
          <stop offset="1" stopColor={props.color} stopOpacity={0} />
        </radialGradient>
      </defs>
      <g className="aura-pulse">
        <Spin seconds={18}>
          <g stroke={`url(#${gradient})`} strokeWidth={f(0.9 * props.weight)} strokeLinecap="round">
            {rays}
          </g>
        </Spin>
      </g>
    </>
  );
}
