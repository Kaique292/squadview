import { render, screen, within } from "@testing-library/react";
import { App } from "./App";

describe("App", () => {
  it("fora de uma sala, a página inicial é Amigos, com o ícone marcado", () => {
    render(<App />);
    expect(screen.getByRole("region", { name: "Amigos" })).toBeInTheDocument();
    const rail = screen.getByRole("navigation", { name: "Salas" });
    expect(within(rail).getByRole("button", { name: "Amigos" })).toHaveAttribute("aria-current", "page");
    // A lateral é a das mensagens diretas, com "Amigos" marcado e nenhuma conversa ainda.
    const dms = screen.getByRole("complementary", { name: "Mensagens diretas" });
    expect(within(dms).getByRole("button", { name: "Amigos" })).toHaveAttribute("aria-current", "page");
    expect(within(dms).getByText("Nenhuma conversa ainda.")).toBeInTheDocument();
  });

  it("tem a navegação de salas acessível, com o mascote no topo", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Salas" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "SquadView" })).toBeInTheDocument();
  });
});
