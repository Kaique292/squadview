import { act, render, screen } from "@testing-library/react";
import { Dissolve } from "./Dissolve";
import {
  gridStep,
  makeParticles,
  particleAt,
  playVideoExit,
  rememberVideoRect,
  useDissolveStore,
} from "./dissolve-effect";

/** Imagem 4×2: a coluna da esquerda opaca, o resto transparente. */
function image() {
  const rgba = new Uint8ClampedArray(4 * 2 * 4);
  for (const y of [0, 1]) {
    const i = (y * 4 + 0) * 4;
    rgba.set([200, 100, 50, 255], i);
  }
  return rgba;
}

beforeEach(() => useDissolveStore.setState({ effect: null }));

describe("animação de saída do vídeo", () => {
  it("limita a quantidade de partículas em vídeos grandes", () => {
    expect(gridStep(100, 100)).toBe(3);
    const step = gridStep(1920, 1080);
    expect((1920 / step) * (1080 / step)).toBeLessThanOrEqual(14_000);
  });

  it("cada bloco visível vira uma partícula com a cor da imagem", () => {
    const particles = makeParticles(image(), 4, 2, 1, () => 0.5);
    expect(particles).toHaveLength(2);
    expect(particles[0]).toMatchObject({ x: 0, y: 0, color: "rgb(200,100,50)" });
  });

  it("a varredura começa pela esquerda", () => {
    const rgba = new Uint8ClampedArray(10 * 1 * 4).fill(255);
    const particles = makeParticles(rgba, 10, 1, 1, () => 0);
    expect(particles[0].delay).toBeLessThan(particles[9].delay);
  });

  it("a partícula espera, voa subindo e apagando, e some", () => {
    const [p] = makeParticles(image(), 4, 2, 1, () => 0.5);
    expect(particleAt(p, p.delay - 0.01)).toBe("waiting");
    const flying = particleAt(p, p.delay + p.life / 2);
    if (typeof flying === "string") throw new Error("deveria estar voando");
    expect(flying.y).toBeLessThan(p.y);
    expect(flying.alpha).toBeGreaterThan(0);
    expect(flying.alpha).toBeLessThan(1);
    expect(particleAt(p, p.delay + p.life)).toBe("gone");
  });

  it("toca no lugar onde o vídeo estava e termina sozinha", () => {
    rememberVideoRect({ x: 10, y: 20, width: 640, height: 360 });
    render(<Dissolve />);
    act(() => playVideoExit("data:image/jpeg;base64,AAAA"));
    // No jsdom não há canvas: a animação termina na hora, sem travar a interface.
    expect(useDissolveStore.getState().effect).toBeNull();
    expect(screen.queryByTestId("animacao-de-saida")).not.toBeInTheDocument();
  });

  it("sem foto do último quadro, não faz nada", () => {
    playVideoExit(null);
    expect(useDissolveStore.getState().effect).toBeNull();
  });

  it("ignora a área vazia de quando o vídeo sai da tela", () => {
    rememberVideoRect({ x: 5, y: 5, width: 300, height: 200 });
    rememberVideoRect({ x: 0, y: 0, width: 0, height: 0 });
    useDissolveStore.getState().play("data:,");
    expect(useDissolveStore.getState().effect?.rect).toEqual({ x: 5, y: 5, width: 300, height: 200 });
  });
});
