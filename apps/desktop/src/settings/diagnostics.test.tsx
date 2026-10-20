import { act, fireEvent, render, screen } from "@testing-library/react";
import { useToastStore } from "../ui/toast-store";
import { DiagnosticsSection } from "./DiagnosticsSection";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

beforeEach(() => {
  invoke.mockReset();
  useToastStore.setState({ toasts: [] });
});

async function renderWith(status: { enabled: boolean; files: number }) {
  invoke.mockResolvedValueOnce(status);
  render(<DiagnosticsSection />);
  await act(async () => {});
}

describe("registro de desempenho", () => {
  it("mostra se está ligado e desliga", async () => {
    await renderWith({ enabled: true, files: 2 });
    const toggle = screen.getByRole("switch", { name: "Gravar registro de desempenho" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    invoke.mockResolvedValueOnce({ enabled: false, files: 2 });
    await act(async () => fireEvent.click(toggle));
    expect(invoke).toHaveBeenCalledWith("set_diagnostics_enabled", { enabled: false });
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("copia os registros e explica onde colar", async () => {
    await renderWith({ enabled: true, files: 3 });
    invoke.mockResolvedValueOnce(3);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copiar registros" })));
    expect(invoke).toHaveBeenCalledWith("copy_diagnostics");
    expect(useToastStore.getState().toasts[0].message).toBe(
      "3 registros copiados. Cole (Ctrl+V) na conversa do Discord.",
    );
  });

  it("sem registros, os botões ficam desligados", async () => {
    await renderWith({ enabled: false, files: 0 });
    expect(screen.getByRole("button", { name: "Copiar registros" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Mostrar na pasta" })).toBeDisabled();
  });

  it("abre a pasta dos registros", async () => {
    await renderWith({ enabled: true, files: 1 });
    invoke.mockResolvedValueOnce(undefined);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Mostrar na pasta" })));
    expect(invoke).toHaveBeenCalledWith("reveal_diagnostics");
  });

  it("explica quando não dá para ligar, copiar ou abrir a pasta", async () => {
    await renderWith({ enabled: false, files: 2 });
    invoke.mockRejectedValueOnce("Pasta sem permissão.");
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: "Gravar registro de desempenho" })));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "Pasta sem permissão." });
    invoke.mockRejectedValueOnce("Nada para copiar.");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copiar registros" })));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "warning", message: "Nada para copiar." });
    invoke.mockRejectedValueOnce("Explorer fechado.");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Mostrar na pasta" })));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "warning", message: "Explorer fechado." });
  });

  it("um registro só fica no singular", async () => {
    await renderWith({ enabled: true, files: 1 });
    invoke.mockResolvedValueOnce(1);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copiar registros" })));
    expect(useToastStore.getState().toasts[0].message).toBe(
      "1 registro copiado. Cole (Ctrl+V) na conversa do Discord.",
    );
  });

  it("sem o Rust responder, a chave fica desligada", async () => {
    invoke.mockRejectedValueOnce(new Error("fora do app"));
    render(<DiagnosticsSection />);
    await act(async () => {});
    expect(screen.getByRole("switch", { name: "Gravar registro de desempenho" })).toBeDisabled();
  });
});
