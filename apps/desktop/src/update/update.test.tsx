import { fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import { UpdateBanner } from "./UpdateBanner";
import { useUpdateStore } from "./update-store";

const { check, relaunch } = vi.hoisted(() => ({ check: vi.fn(), relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));

type ProgressEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

function fakeUpdate(download: (onEvent: (e: ProgressEvent) => void) => Promise<void>) {
  return { version: "0.2.0", downloadAndInstall: vi.fn(download) };
}

beforeEach(() => {
  check.mockReset();
  relaunch.mockReset();
  useUpdateStore.setState({ status: { kind: "idle" } });
});

describe("atualização automática", () => {
  it("não mostra nada quando não há versão nova", async () => {
    check.mockResolvedValue(null);
    render(<UpdateBanner />);
    await act(() => useUpdateStore.getState().checkForUpdate());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("fica quieto se a checagem falhar (sem internet)", async () => {
    check.mockRejectedValue(new Error("offline"));
    await useUpdateStore.getState().checkForUpdate();
    expect(useUpdateStore.getState().status).toEqual({ kind: "idle" });
  });

  it("avisa a versão nova e some ao clicar em 'Agora não'", async () => {
    check.mockResolvedValue(fakeUpdate(async () => {}));
    render(<UpdateBanner />);
    await act(() => useUpdateStore.getState().checkForUpdate());

    expect(screen.getByText("Nova versão disponível: 0.2.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Agora não" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("baixa com progresso e reinicia o app", async () => {
    const seen: number[] = [];
    check.mockResolvedValue(
      fakeUpdate(async (onEvent) => {
        onEvent({ event: "Started", data: { contentLength: 200 } });
        onEvent({ event: "Progress", data: { chunkLength: 100 } });
        const s = useUpdateStore.getState().status;
        if (s.kind === "downloading" && s.percent !== null) seen.push(s.percent);
        onEvent({ event: "Progress", data: { chunkLength: 100 } });
        onEvent({ event: "Finished" });
      }),
    );
    await useUpdateStore.getState().checkForUpdate();
    await useUpdateStore.getState().install();

    expect(seen).toEqual([50]);
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it("mostra progresso indefinido quando o tamanho é desconhecido", async () => {
    check.mockResolvedValue(fakeUpdate(() => new Promise(() => {})));
    render(<UpdateBanner />);
    await act(() => useUpdateStore.getState().checkForUpdate());
    act(() => void useUpdateStore.getState().install());
    expect(screen.getByText("Baixando atualização…")).toBeInTheDocument();
  });

  it("explica o erro se o download falhar e permite fechar o aviso", async () => {
    check.mockResolvedValue(
      fakeUpdate(async () => {
        throw new Error("rede caiu");
      }),
    );
    render(<UpdateBanner />);
    await act(() => useUpdateStore.getState().checkForUpdate());
    await act(() => useUpdateStore.getState().install());

    expect(screen.getByText(/Não deu para atualizar agora/)).toBeInTheDocument();
    expect(relaunch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Fechar aviso" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
