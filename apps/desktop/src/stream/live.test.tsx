import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../App";
import { listenToRoomEvents, type Member, useRoomStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { DEFAULT_QUALITY, listenToStreamEvents, type Source, useStreamStore } from "./stream-store";
import { qualityLabel } from "./StreamMenu";

const { invoke, listen, handlers } = vi.hoisted(() => {
  const handlers = new Map<string, (e: { payload: unknown }) => void>();
  return {
    handlers,
    invoke: vi.fn(),
    listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
      handlers.set(name, cb);
      return () => handlers.delete(name);
    }),
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const ME: Member = { id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: true };
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: false, watching: "aaaaaaaaaaaaaaaa" };
const JOGO = { kind: "window" as const, handle: 42 };
const SOURCES: Source[] = [
  { id: { kind: "monitor", handle: 1 }, name: "Monitor 1", processId: null, processName: null },
  { id: JOGO, name: "Counter-Strike 2", processId: 7, processName: "cs2.exe" },
];

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

let offs: (() => void)[] = [];
beforeEach(async () => {
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string) => (cmd === "list_sources" ? SOURCES : undefined));
  handlers.clear();
  useToastStore.setState({ toasts: [] });
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: {}, pendingWatch: [] });
  useStreamStore.setState({
    status: "live",
    quality: DEFAULT_QUALITY,
    stats: null,
    history: [],
    notice: null,
    fullscreen: false,
    fullscreenOf: null,
    current: { source: JOGO, audioPid: 7, lastAudioPid: 7 },
  });
  offs = [await listenToRoomEvents(), await listenToStreamEvents()];
  emit("room-members", [ME, ANA]);
});
afterEach(() => offs.forEach((off) => off()));

const canal = () => screen.getByRole("region", { name: "Canal de voz" });
const live = () => within(canal()).getByRole("button", { name: /sua transmissão em foco/ });

async function openMenu() {
  fireEvent.contextMenu(live());
  return screen.findByRole("menu", { name: "Opções da transmissão" });
}

