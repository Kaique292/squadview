import { act, render, screen } from "@testing-library/react";
import { App } from "../App";
import { useStreamStore } from "../stream/stream-store";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { listenToRoomEvents, type Member, useRoomStore } from "./room-store";
import { namesList, Viewers, viewersOf } from "./Viewers";

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

  it("a lista completa mostra as fotos de até 3 e o resto em número", () => {
    const EDU = { ...CAIO, id: "eeeeeeeeeeeeeeee", name: "Edu" };
    useRoomStore.setState({
      members: [ME, ANA, BETO, CAIO, EDU].map((m) => (m.id === ANA.id ? m : { ...m, watchingAll: [ANA.id] })),
    });
    const view = render(<Viewers streamerId={ANA.id} />);
    const grupo = screen.getByRole("group", { name: "4 pessoas assistindo" });
    expect(grupo).toHaveTextContent(/^4.*\+1$/);
    useRoomStore.setState({ members: [ME, ANA] });
    view.rerender(<Viewers streamerId={ANA.id} />);
    expect(screen.getByRole("group", { name: "0 pessoas assistindo" })).toBeInTheDocument();
  });

  it("junta os nomes como se fala", () => {
    expect(namesList(["Ana"])).toBe("Ana");
    expect(namesList(["Ana", "Beto"])).toBe("Ana e Beto");
    expect(namesList(["Ana", "Beto", "Caio"])).toBe("Ana, Beto e Caio");
  });

  it("na lista da sala, quem está ao vivo mostra quantos assistem", () => {
    render(<App />);
    emit("room-members", [ME, ANA, { ...BETO, watching: ANA.id }, { ...CAIO, watching: ANA.id }]);
    expect(screen.getByRole("group", { name: "2 pessoas assistindo" })).toBeInTheDocument();
    // Ninguém assistindo: não mostra o zero na lista.
    emit("room-members", [ME, ANA, BETO, CAIO]);
    expect(screen.queryByRole("group", { name: /pessoas? assistindo/ })).toBeNull();
  });

  it("transmitindo, a barra mostra quantos estão assistindo a gente", () => {
    useStreamStore.setState({ status: "live" });
    render(<App />);
    emit("room-members", [{ ...ME, live: true }, { ...ANA, live: false, watching: ME.id }, BETO]);
    expect(screen.getAllByRole("group", { name: "1 pessoa assistindo" }).length).toBeGreaterThan(0);
  });
});
