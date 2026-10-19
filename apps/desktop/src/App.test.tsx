import { render, screen } from "@testing-library/react";
import { App } from "./App";

describe("App", () => {
  it("mostra o estado vazio quando ninguém está transmitindo", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Ninguém está transmitindo ainda" })).toBeInTheDocument();
  });

  it("oferece o botão de transmitir quando ninguém está ao vivo", () => {
    render(<App />);
    expect(screen.getByRole("button", { name: "Transmitir minha tela" })).toBeEnabled();
  });

  it("tem a navegação de salas acessível, com o mascote no topo", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Salas" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "SquadView" })).toBeInTheDocument();
  });
});
