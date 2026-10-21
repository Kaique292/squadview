import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useToastStore } from "../ui/toast-store";
import { SettingsDialog } from "./SettingsDialog";
import { formatBytes } from "./StorageSection";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const MB = 1024 * 1024;
let usage = [
  { id: "web", bytes: 17 * MB },
  { id: "logs", bytes: 3.6 * MB },
];

beforeEach(() => {
  usage = [
    { id: "web", bytes: 17 * MB },
    { id: "logs", bytes: 3.6 * MB },
  ];
  useToastStore.setState({ toasts: [] });
  invoke.mockReset();
  invoke.mockImplementation((cmd: string, args?: { part: string }) => {
    if (cmd === "storage_usage") return Promise.resolve(usage);
    if (cmd === "storage_clear") {
      usage = usage.map((p) => (args!.part === "all" || p.id === args!.part ? { ...p, bytes: 0 } : p));
      return Promise.resolve(usage);
    }
    return Promise.resolve(undefined);
  });
});

async function openStorage() {
  render(<SettingsDialog trigger={<button type="button">Configurações</button>} />);
  fireEvent.click(screen.getByRole("button", { name: "Configurações" }));
  const nav = screen.getByRole("navigation", { name: "Categorias" });
  fireEvent.click(within(nav).getByRole("button", { name: "Armazenamento" }));
  return screen.findByRole("list", { name: "Partes do app" });
}

describe("armazenamento", () => {
  it("escreve os tamanhos para gente ler", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1,5 KB");
    expect(formatBytes(17 * MB)).toBe("17 MB");
    expect(formatBytes(3.6 * MB)).toBe("3,6 MB");
    expect(formatBytes(1.2 * 1024 * MB)).toBe("1,2 GB");
  });

  it("mostra quanto cada parte ocupa e limpa uma só", async () => {
    const lista = await openStorage();
    expect(within(lista).getByText("17 MB")).toBeInTheDocument();
    expect(within(lista).getByText("3,6 MB")).toBeInTheDocument();
    expect(screen.getByText("21 MB")).toBeInTheDocument();

    fireEvent.click(within(lista).getByRole("button", { name: "Limpar registros de desempenho" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("storage_clear", { part: "logs" }));
    await waitFor(() => expect(within(lista).getByText("0 B")).toBeInTheDocument());
    expect(within(lista).getByRole("button", { name: "Limpar registros de desempenho" })).toBeDisabled();
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "success", message: "3,6 MB liberados." });
  });

  it("limpar tudo pede confirmação antes", async () => {
    await openStorage();
    fireEvent.click(screen.getByRole("button", { name: "Limpar tudo" }));
    const confirmar = await screen.findByRole("dialog", { name: "Limpar todo o cache?" });
    expect(invoke).not.toHaveBeenCalledWith("storage_clear", expect.anything());
    fireEvent.click(within(confirmar).getByRole("button", { name: "Limpar tudo" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("storage_clear", { part: "all" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Limpar tudo" })).toBeDisabled());
  });

  it("avisa quando não dá para limpar", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "storage_usage" ? Promise.resolve(usage) : Promise.reject("O WebView2 não conseguiu limpar o cache."),
    );
    const lista = await openStorage();
    fireEvent.click(within(lista).getByRole("button", { name: "Limpar imagens e arquivos baixados" }));
    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]).toMatchObject({
        kind: "error",
        message: "O WebView2 não conseguiu limpar o cache.",
      }),
    );
    expect(within(lista).getByText("17 MB")).toBeInTheDocument();
  });
});
