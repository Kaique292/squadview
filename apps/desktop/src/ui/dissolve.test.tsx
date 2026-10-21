import { act, render, screen } from "@testing-library/react";
import { Dissolve } from "./Dissolve";
import {
  fitInside,
  gridStep,
  imageAlpha,
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
  it("encaixa a foto na área como o vídeo nativo, com faixas nas sobras", () => {
    // 16:9 numa área mais alta: ocupa a largura e fica no meio na vertical.
    expect(fitInside(1280, 720, 640, 480)).toEqual({ x: 0, y: 60, width: 640, height: 360 });
    // Numa área mais larga: ocupa a altura e fica no meio na horizontal.
    expect(fitInside(1280, 720, 1000, 360)).toEqual({ x: 180, y: 0, width: 640, height: 360 });
  });

  it("limita a quantidade de partículas em vídeos grandes", () => {
    expect(gridStep(100, 100)).toBe(3);
    const step = gridStep(1920, 1080);
    expect((1920 / step) * (1080 / step)).toBeLessThanOrEqual(24_000);
  });

  it("cada bloco visível vira um grão dentro dele, com a cor da imagem mais clara", () => {
    const particles = makeParticles(image(), 4, 2, 1, () => 0.5);
    expect(particles).toHaveLength(2);
    expect(particles[0].x).toBeGreaterThanOrEqual(0);
    expect(particles[0].x).toBeLessThan(1);
    // 200,100,50 clareado 55% em direção ao branco.
    expect(particles[0].color).toBe("rgb(230,185,163)");
  });

  it("os grãos saem quase juntos, sem varrer da esquerda para a direita, e acabam a tempo", () => {
    const rgba = new Uint8ClampedArray(10 * 1 * 4).fill(255);
    const particles = makeParticles(rgba, 10, 1, 1, () => 0);
    expect(particles[0].delay).toBe(particles[9].delay);
    for (const p of makeParticles(rgba, 10, 1, 1)) expect(p.delay + p.life).toBeLessThanOrEqual(1);
  });

  it("a imagem some suave e por inteiro no começo da poeira", () => {
    expect(imageAlpha(0)).toBe(1);
    expect(imageAlpha(0.2)).toBeGreaterThan(0);
    expect(imageAlpha(0.2)).toBeLessThan(1);
    expect(imageAlpha(0.5)).toBe(0);
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
