import { render, screen } from "@testing-library/react";
import { App } from "./App";

describe("App", () => {
  it("fora de uma sala, a página inicial é Amigos, com o ícone marcado", () => {
    render(<App />);
    expect(screen.getByRole("region", { name: "Amigos" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Amigos" })).toHaveAttribute("aria-current", "page");
  });

  it("tem a navegação de salas acessível, com o mascote no topo", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Salas" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "SquadView" })).toBeInTheDocument();
  });
});
