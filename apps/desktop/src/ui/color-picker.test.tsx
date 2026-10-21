import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { ColorPicker, hexToHsv, hsvToHex } from "./ColorPicker";

function Harness(props: { start: string; onChange?: (hex: string) => void }) {
  const [value, setValue] = useState(props.start);
  return (
    <>
      <ColorPicker
        value={value}
        onChange={(hex) => {
          setValue(hex);
          props.onChange?.(hex);
        }}
      />
      <button type="button" onClick={() => setValue("#3ba0ff")}>
        Azul
      </button>
    </>
  );
}

describe("seletor de cor", () => {
  it("converte entre #rrggbb e HSV sem perder a cor", () => {
    for (const hex of ["#ff5a4f", "#3ef0b0", "#7c3aed", "#000000", "#ffffff", "#808080"]) {
      expect(hsvToHex(hexToHsv(hex)!)).toBe(hex);
    }
    expect(hexToHsv("#ff0000")).toEqual({ h: 0, s: 1, v: 1 });
    expect(hexToHsv("vermelho")).toBeNull();
  });

  it("muda a cor pelas setas do teclado na matiz e na área", () => {
    const onChange = vi.fn();
    render(<Harness start="#ff0000" onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("slider", { name: "Matiz" }), { key: "ArrowRight", shiftKey: true });
    // 10% da volta: 36 graus, do vermelho para o laranja.
    expect(onChange).toHaveBeenLastCalledWith("#ff9900");
    fireEvent.keyDown(screen.getByRole("slider", { name: "Saturação e brilho" }), { key: "ArrowDown", shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith("#e68a00");
    expect(screen.getByRole("textbox", { name: "Código da cor" })).toHaveValue("#e68a00");
  });

  it("aceita o código digitado e desfaz o que não é cor", () => {
    const onChange = vi.fn();
    render(<Harness start="#ff0000" onChange={onChange} />);
    const codigo = screen.getByRole("textbox", { name: "Código da cor" });
    fireEvent.change(codigo, { target: { value: "#12AB34" } });
    fireEvent.blur(codigo);
    expect(onChange).toHaveBeenLastCalledWith("#12ab34");
    fireEvent.change(codigo, { target: { value: "verde" } });
    fireEvent.keyDown(codigo, { key: "Enter" });
    expect(codigo).toHaveValue("#12ab34");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("acompanha quando a cor muda por fora", () => {
    render(<Harness start="#ff0000" />);
    fireEvent.click(screen.getByRole("button", { name: "Azul" }));
    expect(screen.getByRole("textbox", { name: "Código da cor" })).toHaveValue("#3ba0ff");
  });

  it("arrastar na área escolhe a cor do ponto", () => {
    const onChange = vi.fn();
    render(<Harness start="#ff0000" onChange={onChange} />);
    const area = screen.getByRole("slider", { name: "Saturação e brilho" });
    area.getBoundingClientRect = () => ({ left: 0, top: 0, width: 100, height: 100 }) as DOMRect;
    // Canto de baixo: brilho zero, preto.
    fireEvent.pointerDown(area, { clientX: 50, clientY: 100, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith("#000000");
    // Canto de cima à direita: a cor pura da matiz.
    fireEvent.pointerMove(area, { clientX: 100, clientY: 0, buttons: 1 });
    expect(onChange).toHaveBeenLastCalledWith("#ff0000");
  });
});
