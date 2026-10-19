/** Evento que a janela principal manda para a splash com o andamento. */
export const SPLASH_EVENT = "splash-progress";

export interface SplashProgress {
  /** De 0 a 1. */
  value: number;
  text: string;
}

export interface BootStep {
  text: string;
  run: () => Promise<unknown>;
}

/** Tempo mínimo da splash: abaixo disso ela só pisca e ninguém vê a animação. */
export const MIN_SPLASH_MS = 1400;
/** Nenhuma etapa segura a abertura por mais que isso (ex.: GitHub fora do ar). */
export const STEP_TIMEOUT_MS = 4000;

/**
 * Abertura do app: roda as etapas (cada uma com prazo), avisa a splash do andamento, respeita o
 * tempo mínimo e então chama `ready` (que fecha a splash e mostra a janela principal).
 */
export async function boot(opts: {
  steps: BootStep[];
  progress: (p: SplashProgress) => void;
  ready: () => Promise<unknown>;
  now?: () => number;
  sleep?: (ms: number) => Promise<void>;
}) {
  const now = opts.now ?? (() => performance.now());
  const sleep = opts.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  const started = now();

  for (const [i, step] of opts.steps.entries()) {
    opts.progress({ value: 0.3 + (0.6 * i) / opts.steps.length, text: step.text });
    await Promise.race([step.run().catch(() => undefined), sleep(STEP_TIMEOUT_MS)]);
  }
  opts.progress({ value: 1, text: "Pronto!" });

  const left = MIN_SPLASH_MS - (now() - started);
  if (left > 0) await sleep(left);
  await opts.ready();
}
