import { act, render } from "@testing-library/react";
import { Backdrop } from "./Backdrop";
import { clipPathFor, onSeeThroughChange, seeThroughAreas, setSeeThrough } from "./see-through";

afterEach(() => {
  setSeeThrough("a", null);
  setSeeThrough("b", null);
});

describe("fundo transparente onde há vídeo", () => {
  it("sem vídeo, o fundo aparece inteiro", () => {
    expect(clipPathFor({ left: 0, top: 0, width: 800, height: 600 }, [])).toBe("none");
  });

  it("abre um buraco com os cantos do quadro, um pixel para dentro, relativo ao fundo", () => {
    const clip = clipPathFor({ left: 100, top: 50, width: 800, height: 600 }, [
      { x: 120, y: 70, width: 322, height: 182, radius: 9 },
    ]);
    expect(clip).toBe(
      'path(evenodd, "M0 0H800V600H0Z' +
        "M29 21H333A8 8 0 0 1 341 29V193A8 8 0 0 1 333 201H29A8 8 0 0 1 21 193V29A8 8 0 0 1 29 21Z" +
        '")',
    );
  });

  it("sem cantos (tela cheia), o buraco é um retângulo; vídeo fora do fundo não abre buraco", () => {
    expect(
      clipPathFor({ left: 0, top: 0, width: 800, height: 600 }, [{ x: 0, y: 0, width: 800, height: 600, radius: 0 }]),
    ).toBe('path(evenodd, "M0 0H800V600H0ZM1 1H799V599H1Z")');
    expect(
      clipPathFor({ left: 0, top: 0, width: 800, height: 600 }, [{ x: 900, y: 0, width: 100, height: 100, radius: 0 }]),
    ).toBe("none");
  });

  it("avisa só quando a área muda, e ignora área vazia", () => {
    const changed = vi.fn();
    const stop = onSeeThroughChange(changed);
    setSeeThrough("a", { x: 0, y: 0, width: 10, height: 10, radius: 0 });
    setSeeThrough("a", { x: 0, y: 0, width: 10, height: 10, radius: 0 });
    expect(changed).toHaveBeenCalledTimes(1);
    setSeeThrough("b", { x: 0, y: 0, width: 0, height: 10, radius: 0 });
    setSeeThrough("b", null);
    expect(changed).toHaveBeenCalledTimes(1);
    expect(seeThroughAreas()).toHaveLength(1);
    setSeeThrough("a", null);
    expect(changed).toHaveBeenCalledTimes(2);
    stop();
    setSeeThrough("a", { x: 0, y: 0, width: 10, height: 10, radius: 0 });
    expect(changed).toHaveBeenCalledTimes(2);
  });

  it("o fundo pinta a cor dele e fica transparente onde a live passa", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      width: 400,
      height: 300,
    } as DOMRect);
    const { container, unmount } = render(<Backdrop className="bg-black" />);
    const fundo = container.querySelector<HTMLElement>("[data-backdrop]")!;
    expect(fundo).toHaveClass("bg-black");
    expect(fundo.style.clipPath).toBe("none");
    act(() => setSeeThrough("a", { x: 10, y: 10, width: 100, height: 50, radius: 0 }));
    expect(fundo.style.clipPath).toContain("M11 11H109V59H11Z");
    act(() => setSeeThrough("a", null));
    expect(fundo.style.clipPath).toBe("none");
    unmount();
    vi.restoreAllMocks();
  });
});
