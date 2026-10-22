import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../App";
import { useStreamStore } from "../stream/stream-store";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { listenToRoomEvents, type Member, useRoomStore } from "./room-store";
import { STACKED, Viewers, viewersOf } from "./Viewers";

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

const ME: Member = { id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: false };
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: true };
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false };
const CAIO: Member = { id: "dddddddddddddddd", name: "Caio", live: false };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useStreamStore.setState({ status: "idle" });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: {}, pendingWatch: [] });
  await listenToRoomEvents();
});

describe("quem está assistindo", () => {
  it("conta só quem assiste aquela transmissão", () => {
    const members = [{ ...ME, watching: ANA.id }, ANA, { ...BETO, watching: ANA.id }, { ...CAIO, watching: null }];
    expect(viewersOf(members, ANA.id).map((m) => m.name)).toEqual(["Kaique", "Beto"]);
    expect(viewersOf(members, BETO.id)).toEqual([]);
  });

  it("quem assiste várias lives conta em todas; a lista nova vale mais que o campo antigo", () => {
    const members = [
      { ...ME, watching: ANA.id, watchingAll: [ANA.id, CAIO.id] },
      ANA,
      // App até o 0.8.0: só o campo antigo.
      { ...BETO, watching: CAIO.id },
      { ...CAIO, watching: ANA.id, watchingAll: [] },
    ];
    expect(viewersOf(members, ANA.id).map((m) => m.name)).toEqual(["Kaique"]);
    expect(viewersOf(members, CAIO.id).map((m) => m.name)).toEqual(["Kaique", "Beto"]);
  });

  it("mostra até 3 fotos empilhadas e o resto em número; sem ninguém, nada", () => {
    const EDU = { ...CAIO, id: "eeeeeeeeeeeeeeee", name: "Edu" };
    useRoomStore.setState({
      members: [ME, ANA, BETO, CAIO, EDU].map((m) => (m.id === ANA.id ? m : { ...m, watchingAll: [ANA.id] })),
    });
    const view = render(<Viewers streamerId={ANA.id} />);
    const grupo = screen.getByRole("group", { name: "4 pessoas assistindo" });
    expect(grupo.children).toHaveLength(STACKED + 1);
    expect(grupo).toHaveTextContent(/\+1$/);
    // Com exatamente 3, não aparece o "+".
    useRoomStore.setState({
      members: [ME, ANA, BETO, CAIO].map((m) => (m.id === ANA.id ? m : { ...m, watchingAll: [ANA.id] })),
    });
    view.rerender(<Viewers streamerId={ANA.id} />);
    expect(screen.getByRole("group", { name: "3 pessoas assistindo" })).not.toHaveTextContent("+");
    useRoomStore.setState({ members: [ME, ANA] });
    view.rerender(<Viewers streamerId={ANA.id} />);
    expect(screen.queryByRole("group", { name: /assistindo/ })).toBeNull();
  });

  it("com o mouse (ou o teclado) em cima, abre a lista dos espectadores com foto e nome", async () => {
    // A dica do Radix mede o tamanho do conteúdo.
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    useRoomStore.setState({
      members: [ME, ANA, BETO].map((m) => (m.id === ANA.id ? m : { ...m, watchingAll: [ANA.id] })),
    });
    render(<Viewers streamerId={ANA.id} />);
    fireEvent.focus(screen.getByRole("group", { name: "2 pessoas assistindo" }));
    const lista = await screen.findByRole("tooltip");
    expect(lista).toHaveTextContent("Espectadores – 2");
    const nomes = within(lista)
      .getAllByRole("listitem")
      .map((li) => li.textContent);
    expect(nomes).toEqual(["KKaique (você)", "BBeto"]);
    vi.unstubAllGlobals();
  });

  it("na lista da sala, quem está ao vivo não mostra mais o olho com o número", () => {
    render(<App />);
    emit("room-members", [ME, ANA, { ...BETO, watching: ANA.id }, { ...CAIO, watching: ANA.id }]);
    const lista = screen.getByRole("list", { name: "Membros da sala" });
    expect(within(lista).getByText("Ao vivo")).toBeInTheDocument();
    expect(within(lista).queryByRole("group", { name: /assistindo/ })).toBeNull();
  });

  it("com a nossa live em foco, o topo mostra de quem é a tela e quem está assistindo", () => {
    useStreamStore.setState({ status: "live" });
    render(<App />);
    emit("room-members", [{ ...ME, live: true }, { ...ANA, live: false, watching: ME.id }, BETO]);
    expect(screen.queryByText(/Tela de/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Pôr sua transmissão em foco" }));
    const topo = screen.getByText("Tela de Kaique").closest("header")!;
    expect(within(topo).getByRole("group", { name: "1 pessoa assistindo" })).toBeInTheDocument();
    // No canto da barra, a tela cheia da live em foco (a nossa não vai para a janelinha).
    expect(screen.getByRole("button", { name: "Tela cheia" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Picture in picture" })).toBeNull();
  });

  it("com a live de quem assistimos em foco, o canto da barra solta ou põe em tela cheia", async () => {
    useStreamStore.setState({ status: "idle", fullscreen: false, fullscreenOf: null, pip: null });
    render(<App />);
    emit("room-members", [ME, ANA, { ...BETO, watchingAll: [ANA.id] }]);
    emit("watch-started", ANA);
    // A live que começou vem em foco: o topo diz de quem é.
    expect(screen.getByText("Tela de Ana")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Picture in picture" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_set_pip", { memberId: ANA.id, pip: true }));
    fireEvent.click(screen.getByRole("button", { name: "Tela cheia" }));
    await waitFor(() => expect(useStreamStore.getState()).toMatchObject({ fullscreen: true, fullscreenOf: ANA.id }));
  });
});
