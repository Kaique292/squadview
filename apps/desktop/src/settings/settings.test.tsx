import { fireEvent, render, screen } from "@testing-library/react";
import { DEFAULT_GLASS, applyGlass, useAppearanceStore } from "./appearance-store";
import { SettingsDialog } from "./SettingsDialog";

const glassVar = () => document.documentElement.style.getPropertyValue("--glass");

beforeEach(() => {
  localStorage.clear();
  useAppearanceStore.setState({ glass: DEFAULT_GLASS });
  applyGlass(DEFAULT_GLASS);
});

function openSettings() {
  render(<SettingsDialog trigger={<button type="button">Configurações</button>} />);
  fireEvent.click(screen.getByRole("button", { name: "Configurações" }));
}

describe("configurações de aparência", () => {
  it("começa no vidro padrão", () => {
    openSettings();
    expect(screen.getByRole("slider", { name: "Efeito de vidro" })).toHaveValue(String(DEFAULT_GLASS));
    expect(glassVar()).toBe(String(DEFAULT_GLASS / 100));
  });

  it("muda a intensidade na hora e salva", () => {
    openSettings();
    fireEvent.change(screen.getByRole("slider", { name: "Efeito de vidro" }), { target: { value: "75" } });
    expect(screen.getByText("75%")).toBeInTheDocument();
    expect(glassVar()).toBe("0.75");
    expect(document.documentElement).toHaveClass("glass");
    expect(localStorage.getItem("squadview.vidro")).toBe("75");
  });

  it("em 0% o app fica sólido", () => {
    openSettings();
    fireEvent.change(screen.getByRole("slider", { name: "Efeito de vidro" }), { target: { value: "0" } });
    expect(glassVar()).toBe("0");
    // Sem vidro, o fundo da janela fica sólido.
    expect(document.documentElement).not.toHaveClass("glass");
  });

  it("volta ao padrão", () => {
    openSettings();
    fireEvent.change(screen.getByRole("slider", { name: "Efeito de vidro" }), { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "Voltar ao padrão" }));
    expect(useAppearanceStore.getState().glass).toBe(DEFAULT_GLASS);
    expect(screen.queryByRole("button", { name: "Voltar ao padrão" })).not.toBeInTheDocument();
  });

  it("limita valores fora da faixa", () => {
    useAppearanceStore.getState().setGlass(150);
    expect(useAppearanceStore.getState().glass).toBe(100);
    useAppearanceStore.getState().setGlass(-3);
    expect(useAppearanceStore.getState().glass).toBe(0);
  });
});
