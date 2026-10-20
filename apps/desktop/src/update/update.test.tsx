import { act, fireEvent, render, screen } from "@testing-library/react";
import { UpdateButton } from "./UpdateButton";
import { REJOIN_WITHIN_MS, rememberRoomToRejoin, takeRoomToRejoin, useRoomStore } from "../room/room-store";
import { parseNotes, takeWhatsNew, useUpdateStore } from "./update-store";

const { check, relaunch } = vi.hoisted(() => ({ check: vi.fn(), relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));

type ProgressEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

const NOTES = `## [0.2.0](https://github.com/Kaique292/squadview/compare/v0.1.0...v0.2.0) (2026-10-20)

### ✨ Novidades

* **ui:** desfaz o vídeo em poeira ao parar ([35b42be](https://github.com/Kaique292/squadview/commit/35b42be))
* **app:** volume de quem você assiste até 200% ([abc1234](https://github.com/Kaique292/squadview/commit/abc1234))

### 🐛 Correções

* **app:** abre a janela no centro da tela ([41295d7](https://github.com/Kaique292/squadview/commit/41295d7))
`;

function fakeUpdate(download: (onEvent: (e: ProgressEvent) => void) => Promise<void>) {
  return { version: "0.2.0", body: NOTES, downloadAndInstall: vi.fn(download) };
}

beforeEach(() => {
  check.mockReset();
  relaunch.mockReset();
  useUpdateStore.setState({ status: { kind: "idle" }, notes: null });
  useRoomStore.setState({ code: null });
  localStorage.clear();
});

async function withUpdate(download: (onEvent: (e: ProgressEvent) => void) => Promise<void> = async () => {}) {
  check.mockResolvedValue(fakeUpdate(download));
  render(<UpdateButton />);
  await act(() => useUpdateStore.getState().checkForUpdate());
}

describe("atualização automática", () => {
  it("sem versão nova, não mostra o botão", async () => {
    check.mockResolvedValue(null);
    render(<UpdateButton />);
    await act(() => useUpdateStore.getState().checkForUpdate());
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("fica quieto se a checagem falhar (sem internet)", async () => {
    check.mockRejectedValue(new Error("offline"));
    await useUpdateStore.getState().checkForUpdate();
    expect(useUpdateStore.getState().status).toEqual({ kind: "idle" });
  });

  it("fora de uma sala, a setinha verde já atualiza e reinicia, como no Discord", async () => {
    const seen: number[] = [];
    await withUpdate(async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 200 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      const s = useUpdateStore.getState().status;
      if (s.kind === "downloading" && s.percent !== null) seen.push(s.percent);
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished" });
    });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" })));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(seen).toEqual([50]);
    expect(relaunch).toHaveBeenCalledOnce();
    // As novidades ficam guardadas para depois de abrir de novo; não há sala para voltar.
    expect(takeWhatsNew()).toEqual({ version: "0.2.0", notes: NOTES });
    expect(takeRoomToRejoin()).toBeNull();
  });

  it("numa sala, avisa que sai por alguns segundos e só atualiza se confirmar", async () => {
    useRoomStore.setState({ code: "B8RJ2Q" });
    await withUpdate();
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    expect(screen.getByRole("dialog", { name: "Sair temporariamente da sala?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(relaunch).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Atualizar mesmo assim!" })));
    expect(relaunch).toHaveBeenCalledOnce();
    // Ao abrir de novo, volta para a mesma sala (uma vez só).
    expect(takeRoomToRejoin()).toBe("B8RJ2Q");
    expect(takeRoomToRejoin()).toBeNull();
  });

  it("a volta para a sala vale só logo depois da atualização", () => {
    useRoomStore.setState({ code: "B8RJ2Q" });
    rememberRoomToRejoin(1_000);
    expect(takeRoomToRejoin(1_000 + REJOIN_WITHIN_MS + 1)).toBeNull();
  });

  it("enquanto baixa, a setinha gira e mostra o progresso", async () => {
    await withUpdate(() => new Promise(() => {}));
    act(() => void useUpdateStore.getState().install());
    expect(screen.getByRole("button", { name: "Baixando a atualização…" })).toBeInTheDocument();
    act(() => useUpdateStore.setState({ status: { kind: "downloading", version: "0.2.0", percent: 40 } }));
    // Clicar enquanto baixa não faz nada.
    fireEvent.click(screen.getByRole("button", { name: "Baixando a atualização (40%)" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("se o download falhar, a setinha fica vermelha e tenta de novo ao clicar", async () => {
    useRoomStore.setState({ code: "B8RJ2Q" });
    await withUpdate(async () => {
      throw new Error("rede caiu");
    });
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Atualizar mesmo assim!" })));
    const retry = screen.getByRole("button", { name: "Não deu para atualizar. Clique para tentar de novo." });
    expect(retry).toHaveClass("text-live");
    // Não atualizou: nada de voltar para a sala nem de novidades.
    expect(takeRoomToRejoin()).toBeNull();
    expect(takeWhatsNew()).toBeNull();
  });

  it("depois de atualizar, mostra as novidades uma vez", () => {
    localStorage.setItem("squadview.novidades", JSON.stringify({ version: "0.2.0", notes: NOTES }));
    render(<UpdateButton />);
    const dialog = screen.getByRole("dialog", { name: "Novidades da versão 0.2.0" });
    expect(dialog).toHaveTextContent("Desfaz o vídeo em poeira ao parar");
    fireEvent.click(screen.getByRole("button", { name: "Entendi" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(takeWhatsNew()).toBeNull();
  });

  it("novidades sem notas viram 'correções e melhorias'", () => {
    localStorage.setItem("squadview.novidades", JSON.stringify({ version: "0.2.1", notes: null }));
    render(<UpdateButton />);
    expect(screen.getByText("Correções e melhorias.")).toBeInTheDocument();
  });

  it("transforma as notas da release em listas simples", () => {
    expect(parseNotes(NOTES)).toEqual([
      {
        title: "✨ Novidades",
        items: ["Desfaz o vídeo em poeira ao parar", "Volume de quem você assiste até 200%"],
      },
      { title: "🐛 Correções", items: ["Abre a janela no centro da tela"] },
    ]);
    expect(parseNotes(null)).toEqual([]);
  });
});
