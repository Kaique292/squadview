import { act, render, screen } from "@testing-library/react";
import { Dissolve } from "./Dissolve";
import { DUST_MS, HOLD_MS, useDissolveStore } from "./dissolve-effect";

/**
 * O jsdom não tem canvas nem carrega imagens: aqui eles são trocados por versões de mentira para
 * rodar a animação quadro a quadro, com o relógio na mão (sem depender de tempo real).
 */

function fakeContext() {
  return {
    scale: vi.fn(),
    clearRect: vi.fn(),
    drawImage: vi.fn(),
    strokeRect: vi.fn(),
    fillRect: vi.fn(),
    getImageData: vi.fn((_x: number, _y: number, w: number, h: number) => ({
      data: new Uint8ClampedArray(w * h * 4).fill(255),
    })),
    globalAlpha: 1,
    globalCompositeOperation: "source-over",
    filter: "none",
    strokeStyle: "",
    fillStyle: "",
    lineWidth: 1,
  };
}

let ctx: ReturnType<typeof fakeContext>;
/** `null` faz o canvas de amostra (o segundo `getContext`) falhar. */
let sampleCtx: ReturnType<typeof fakeContext> | null;
let frames: FrameRequestCallback[];
let imageFails: boolean;

class FakeImage {
  naturalWidth = 80;
  naturalHeight = 40;
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  set src(_value: string) {
    queueMicrotask(() => (imageFails ? this.onerror?.() : this.onload?.()));
  }
}

beforeEach(() => {
  ctx = fakeContext();
  sampleCtx = fakeContext();
  frames = [];
  imageFails = false;
  useDissolveStore.setState({ effect: null });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (
    this: HTMLCanvasElement,
    _type: string,
    options?: unknown,
  ) {
    return (options ? sampleCtx : ctx) as unknown as RenderingContext;
  } as typeof HTMLCanvasElement.prototype.getContext);
  vi.stubGlobal("Image", FakeImage);
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  vi.spyOn(performance, "now").mockReturnValue(0);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Roda o próximo quadro pedido no instante `now` (ms desde o começo). */
function frame(now: number) {
  const cb = frames.shift();
  if (!cb) throw new Error("nenhum quadro pedido");
  act(() => cb(now));
}

async function play() {
  render(<Dissolve />);
  act(() => useDissolveStore.getState().play("data:image/jpeg;base64,AAAA", { x: 0, y: 300, width: 40, height: 20 }));
  // A foto "carrega" num microtask.
  await act(async () => {});
}

describe("animação de saída desenhada no canvas", () => {
  it("segura a foto parada, desfaz em poeira e termina sozinha", async () => {
    await play();
    expect(screen.getByTestId("animacao-de-saida")).toBeInTheDocument();

    // Parada: a foto inteira, sem efeito.
    frame(HOLD_MS / 2);
    expect(ctx.drawImage).toHaveBeenCalledTimes(1);

    // Começo da poeira: a imagem ainda aparece, mais apagada, e os grãos saem com a cor dela.
    frame(HOLD_MS + DUST_MS * 0.2);
    expect(ctx.drawImage).toHaveBeenCalledTimes(2);
    expect(ctx.fillRect).toHaveBeenCalled();
    expect(ctx.fillStyle).toBe("rgb(255,255,255)");
    expect(ctx.filter).toBe("none");
    expect(ctx.globalCompositeOperation).toBe("source-over");

    // Depois: a imagem já sumiu por inteiro e só a poeira continua.
    frame(HOLD_MS + DUST_MS * 0.6);
    expect(ctx.drawImage).toHaveBeenCalledTimes(2);

    // Fim: a animação sai da tela e não pede mais quadros.
    frame(HOLD_MS + DUST_MS);
    expect(useDissolveStore.getState().effect).toBeNull();
    expect(screen.queryByTestId("animacao-de-saida")).not.toBeInTheDocument();
    expect(frames).toHaveLength(0);
  });

  it("não tem moldura branca nem buracos em blocos na imagem", async () => {
    await play();
    for (const at of [HOLD_MS / 2, HOLD_MS + 1, HOLD_MS + DUST_MS * 0.1, HOLD_MS + DUST_MS * 0.3]) frame(at);
    expect(ctx.strokeRect).not.toHaveBeenCalled();
    // Só a limpeza do canvas inteiro a cada quadro: nada é apagado em pedaços.
    for (const [x, y] of ctx.clearRect.mock.calls) expect([x, y]).toEqual([0, 0]);
  });

  it("com a escala do Windows em 125%, o canvas cai em pixels físicos inteiros (sem borrar)", async () => {
    vi.stubGlobal("devicePixelRatio", 1.25);
    render(<Dissolve />);
    act(() => useDissolveStore.getState().play("data:,", { x: 10.3, y: 300.7, width: 333.3, height: 187.5 }));
    await act(async () => {});
    const canvas = screen.getByTestId("animacao-de-saida") as HTMLCanvasElement;
    expect(parseFloat(canvas.style.width) * 1.25).toBeCloseTo(canvas.width, 6);
    expect(parseFloat(canvas.style.height) * 1.25).toBeCloseTo(canvas.height, 6);
    expect(parseFloat(canvas.style.left) * 1.25).toBeCloseTo(Math.round(10.3 * 1.25), 6);
    expect(ctx.scale).not.toHaveBeenCalled();
  });

  it("com menos movimento pedido no Windows, termina na hora", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce") }));
    render(<Dissolve />);
    act(() => useDissolveStore.getState().play("data:,", { x: 0, y: 0, width: 40, height: 20 }));
    expect(useDissolveStore.getState().effect).toBeNull();
  });

  it("se a foto não carregar, termina sem travar", async () => {
    imageFails = true;
    await play();
    expect(useDissolveStore.getState().effect).toBeNull();
  });

  it("sem como ler os pixels da foto, termina sem animar", async () => {
    sampleCtx = null;
    await play();
    expect(useDissolveStore.getState().effect).toBeNull();
    expect(frames).toHaveLength(0);
  });

  it("um vídeo novo saindo no meio cancela a animação anterior", async () => {
    await play();
    frame(HOLD_MS + DUST_MS / 4);
    act(() => useDissolveStore.getState().play("data:,outra", { x: 0, y: 300, width: 40, height: 20 }));
    expect(cancelAnimationFrame).toHaveBeenCalled();
    // O quadro que já estava pedido para a animação antiga não desenha mais nada.
    const before = ctx.drawImage.mock.calls.length;
    frame(HOLD_MS + DUST_MS / 2);
    expect(ctx.drawImage.mock.calls.length).toBe(before);
  });

  it("se a tela some antes da foto carregar, não começa a animar", async () => {
    const { unmount } = render(<Dissolve />);
    act(() => useDissolveStore.getState().play("data:,", { x: 0, y: 0, width: 40, height: 20 }));
    unmount();
    await act(async () => {});
    expect(frames).toHaveLength(0);
  });
});
