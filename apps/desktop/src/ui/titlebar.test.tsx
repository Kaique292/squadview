import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TitleBar } from "./TitleBar";

const { win, getCurrentWindow } = vi.hoisted(() => {
  const win = {
    isMaximized: vi.fn(),
    onResized: vi.fn(),
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
  };
  return { win, getCurrentWindow: vi.fn(() => win) };
});
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow }));

/** Redimensionamento avisado pela janela (o Tauri chama de volta). */
let resized: (() => void) | null = null;
const unlisten = vi.fn();

function enterTauri() {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
}

beforeEach(() => {
  resized = null;
  unlisten.mockReset();
  getCurrentWindow.mockReset();
  getCurrentWindow.mockImplementation(() => win);
  win.isMaximized.mockReset().mockResolvedValue(false);
  win.onResized.mockReset().mockImplementation(async (cb: () => void) => {
    resized = cb;
    return unlisten;
  });
  for (const fn of [win.minimize, win.toggleMaximize, win.close]) fn.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("barra de título", () => {
  it("mostra o nome da sala no meio, ou SquadView fora de sala", () => {
    const { rerender } = render(<TitleBar />);
    expect(screen.getByText("SquadView")).toBeInTheDocument();
    rerender(<TitleBar title="Galera" />);
    expect(screen.getByText("Galera")).toBeInTheDocument();
  });

  it("fora do app (navegador, testes), os botões da janela não fazem nada", () => {
    render(<TitleBar />);
    fireEvent.click(screen.getByRole("button", { name: "Minimizar" }));
    fireEvent.click(screen.getByRole("button", { name: "Maximizar" }));
    fireEvent.click(screen.getByRole("button", { name: "Fechar" }));
    expect(getCurrentWindow).not.toHaveBeenCalled();
  });

  it("minimiza, maximiza e fecha a janela de verdade", async () => {
    enterTauri();
    render(<TitleBar />);
    fireEvent.click(screen.getByRole("button", { name: "Minimizar" }));
    fireEvent.click(screen.getByRole("button", { name: "Maximizar" }));
    fireEvent.click(screen.getByRole("button", { name: "Fechar" }));
    expect(win.minimize).toHaveBeenCalled();
    expect(win.toggleMaximize).toHaveBeenCalled();
    expect(win.close).toHaveBeenCalled();
  });

  it("maximizada, o botão vira restaurar e acompanha o redimensionamento", async () => {
    enterTauri();
    win.isMaximized.mockResolvedValue(true);
    const { unmount } = render(<TitleBar />);
    expect(await screen.findByRole("button", { name: "Restaurar" })).toBeInTheDocument();
    win.isMaximized.mockResolvedValue(false);
    await act(async () => resized!());
    expect(await screen.findByRole("button", { name: "Maximizar" })).toBeInTheDocument();
    // Ao sair, para de ouvir a janela.
    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("falha da janela não derruba a barra", async () => {
    enterTauri();
    win.isMaximized.mockRejectedValue(new Error("sem janela"));
    win.onResized.mockRejectedValue(new Error("sem janela"));
    win.close.mockRejectedValue(new Error("sem janela"));
    const { unmount } = render(<TitleBar />);
    fireEvent.click(screen.getByRole("button", { name: "Fechar" }));
    await waitFor(() => expect(win.close).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Maximizar" })).toBeInTheDocument();
    unmount();
  });

  it("sem janela nativa (modo de demonstração), a barra funciona sem o estado maximizado", () => {
    enterTauri();
    getCurrentWindow.mockImplementation(() => {
      throw new Error("sem janela");
    });
    render(<TitleBar />);
    expect(screen.getByRole("button", { name: "Maximizar" })).toBeInTheDocument();
    expect(() => fireEvent.click(screen.getByRole("button", { name: "Minimizar" }))).not.toThrow();
  });
});
