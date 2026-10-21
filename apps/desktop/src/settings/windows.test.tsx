import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { WindowsSection } from "./WindowsSection";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("configurações do Windows", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("mostra o que está salvo e guarda ao desligar a bandeja", async () => {
    invoke.mockImplementation((cmd: string, args?: { enabled: boolean }) =>
      Promise.resolve(cmd === "window_settings" ? { closeToTray: true } : { closeToTray: args?.enabled }),
    );
    render(<WindowsSection />);
    const chave = screen.getByRole("switch", { name: "Minimizar para a bandeja ao fechar" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("window_settings"));
    expect(chave).toHaveAttribute("aria-checked", "true");
    await act(async () => fireEvent.click(chave));
    expect(invoke).toHaveBeenCalledWith("set_close_to_tray", { enabled: false });
    expect(chave).toHaveAttribute("aria-checked", "false");
  });

  it("fora do app (sem Rust), fica no padrão ligado sem quebrar", async () => {
    invoke.mockImplementation(() => Promise.reject(new Error("sem tauri")));
    render(<WindowsSection />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("window_settings"));
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });
});
