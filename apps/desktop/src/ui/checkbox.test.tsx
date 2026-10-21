import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { Checkbox, CheckMark } from "./Checkbox";

function Lembrar(props: { start?: boolean; disabled?: boolean }) {
  const [on, setOn] = useState(props.start ?? false);
  return (
    <Checkbox checked={on} onChange={setOn} disabled={props.disabled}>
      Lembrar minha escolha
    </Checkbox>
  );
}

const mark = () => document.querySelector(".check-mark")!;

describe("checkbox do app", () => {
  it("marca e desmarca pelo rótulo, animando só quando muda", () => {
    render(<Lembrar />);
    const box = screen.getByRole("checkbox", { name: "Lembrar minha escolha" });
    expect(box).not.toBeChecked();
    // Ao aparecer, sem animação.
    expect(mark()).not.toHaveAttribute("data-animate");
    expect(mark()).not.toHaveAttribute("data-checked");

    fireEvent.click(screen.getByText("Lembrar minha escolha"));
    expect(box).toBeChecked();
    expect(mark()).toHaveAttribute("data-checked");
    expect(mark()).toHaveAttribute("data-animate");

    // Desmarcar também anima (o ✓ se apaga e a caixa esvazia).
    fireEvent.click(box);
    expect(box).not.toBeChecked();
    expect(mark()).not.toHaveAttribute("data-checked");
    expect(mark()).toHaveAttribute("data-animate");
  });

  it("já marcado aparece cheio e parado; desabilitado não muda", () => {
    render(<Lembrar start disabled />);
    const box = screen.getByRole("checkbox", { name: "Lembrar minha escolha" });
    expect(box).toBeChecked();
    expect(box).toBeDisabled();
    expect(mark()).toHaveAttribute("data-checked");
    expect(mark()).not.toHaveAttribute("data-animate");
  });

  it("a caixa sozinha (dos menus) fica escondida do leitor de tela", () => {
    const { rerender } = render(<CheckMark checked={false} />);
    expect(mark()).toHaveAttribute("aria-hidden", "true");
    rerender(<CheckMark checked />);
    expect(mark()).toHaveAttribute("data-animate");
  });
});
