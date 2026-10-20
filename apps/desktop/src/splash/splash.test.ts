import { screen } from "@testing-library/react";
import { SPLASH_EVENT } from "./boot";

const { listen, show, handlers } = vi.hoisted(() => {
  const handlers = new Map<string, (e: { payload: unknown }) => void>();
  return {
    handlers,
    show: vi.fn(),
    listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
      handlers.set(name, cb);
      return () => handlers.delete(name);
    }),
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ show }) }));
vi.mock("@fontsource-variable/figtree", () => ({}));

/** O mesmo esqueleto do `splash.html`. */
const HTML = `
  <p id="status">Iniciando…</p>
  <div role="progressbar" aria-label="Carregando o SquadView"><div id="fill"></div></div>
  <p id="tip"></p>
`;

const bar = () => screen.getByRole("progressbar", { name: "Carregando o SquadView" });

/** Carrega a splash do zero (o script roda ao ser importado). */
async function loadSplash(opts: { tauri: boolean }) {
  vi.resetModules();
  if (opts.tauri) (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  await import("./splash");
}

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = HTML;
  handlers.clear();
  show.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(document, "fonts", { configurable: true, value: { ready: Promise.resolve() } });
});

afterEach(() => {
  vi.useRealTimers();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  document.body.innerHTML = "";
});

describe("splash de abertura", () => {
  it("mostra uma dica e a barra anda sozinha devagar, até 30%", async () => {
    await loadSplash({ tauri: false });
    expect(document.getElementById("tip")!.textContent).toMatch(/^Dica: /);
    vi.advanceTimersByTime(250);
    expect(bar()).toHaveAttribute("aria-valuenow", "7");
    vi.advanceTimersByTime(250 * 50);
    expect(bar()).toHaveAttribute("aria-valuenow", "30");
    expect(document.getElementById("fill")!.style.width).toBe("30%");
  });

  it("no app, aparece quando a fonte carrega e segue o andamento da janela principal", async () => {
    await loadSplash({ tauri: true });
    await vi.advanceTimersByTimeAsync(30);
    expect(show).toHaveBeenCalled();
    expect(listen).toHaveBeenCalledWith(SPLASH_EVENT, expect.any(Function));

    handlers.get(SPLASH_EVENT)!({ payload: { value: 0.6, text: "Conectando…" } });
    expect(bar()).toHaveAttribute("aria-valuenow", "60");
    expect(screen.getByText("Conectando…")).toBeInTheDocument();
    // Depois do primeiro aviso, a barra para de andar sozinha.
    vi.advanceTimersByTime(1000);
    expect(bar()).toHaveAttribute("aria-valuenow", "60");
    // Nunca volta para trás nem passa de 100%.
    handlers.get(SPLASH_EVENT)!({ payload: { value: 0.4, text: "" } });
    expect(bar()).toHaveAttribute("aria-valuenow", "60");
    expect(screen.getByText("Conectando…")).toBeInTheDocument();
    handlers.get(SPLASH_EVENT)!({ payload: { value: 1.5, text: "Pronto!" } });
    expect(bar()).toHaveAttribute("aria-valuenow", "100");
  });

  it("se a fonte não carregar, a janela aparece mesmo assim", async () => {
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { ready: Promise.reject(new Error("sem fonte")) },
    });
    show.mockRejectedValue(new Error("janela fechada"));
    await loadSplash({ tauri: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(show).toHaveBeenCalled();
  });
});
