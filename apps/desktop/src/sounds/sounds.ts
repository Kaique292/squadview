/**
 * Sons de aviso (entrar e sair da sala, mudo, surdo, transmissão), no estilo dos do Discord, mas
 * nossos: são sintetizados na hora com a Web Audio API, sem arquivo de áudio nenhum.
 *
 * Cada som é uma sequência curta de notas suaves (seno com um pouco de triângulo para dar corpo),
 * com ataque rápido e queda exponencial, às vezes deslizando de altura.
 */

export type Cue =
  | "join"
  | "leave"
  | "user-join"
  | "user-leave"
  | "mute"
  | "unmute"
  | "deafen"
  | "undeafen"
  | "stream-start"
  | "stream-stop"
  | "disconnect";

interface Note {
  /** Frequência inicial, em Hz. */
  freq: number;
  /** Desliza até esta frequência durante a nota. */
  to?: number;
  /** Início, em segundos a partir do disparo. */
  at: number;
  /** Duração até sumir, em segundos. */
  dur: number;
  /** Volume relativo (1 = cheio). */
  gain?: number;
}

/** As notas de cada som. Subir = entrar/ligar; descer = sair/desligar (como no Discord). */
export const CUES: Record<Cue, Note[]> = {
  // Duas notas subindo, a segunda mais longa: "blu-blim".
  join: [
    { freq: 587, at: 0, dur: 0.16 },
    { freq: 880, to: 932, at: 0.09, dur: 0.32 },
  ],
  leave: [
    { freq: 880, at: 0, dur: 0.16 },
    { freq: 587, to: 554, at: 0.09, dur: 0.32 },
  ],
  // Alguém entrou ou saiu: um "blip" só, mais baixo.
  "user-join": [{ freq: 698, to: 1047, at: 0, dur: 0.18, gain: 0.7 }],
  "user-leave": [{ freq: 1047, to: 698, at: 0, dur: 0.18, gain: 0.7 }],
  // Mudo: um toque curto descendo; tirar o mudo, subindo.
  mute: [{ freq: 660, to: 440, at: 0, dur: 0.12, gain: 0.8 }],
  unmute: [{ freq: 440, to: 660, at: 0, dur: 0.12, gain: 0.8 }],
  // Surdo: o mesmo, em duas notas e mais grave.
  deafen: [
    { freq: 523, at: 0, dur: 0.1, gain: 0.8 },
    { freq: 349, to: 330, at: 0.07, dur: 0.18, gain: 0.8 },
  ],
  undeafen: [
    { freq: 349, at: 0, dur: 0.1, gain: 0.8 },
    { freq: 523, to: 554, at: 0.07, dur: 0.18, gain: 0.8 },
  ],
  // Transmissão: arpejo de quatro notas (dó-mi-sol-dó) subindo ou descendo.
  "stream-start": [
    { freq: 523, at: 0, dur: 0.14, gain: 0.7 },
    { freq: 659, at: 0.06, dur: 0.14, gain: 0.7 },
    { freq: 784, at: 0.12, dur: 0.14, gain: 0.7 },
    { freq: 1047, at: 0.18, dur: 0.36, gain: 0.8 },
  ],
  "stream-stop": [
    { freq: 1047, at: 0, dur: 0.14, gain: 0.7 },
    { freq: 784, at: 0.06, dur: 0.14, gain: 0.7 },
    { freq: 659, at: 0.12, dur: 0.14, gain: 0.7 },
    { freq: 523, at: 0.18, dur: 0.36, gain: 0.8 },
  ],
  // A conexão caiu de vez: um "tuuun" descendo devagar.
  disconnect: [{ freq: 494, to: 247, at: 0, dur: 0.45, gain: 0.8 }],
};

/** Volume de pico de uma nota em 100%: os avisos ficam abaixo da voz da galera. */
const PEAK = 0.22;
/** Ataque das notas (sem estalo no começo). */
const ATTACK = 0.008;
/** Depois do último som, o contexto de áudio é suspenso para não gastar CPU ao lado do jogo. */
const IDLE_MS = 1500;

let ctx: AudioContext | null = null;
let idle: ReturnType<typeof setTimeout> | null = null;

function context(): AudioContext | null {
  if (ctx) return ctx;
  const Ctor = (globalThis as { AudioContext?: typeof AudioContext }).AudioContext;
  if (!Ctor) return null;
  try {
    ctx = new Ctor({ latencyHint: "interactive" });
  } catch {
    return null;
  }
  return ctx;
}

/** Toca um som. `volume` de 0 a 1. Sem Web Audio (testes), não faz nada. */
export function playCue(cue: Cue, volume: number) {
  if (volume <= 0) return;
  const ac = context();
  if (!ac) return;
  if (idle) clearTimeout(idle);
  void ac.resume().catch(() => {});

  const start = ac.currentTime + 0.01;
  let end = 0;
  for (const note of CUES[cue]) {
    const t0 = start + note.at;
    const t1 = t0 + note.dur;
    end = Math.max(end, note.at + note.dur);
    const peak = PEAK * volume * (note.gain ?? 1);

    const env = ac.createGain();
    env.gain.setValueAtTime(0.0001, t0);
    env.gain.exponentialRampToValueAtTime(peak, t0 + ATTACK);
    env.gain.exponentialRampToValueAtTime(0.0001, t1);
    env.connect(ac.destination);

    // Seno para a nota e um triângulo baixinho por cima para não soar "bipe de micro-ondas".
    for (const [type, level] of [
      ["sine", 1],
      ["triangle", 0.18],
    ] as const) {
      const osc = ac.createOscillator();
      osc.type = type;
      osc.frequency.setValueAtTime(note.freq, t0);
      if (note.to) osc.frequency.exponentialRampToValueAtTime(note.to, t1);
      const mix = ac.createGain();
      mix.gain.value = level;
      osc.connect(mix).connect(env);
      osc.start(t0);
      osc.stop(t1 + 0.02);
    }
  }
  idle = setTimeout(
    () => {
      idle = null;
      void ac.suspend().catch(() => {});
    },
    end * 1000 + IDLE_MS,
  );
}
