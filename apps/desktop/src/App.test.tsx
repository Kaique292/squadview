import { render, screen } from "@testing-library/react";
import { App } from "./App";

describe("App", () => {
  it("mostra o estado vazio quando ninguém está transmitindo", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Ninguém está transmitindo ainda" })).toBeInTheDocument();
  });

  it("deixa o botão de transmitir desabilitado até existir uma sala", () => {
    render(<App />);
    expect(screen.getByRole("button", { name: "Transmitir minha tela" })).toBeDisabled();
  });

  it("tem a navegação de salas acessível", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Salas" })).toBeInTheDocument();
  });
});
