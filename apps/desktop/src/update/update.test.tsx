import { act, fireEvent, render, screen } from "@testing-library/react";
import { UpdateButton } from "./UpdateButton";
import { parseNotes, useUpdateStore } from "./update-store";

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

  it("mostra o botão verde e abre o modal com as novidades", async () => {
    await withUpdate();
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    expect(screen.getByRole("dialog", { name: "Atualizar o SquadView" })).toBeInTheDocument();
    expect(screen.getByText("Desfaz o vídeo em poeira ao parar")).toBeInTheDocument();
    expect(screen.getByText("Abre a janela no centro da tela")).toBeInTheDocument();
  });

  it("'Agora não' fecha o modal e o botão continua lá", async () => {
    await withUpdate();
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    fireEvent.click(screen.getByRole("button", { name: "Agora não" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" })).toBeInTheDocument();
  });

  it("baixa com progresso e reinicia o app", async () => {
    const seen: number[] = [];
    await withUpdate(async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 200 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      const s = useUpdateStore.getState().status;
      if (s.kind === "downloading" && s.percent !== null) seen.push(s.percent);
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished" });
    });
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Atualizar e reiniciar" })));
    expect(seen).toEqual([50]);
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it("enquanto baixa, mostra o progresso e não deixa fechar", async () => {
    await withUpdate(() => new Promise(() => {}));
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    act(() => void useUpdateStore.getState().install());
    expect(screen.getByRole("progressbar", { name: "Baixando a atualização" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agora não" })).toBeDisabled();
  });

  it("se o download falhar, explica e deixa tentar de novo", async () => {
    await withUpdate(async () => {
      throw new Error("rede caiu");
    });
    fireEvent.click(screen.getByRole("button", { name: "Atualização disponível: 0.2.0" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Atualizar e reiniciar" })));
    expect(screen.getByRole("alert")).toHaveTextContent("Não deu para baixar a atualização");
    expect(screen.getByRole("button", { name: "Tentar de novo" })).toBeInTheDocument();
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
