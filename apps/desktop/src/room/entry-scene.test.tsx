import { act, fireEvent, render, screen } from "@testing-library/react";
import { App } from "../App";
import { DEFAULT_PREFS, listenToVoiceEvents, useVoiceStore } from "../voice/voice-store";
import { ENTRY_READY_MS, ENTRY_TIMEOUT_MS } from "./EntryScene";
import { listenToRoomEvents, type Member, useRoomStore } from "./room-store";

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
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: false };
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q", roomName: "Galera" } : undefined),
  );
  handlers.clear();
  localStorage.clear();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({
    code: null,
    roomName: null,
    members: [],
    watching: {},
    pendingWatch: [],
    entry: null,
    joinError: null,
  });
  await listenToRoomEvents();
  await listenToVoiceEvents(() => undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

async function entrar() {
  render(<App />);
  await act(() => useRoomStore.getState().join(null, "Kaique"));
}

function cena() {
  return screen.queryByRole("status", { name: "Entrando na sala" });
}

function barra() {
  return Number(screen.getByRole("progressbar", { name: "Entrada na sala" }).getAttribute("aria-valuenow"));
}

describe("cena de entrada na sala (ADR 0034)", () => {
  it("a barra acompanha as conexões de verdade, até conectar com todos", async () => {
    await entrar();
    // Sala encontrada: a cena aparece já com um terço da barra.
    expect(cena()).toHaveTextContent("Sala encontrada");
    expect(barra()).toBe(33);
    emit("room-members", [ME, ANA, BETO]);
    expect(cena()).toHaveTextContent("0 de 2");
    expect(screen.getByRole("list", { name: "Quem está na sala" }).children).toHaveLength(2);
    emit("voice-peers", [ANA.id]);
    expect(cena()).toHaveTextContent("1 de 2");
    expect(barra()).toBe(50);
    emit("voice-peers", [ANA.id, BETO.id]);
    expect(cena()).toHaveTextContent("Conexão direta com as 2 pessoas");
    expect(cena()).toHaveTextContent("Você está na sala.");
    expect(barra()).toBe(100);
  });

  it("pronta, some sozinha", async () => {
    await entrar();
    vi.useFakeTimers();
    emit("room-members", [ME, ANA]);
    emit("voice-peers", [ANA.id]);
    expect(cena()).toHaveTextContent("Conexão direta com 1 pessoa");
    act(() => vi.advanceTimersByTime(ENTRY_READY_MS));
    expect(cena()).toBeNull();
    expect(useRoomStore.getState().entry).toBeNull();
  });

  it("sozinho na sala, fica pronta na hora", async () => {
    await entrar();
    emit("room-members", [ME]);
    expect(cena()).toHaveTextContent("Ninguém mais na sala ainda");
    expect(barra()).toBe(100);
  });

  it("alguém demorando não prende a cena: no tempo limite, fica pronta com quem deu", async () => {
    vi.useFakeTimers();
    await entrar();
    emit("room-members", [ME, ANA, BETO]);
    emit("voice-peers", [ANA.id]);
    act(() => vi.advanceTimersByTime(ENTRY_TIMEOUT_MS));
    expect(cena()).toHaveTextContent("Conectado com 1 de 2 (o resto continua tentando)");
    expect(barra()).toBe(100);
  });

  it("Esc ou um clique fecham na hora", async () => {
    await entrar();
    emit("room-members", [ME, ANA]);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(cena()).toBeNull();
    act(() => useRoomStore.getState().setEntry("connecting"));
    fireEvent.click(document.querySelector("[data-entry-scene]")!);
    expect(cena()).toBeNull();
  });

  it("falhar ao entrar ou sair da sala não deixa a cena para trás", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "room_join" ? Promise.reject("Sala não encontrada.") : Promise.resolve(undefined),
    );
    render(<App />);
    await act(() => useRoomStore.getState().join("ZZZZZZ", "Kaique"));
    expect(useRoomStore.getState().entry).toBeNull();

    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q", roomName: "Galera" } : undefined),
    );
    await act(() => useRoomStore.getState().join(null, "Kaique"));
    expect(useRoomStore.getState().entry?.phase).toBe("connecting");
    await act(() => useRoomStore.getState().leave());
    expect(useRoomStore.getState().entry).toBeNull();
  });
});
