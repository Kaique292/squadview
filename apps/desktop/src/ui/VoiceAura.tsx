import { type CSSProperties, type ReactNode, memo, useId } from "react";
import type { SpeakingEffect, SpeakingEffectKind } from "../voice/speaking-effect";

/** Quanto o efeito passa do tamanho da foto (o efeito fica atrás dela, só a borda aparece). */
export const AURA_SCALE = 1.7;
/** Raio da foto no desenho (cada camada vai de -50 a 50). */
const INNER = 50 / AURA_SCALE;

/**
 * Efeito de fala em volta da foto (ADR 0015). Cada parte que gira é um `<svg>` próprio, e quem
 * gira, pulsa e respira é o próprio elemento (rotação, escala e opacidade): a animação fica toda
 * na GPU, sem redesenhar o SVG a cada quadro. Animar algo *dentro* do SVG fazia o Chromium
 * redesenhar tudo e custava até 40% da thread da interface com a sala cheia falando.
 * Parado e invisível quando a pessoa não fala; aparece e anima enquanto ela fala.
 */
export const VoiceAura = memo(function VoiceAura(props: { effect: SpeakingEffect; size: number; active: boolean }) {
  const box = Math.round(props.size * AURA_SCALE);
  return (
    <span
      aria-hidden
      data-effect={props.effect.kind}
      data-active={props.active || undefined}
      className="voice-aura"
      style={{ width: box, height: box, "--aura": props.effect.color } as CSSProperties}
    >
      <span className="aura-pulse">
        <AuraShape kind={props.effect.kind} color={props.effect.color} weight={weightFor(props.size)} />
      </span>
    </span>
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

/** Uma camada do efeito: um SVG inteiro que gira (ou respira) sozinho. */
function Layer(props: {
  animation: "aura-spin" | "aura-spin-reverse" | "aura-breathe";
  seconds: number;
  /** Começa já no meio do ciclo (s), para as camadas não andarem juntas. */
  delay?: number;
  children: ReactNode;
}) {
  return (
    <svg
      viewBox="-50 -50 100 100"
      className={`aura-layer ${props.animation}`}
      style={{
        animationDuration: `${f(props.seconds)}s`,
        animationDelay: props.delay ? `-${f(props.delay)}s` : undefined,
      }}
    >
      {props.children}
    </svg>
  );
}

/** Esfera de pontos: anéis de pontos inclinados, ondulando e girando, com frente e fundo. */
function Dots(props: { weight: number }) {
  return Array.from({ length: 6 }, (_, ring) => {
    const squash = 0.25 + (ring / 5) * 0.75;
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
      <Layer key={ring} animation={ring % 2 === 1 ? "aura-spin-reverse" : "aura-spin"} seconds={7 + ring * 1.7}>
        <g transform={`rotate(${ring * 31})`} fill="var(--aura)">
          {dots}
        </g>
      </Layer>
    );
  });
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
  const line = { fill: "none", strokeWidth: f(0.4 * props.weight), strokeOpacity: 0.75 };
  return (
    <>
      <Layer animation="aura-spin" seconds={11}>
        <g stroke="var(--aura)" {...line}>
          {band(0)}
        </g>
      </Layer>
      <Layer animation="aura-spin-reverse" seconds={14}>
        {/* O segundo feixe é a mesma cor, mais clara, para dar volume como nas fitas. */}
        <g stroke={`color-mix(in srgb, ${props.color} 55%, white)`} {...line}>
          {band(1.6)}
        </g>
      </Layer>
    </>
  );
}

/**
 * Equalizador: barrinhas saindo da foto, em grupos que crescem e encolhem cada um no seu ritmo.
 * Ao encolher, o pé da barra entra atrás da foto: parece a barra baixando, sem animar uma por uma.
 */
function Equalizer(props: { weight: number }) {
  const random = seeded(7);
  // Barras mais grossas, menos barras: em foto pequena elas não viram um anel cheio.
  const count = props.weight > 1.6 ? 24 : 40;
  const groups = 6;
  const bars = Array.from({ length: count }, (_, i) => {
    const length = 15 * (0.55 + random() * 0.45);
    return (
      <rect
        key={i}
        transform={`rotate(${f((i * 360) / count)})`}
        x={f(-0.9 * props.weight)}
        y={f(-(INNER + 2 + length))}
        width={f(1.8 * props.weight)}
        height={f(length)}
        rx={f(0.9 * props.weight)}
      />
    );
  });
  return Array.from({ length: groups }, (_, g) => (
    // Ciclo longo com vários batimentos (ver `aura-breathe`): o ritmo é rápido, as voltas são poucas.
    <Layer key={g} animation="aura-breathe" seconds={3.2 + random() * 1.4} delay={random() * 4}>
      <g fill="var(--aura)">{bars.filter((_, i) => i % groups === g)}</g>
    </Layer>
  ));
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
    <Layer animation="aura-spin" seconds={18}>
      <defs>
        <radialGradient id={gradient} gradientUnits="userSpaceOnUse" cx={0} cy={0} r={50}>
          <stop offset={f(INNER / 50)} stopColor={props.color} />
          <stop offset="1" stopColor={props.color} stopOpacity={0} />
        </radialGradient>
      </defs>
      <g stroke={`url(#${gradient})`} strokeWidth={f(0.9 * props.weight)} strokeLinecap="round">
        {rays}
      </g>
    </Layer>
  );
}
