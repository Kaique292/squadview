import type { Cue } from "./sounds";

/** Um AudioContext de mentira: registra osciladores e ganhos e se foi retomado ou suspenso. */
class FakeContext {
  static made = 0;
  currentTime = 10;
  destination = {};
  oscillators: { type: string; freqs: number[]; ramps: number[]; started: number; stopped: number }[] = [];
  gains: { values: number[] }[] = [];
  resumed = 0;
  suspended = 0;
  constructor() {
    FakeContext.made++;
  }
  resume() {
    this.resumed++;
    return Promise.resolve();
  }
  suspend() {
    this.suspended++;
    return Promise.resolve();
  }
  createGain() {
    const g = {
      values: [] as number[],
      gain: {
        value: 0,
        setValueAtTime: (v: number) => g.values.push(v),
        exponentialRampToValueAtTime: (v: number) => g.values.push(v),
      },
      connect: (to: unknown) => to,
    };
    this.gains.push(g);
    return g;
  }
  createOscillator() {
    const o = { type: "", freqs: [] as number[], ramps: [] as number[], started: 0, stopped: 0 };
    this.oscillators.push(o);
    return {
      set type(t: string) {
        o.type = t;
      },
      frequency: {
        setValueAtTime: (f: number) => o.freqs.push(f),
        exponentialRampToValueAtTime: (f: number) => o.ramps.push(f),
      },
      connect: (to: unknown) => to,
      start: (t: number) => (o.started = t),
      stop: (t: number) => (o.stopped = t),
    };
  }
}

let last: FakeContext | null = null;

async function load(ctor: unknown = FakeContext) {
  vi.resetModules();
  (globalThis as Record<string, unknown>).AudioContext = ctor;
  const mod = await import("./sounds");
  return mod;
}

beforeEach(() => {
  vi.useFakeTimers();
  FakeContext.made = 0;
  last = null;
  class Tracked extends FakeContext {
    constructor() {
      super();
      // eslint-disable-next-line @typescript-eslint/no-this-alias
      last = this;
    }
  }
  (globalThis as Record<string, unknown>).Tracked = Tracked;
});

afterEach(() => {
  vi.useRealTimers();
  delete (globalThis as Record<string, unknown>).AudioContext;
  delete (globalThis as Record<string, unknown>).Tracked;
});

describe("síntese dos sons", () => {
  it("cada nota vira um seno e um triângulo, com a altura e o deslize certos", async () => {
    const { playCue, CUES } = await load((globalThis as Record<string, unknown>).Tracked);
    playCue("join", 1);
    const ac = last!;
    expect(ac.resumed).toBe(1);
    // Duas notas no "entrar": 4 osciladores (seno + triângulo de cada).
    expect(ac.oscillators.map((o) => o.type)).toEqual(["sine", "triangle", "sine", "triangle"]);
    expect(ac.oscillators.map((o) => o.freqs[0])).toEqual([587, 587, 880, 880]);
    // Só a segunda nota desliza (de 880 para 932).
    expect(ac.oscillators.map((o) => o.ramps)).toEqual([[], [], [932], [932]]);
    // Começa um pouco depois de agora e para depois de terminar.
    expect(ac.oscillators[0].started).toBeCloseTo(10.01);
    expect(ac.oscillators[2].stopped).toBeGreaterThan(ac.oscillators[2].started + CUES.join[1].dur);
  });

  it("o volume escala o pico de cada nota, e volume zero não toca nada", async () => {
    const { playCue } = await load((globalThis as Record<string, unknown>).Tracked);
    playCue("mute", 0);
    expect(last).toBeNull();
    playCue("mute", 1);
    const alto = Math.max(...last!.gains.flatMap((g) => g.values));
    playCue("mute", 0.5);
    const envelopes = last!.gains.filter((g) => g.values.length > 0);
    const baixo = Math.max(...envelopes[envelopes.length - 1].values);
    expect(baixo).toBeCloseTo(alto / 2);
  });

  it("um contexto só, e ele é suspenso depois de um tempo sem som (não gasta CPU com o jogo)", async () => {
    const { playCue } = await load((globalThis as Record<string, unknown>).Tracked);
    const cues: Cue[] = ["user-join", "stream-start"];
    for (const c of cues) playCue(c, 1);
    expect(FakeContext.made).toBe(1);
    expect(last!.suspended).toBe(0);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(last!.suspended).toBe(1);
  });

  it("sem Web Audio, ou se ele não abrir, não quebra", async () => {
    let mod = await load(undefined);
    expect(() => mod.playCue("join", 1)).not.toThrow();
    mod = await load(
      class {
        constructor() {
          throw new Error("sem dispositivo de áudio");
        }
      },
    );
    expect(() => mod.playCue("join", 1)).not.toThrow();
  });
});
