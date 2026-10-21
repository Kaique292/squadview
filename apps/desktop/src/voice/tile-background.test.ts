import { TILE_GLOWS, tileGlows, tileGradient } from "./tile-background";

describe("fundos dos quadros da sala", () => {
  it("cada pessoa ganha uma cor diferente das outras da sala", () => {
    const ids = ["aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb", "cccccccccccccccc", "dddddddddddddddd", "eeeeeeeeeeeeeeee"];
    const glows = tileGlows(ids);
    expect(new Set(Object.values(glows)).size).toBe(ids.length);
  });

  it("todo mundo vê as mesmas cores, seja qual for a ordem da lista (cada PC começa por si)", () => {
    const naAna = tileGlows(["ana", "beto", "caio"]);
    const noBeto = tileGlows(["beto", "caio", "ana"]);
    expect(noBeto).toEqual(naAna);
  });

  it("com mais gente que cores, repete sem quebrar", () => {
    const ids = Array.from({ length: TILE_GLOWS.length + 3 }, (_, i) => `pessoa-${i}`);
    const glows = tileGlows(ids);
    expect(Object.keys(glows)).toHaveLength(ids.length);
    expect(new Set(Object.values(glows)).size).toBe(TILE_GLOWS.length);
  });

  it("o degradê é um brilho subindo de baixo, na cor escolhida", () => {
    const g = tileGradient("#6e4ff6");
    expect(g).toMatch(/^radial-gradient\(130% 100% at 50% 118%/);
    expect(g).toContain("#6e4ff6");
  });
});
