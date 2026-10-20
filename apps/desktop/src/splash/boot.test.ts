import { MIN_SPLASH_MS, STEP_TIMEOUT_MS, type SplashProgress, boot } from "./boot";

function clock() {
  let t = 0;
  return {
    now: () => t,
    sleep: (ms: number) => {
      t += ms;
      return Promise.resolve();
    },
  };
}

describe("abertura com splash", () => {
  it("avisa cada etapa, termina em 100% e só então mostra o app", async () => {
    const seen: SplashProgress[] = [];
    const order: string[] = [];
    const c = clock();
    await boot({
      steps: [
        { text: "Um", run: () => Promise.resolve(order.push("um")) },
        { text: "Dois", run: () => Promise.resolve(order.push("dois")) },
      ],
      progress: (p) => seen.push(p),
      ready: async () => void order.push("pronto"),
      ...c,
    });
    expect(seen.map((p) => p.text)).toEqual(["Um", "Dois", "Pronto!"]);
    expect(seen.map((p) => p.value)).toEqual([0.3, 0.6, 1]);
    expect(order).toEqual(["um", "dois", "pronto"]);
  });

  it("segura a splash o tempo mínimo para a animação aparecer", async () => {
    const c = clock();
    await boot({ steps: [], progress: () => {}, ready: async () => {}, ...c });
    expect(c.now()).toBe(MIN_SPLASH_MS);
  });

  it("uma etapa que falha não impede o app de abrir", async () => {
    const ready = vi.fn(async () => {});
    await boot({
      steps: [{ text: "Quebra", run: () => Promise.reject(new Error("sem internet")) }],
      progress: () => {},
      ready,
      ...clock(),
    });
    expect(ready).toHaveBeenCalled();
  });

  it("uma etapa travada tem prazo", async () => {
    const ready = vi.fn(async () => {});
    const c = clock();
    await boot({
      steps: [{ text: "Trava", run: () => new Promise(() => {}) }],
      progress: () => {},
      ready,
      ...c,
    });
    expect(ready).toHaveBeenCalled();
    expect(c.now()).toBeGreaterThanOrEqual(STEP_TIMEOUT_MS);
  });

  it("a etapa sem prazo (baixar a atualização) espera terminar antes de abrir o app", async () => {
    const ready = vi.fn(async () => {});
    let finish = () => {};
    const download = new Promise<void>((r) => (finish = r));
    const opened = boot({
      steps: [{ text: "Atualizando…", run: () => download, untimed: true }],
      progress: () => {},
      ready,
      ...clock(),
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(ready).not.toHaveBeenCalled();
    finish();
    await opened;
    expect(ready).toHaveBeenCalled();
  });
});
