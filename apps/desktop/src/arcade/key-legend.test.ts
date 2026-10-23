import { keyLegend } from "./key-legend";

describe("legenda das teclas do fliperama", () => {
  it("mostra o analógico e os botões com as teclas do mapa, letra antes de seta", () => {
    const legend = keyLegend({
      keys: {
        KeyW: "left-stick-up",
        ArrowUp: "left-stick-up",
        KeyA: "left-stick-left",
        KeyS: "left-stick-down",
        KeyD: "left-stick-right",
        Space: "a",
        KeyJ: "a",
        KeyK: "b",
        KeyL: "x",
        KeyI: "y",
        Enter: "start",
      },
    });
    expect(legend).toBe("WASD anda · J = A · K = B · L = X · I = Y · Enter = Start · mouse = analógico direito");
  });

  it("só setas no analógico; sem botões, só o que existe", () => {
    const legend = keyLegend({
      keys: {
        ArrowUp: "left-stick-up",
        ArrowLeft: "left-stick-left",
        ArrowDown: "left-stick-down",
        ArrowRight: "left-stick-right",
        Space: "a",
      },
    });
    expect(legend).toBe("↑←↓→ anda · Espaço = A · mouse = analógico direito");
    expect(keyLegend({ keys: {} })).toBe("mouse = analógico direito");
  });
});