describe("a nossa live como um quadro da sala", () => {
  it("aparece primeiro na grade, com a qualidade, o AO VIVO em pílula e o nosso nome", () => {
    render(<App />);
    const grade = within(canal()).getByRole("list", { name: "Quem está na voz" });
    const [primeiro] = within(grade).getAllByRole("listitem");
    expect(within(primeiro).getByRole("button", { name: "Pôr sua transmissão em foco" })).toBeInTheDocument();
    expect(within(primeiro).getByText("1080p 60fps")).toBeInTheDocument();
    const selo = within(primeiro).getByText("Ao vivo");
    expect(selo).toHaveClass("uppercase", "rounded-full");
    expect(within(primeiro).getByText("Kaique")).toBeInTheDocument();
    // A barra e as faixas de quem está ao vivo também usam a pílula.
    expect(within(grade).getAllByRole("listitem")).toHaveLength(3);
  });

  it("clicar põe a live em foco, com as pessoas na faixa de baixo; de novo, volta para a grade", () => {
    render(<App />);
    fireEvent.click(live());
    const foco = within(canal()).getByRole("group", { name: "Em foco" });
    expect(
      within(foco).getByRole("button", { name: /Voltar para a grade \(sua transmissão em foco\)/ }),
    ).toBeInTheDocument();
    expect(within(within(canal()).getByRole("list", { name: "Outros na voz" })).getAllByRole("listitem")).toHaveLength(
      2,
    );
    fireEvent.keyDown(live(), { key: "Enter" });
    expect(within(canal()).getByRole("list", { name: "Quem está na voz" })).toBeInTheDocument();
    // Parou a live com ela em foco: volta para a grade, sem ela.
    fireEvent.click(live());
    act(() => useStreamStore.setState({ status: "idle", current: null }));
    expect(within(canal()).getByRole("list", { name: "Quem está na voz" })).toBeInTheDocument();
    expect(within(canal()).queryByRole("button", { name: /sua transmissão/ })).toBeNull();
  });

  it("o botão direito abre o menu e Parar transmissão para", async () => {
    render(<App />);
    const menu = await openMenu();
    expect(within(menu).queryByText(/Mais opções/)).toBeNull();
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Parar transmissão/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_preview"));
    expect(useStreamStore.getState()).toMatchObject({ status: "idle", current: null });
  });

  it("trocar a transmissão abre o modal e troca sem sair do ar", async () => {
    render(<App />);
    fireEvent.click(within(await openMenu()).getByRole("menuitem", { name: /Trocar transmissão/ }));
    const modal = await screen.findByRole("dialog", { name: "Trocar transmissão" });
    fireEvent.click(within(modal).getByRole("tab", { name: "Telas" }));
    fireEvent.click(await within(modal).findByRole("button", { name: /Monitor 1/ }));
    // O "Trocar" do rodapé (o outro volta para escolher outra fonte).
    fireEvent.click(within(modal).getAllByRole("button", { name: "Trocar" }).at(-1)!);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", {
        source: { kind: "monitor", handle: 1 },
        quality: DEFAULT_QUALITY,
        audioPid: null,
      }),
    );
    // Nunca passou por "começando": a live ficou na tela o tempo todo.
    expect(useStreamStore.getState().status).toBe("live");
    // Sem som agora, mas o programa de antes fica lembrado para religar.
    expect(useStreamStore.getState().current).toEqual({
      source: { kind: "monitor", handle: 1 },
      audioPid: null,
      lastAudioPid: 7,
    });
  });

  it("a qualidade muda pelo submenu, mantendo a fonte e o som", async () => {
    render(<App />);
    const trigger = within(await openMenu()).getByRole("menuitem", { name: /Qualidade da transmissão/ });
    fireEvent.keyDown(trigger, { key: "ArrowRight" });
    const sub = await screen.findByRole("menu", { name: "Qualidade da transmissão" });
    fireEvent.click(within(sub).getByRole("menuitemradio", { name: "720p" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", {
        source: JOGO,
        quality: { resolution: "720", fps: 60 },
        audioPid: 7,
      }),
    );
    fireEvent.click(within(sub).getByRole("menuitemradio", { name: "30 fps" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", {
        source: JOGO,
        quality: { resolution: "720", fps: 30 },
        audioPid: 7,
      }),
    );
    // Com o menu aberto o resto da página sai da árvore de acessibilidade: confere pelo texto.
    expect(screen.getByText("720p 30fps")).toBeInTheDocument();
  });

  it("compartilhar o áudio liga e desliga o som sem parar a live", async () => {
    render(<App />);
    const menu = await openMenu();
    const audio = within(menu).getByRole("menuitemcheckbox", { name: /Compartilhar áudio da transmissão/ });
    expect(audio).toHaveAttribute("aria-checked", "true");
    fireEvent.click(audio);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", { source: JOGO, quality: DEFAULT_QUALITY, audioPid: null }),
    );
    // O menu fica aberto, com a caixa desmarcada.
    await waitFor(() => expect(audio).toHaveAttribute("aria-checked", "false"));
    fireEvent.click(audio);
    await waitFor(() => expect(useStreamStore.getState().current?.audioPid).toBe(7));
  });

  it("sem nenhum programa escolhido, não dá para ligar o som", async () => {
    useStreamStore.setState({
      current: { source: { kind: "monitor", handle: 1 }, audioPid: null, lastAudioPid: null },
    });
    render(<App />);
    const audio = within(await openMenu()).getByRole("menuitemcheckbox", { name: /Compartilhar áudio/ });
    expect(audio).toHaveAttribute("data-disabled");
    expect(audio).toHaveTextContent("Escolha um programa no Trocar transmissão");
    await useStreamStore.getState().setShareAudio(true);
    expect(invoke).not.toHaveBeenCalledWith("start_preview", expect.anything());
  });

  it("se a troca falhar, a live acaba e avisa", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "start_preview") throw "A janela fechou.";
    });
    await useStreamStore.getState().changeQuality({ resolution: "720", fps: 30 });
    expect(useStreamStore.getState()).toMatchObject({ status: "idle", current: null, error: "A janela fechou." });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "A janela fechou." });
  });

  it("sem live não há o que trocar", async () => {
    useStreamStore.setState({ status: "idle", current: null });
    await useStreamStore.getState().changeQuality({ resolution: "720", fps: 30 });
    await useStreamStore.getState().setShareAudio(false);
    expect(invoke).not.toHaveBeenCalledWith("start_preview", expect.anything());
  });

  it("os ... aparecem com o mouse em cima e abrem o menu, com os detalhes", async () => {
    render(<App />);
    expect(screen.queryByRole("button", { name: "Mais opções da transmissão" })).toBeNull();
    // O vídeo fica por baixo da página: o mouse em cima dele é o mouse em cima do quadro.
    fireEvent.mouseEnter(live());
    expect(screen.getByRole("button", { name: "Mais opções da transmissão" })).toBeInTheDocument();
    fireEvent.mouseLeave(live());
    expect(screen.queryByRole("button", { name: "Mais opções da transmissão" })).toBeNull();
    fireEvent.mouseEnter(live());
    fireEvent.pointerDown(screen.getByRole("button", { name: "Mais opções da transmissão" }), {
      button: 0,
      ctrlKey: false,
    });
    // O Radix dá ao menu o nome do botão que o abriu.
    const menu = await screen.findByRole("menu", { name: "Mais opções da transmissão" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Detalhes da transmissão/ }));
    const detalhes = await screen.findByRole("dialog", { name: "Detalhes da transmissão" });
    expect(within(detalhes).getByText("1 assistindo")).toBeInTheDocument();
  });

  it("o foco de um clique não segura os controles depois que o mouse sai; o do teclado segura", () => {
    render(<App />);
    const convidar = within(canal()).getByRole("button", { name: "Convidar pessoas" });
    fireEvent.mouseEnter(canal());
    fireEvent.pointerDown(convidar);
    fireEvent.focus(convidar);
    fireEvent.mouseLeave(canal());
    expect(canal()).not.toHaveAttribute("data-chrome");
    // Pelo Tab (sem clique), os controles ficam enquanto o foco estiver neles.
    fireEvent.focus(convidar);
    expect(canal()).toHaveAttribute("data-chrome");
  });

  it("na barra da chamada, o botão de tela fica verde e abre o menu da live", async () => {
    render(<App />);
    const botao = within(canal()).getByRole("button", { name: "Opções da transmissão" });
    expect(botao).toHaveClass("text-online");
    fireEvent.pointerDown(botao, { button: 0, ctrlKey: false });
    const menu = await screen.findByRole("menu", { name: "Opções da transmissão" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Trocar transmissão/ }));
    expect(await screen.findByRole("dialog", { name: "Trocar transmissão" })).toBeInTheDocument();
  });

  it("os avisos da live aparecem em cima da sala", () => {
    render(<App />);
    emit("stream-notice", { kind: "source-changed", message: "Agora transmitindo “LoL”." });
    expect(within(canal()).getByRole("status")).toHaveTextContent("Agora transmitindo “LoL”.");
    emit("stream-notice", { kind: "no-frames", message: "A imagem parou de chegar." });
    expect(within(canal()).getByRole("status")).toHaveClass("bg-warning");
  });

  it("em tela cheia, só a live", () => {
    useStreamStore.setState({ fullscreen: true });
    render(<App />);
    const sozinha = screen.getByRole("region", { name: "Sua transmissão" });
    expect(within(sozinha).getByRole("button", { name: /sua transmissão em foco/ })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Canal de voz" })).toBeNull();
  });
});

describe("selo da qualidade", () => {
  it("na Fonte mostra a altura de verdade, ou Fonte enquanto ela não chega", () => {
    expect(qualityLabel({ resolution: "source", fps: 60 }, 1440)).toBe("1440p 60fps");
    expect(qualityLabel({ resolution: "source", fps: 30 })).toBe("Fonte 30fps");
  });
});
