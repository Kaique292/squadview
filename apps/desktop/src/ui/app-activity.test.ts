import { appIdle, watchAppActivity } from "./app-activity";

let focused = true;
let visibility: DocumentVisibilityState = "visible";

beforeEach(() => {
  focused = true;
  visibility = "visible";
  vi.spyOn(document, "hasFocus").mockImplementation(() => focused);
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
});

afterEach(() => {
  vi.restoreAllMocks();
  delete document.documentElement.dataset.idle;
});

const idle = () => "idle" in document.documentElement.dataset;

describe("app em uso (pausa dos efeitos contínuos, ADR 0034)", () => {
  it("em uso só com a janela visível e com foco", () => {
    expect(appIdle()).toBe(false);
    focused = false;
    expect(appIdle()).toBe(true);
    focused = true;
    visibility = "hidden";
    expect(appIdle()).toBe(true);
  });

  it("perder o foco para o jogo marca o html, e voltar desmarca", () => {
    const stop = watchAppActivity();
    expect(idle()).toBe(false);
    focused = false;
    window.dispatchEvent(new Event("blur"));
    expect(idle()).toBe(true);
    focused = true;
    window.dispatchEvent(new Event("focus"));
    expect(idle()).toBe(false);
    stop();
  });

  it("minimizar marca, mesmo sem o evento de foco", () => {
    const stop = watchAppActivity();
    visibility = "hidden";
    document.dispatchEvent(new Event("visibilitychange"));
    expect(idle()).toBe(true);
    visibility = "visible";
    document.dispatchEvent(new Event("visibilitychange"));
    expect(idle()).toBe(false);
    stop();
  });

  it("já abre marcado se começar sem foco, e parar de acompanhar limpa a marca", () => {
    focused = false;
    const stop = watchAppActivity();
    expect(idle()).toBe(true);
    stop();
    expect(idle()).toBe(false);
    // Depois de parar, os eventos não mexem mais.
    window.dispatchEvent(new Event("blur"));
    expect(idle()).toBe(false);
  });
});
