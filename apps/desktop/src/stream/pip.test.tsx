import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { listenToRoomEvents, type Member, useRoomStore, type Watch } from "../room/room-store";
import { parsePipHash, PIP_CONTROL_EVENT, PIP_STATE_EVENT } from "./pip";
import { PipView } from "./PipView";
import { useStreamStore } from "./stream-store";

const { invoke, emitTo, startDragging, handlers } = vi.hoisted(() => ({
  invoke: vi.fn(),
  emitTo: vi.fn(),
  startDragging: vi.fn(),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  emitTo,
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return () => handlers.delete(name);
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ startDragging }) }));

const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: true };
const PAGE = { memberId: ANA.id, name: "Ana", listening: true, volume: 100 };

function watchOf(extra: Partial<Watch> = {}): Watch {
  return { member: ANA, stats: null, history: [], listening: true, volume: 100, ...extra };
}

beforeEach(() => {
  handlers.clear();
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  emitTo.mockReset();
  emitTo.mockResolvedValue(undefined);
  startDragging.mockReset();
  startDragging.mockResolvedValue(undefined);
});

describe("endereço da janelinha", () => {
  it("lê quem transmite e o som, do jeito que o Rust monta", () => {
    expect(parsePipHash("#pip=bbbbbbbbbbbbbbbb&nome=Ana%20Paula%20%26%20cia&som=0&volume=250")).toEqual({
      memberId: ANA.id,
      name: "Ana Paula & cia",
      listening: false,
      volume: 200,
    });
    expect(parsePipHash("#pip=x&volume=abc")).toEqual({ memberId: "x", name: "Live", listening: true, volume: 100 });
  });

  it("não é a janelinha sem o `#pip=` ou sem quem transmite", () => {
    expect(parsePipHash("")).toBeNull();
    expect(parsePipHash("#demo=sala")).toBeNull();
    expect(parsePipHash("#pip=")).toBeNull();
  });
});

describe("controles da janelinha", () => {
  it("mostra quem transmite, o AO VIVO e os controles", () => {
    render(<PipView page={PAGE} />);
    expect(screen.getByRole("region", { name: "Live de Ana em picture in picture" })).toHaveTextContent("Ana");
    expect(screen.getByText("Ao vivo")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Voltar para o app" })).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: "Volume da transmissão" })).toHaveValue("100");
  });

  it("silenciar e o volume vão para a janela principal, que mexe no som", () => {
    render(<PipView page={PAGE} />);
    fireEvent.click(screen.getByRole("button", { name: "Silenciar a transmissão" }));
    expect(emitTo).toHaveBeenLastCalledWith("main", PIP_CONTROL_EVENT, { memberId: ANA.id, listening: false });
    // Mexer no volume com o som desligado liga o som, como no Discord.
    fireEvent.change(screen.getByRole("slider", { name: "Volume da transmissão" }), { target: { value: "60" } });
    expect(emitTo).toHaveBeenLastCalledWith("main", PIP_CONTROL_EVENT, {
      memberId: ANA.id,
      volume: 60,
      listening: true,
    });
    expect(screen.getByRole("button", { name: "Silenciar a transmissão" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("slider", { name: "Volume da transmissão" }), { target: { value: "80" } });
    expect(emitTo).toHaveBeenLastCalledWith("main", PIP_CONTROL_EVENT, { memberId: ANA.id, volume: 80 });
    expect(screen.getByText("80%")).toBeInTheDocument();
  });

  it("com o som desligado, zerar o volume não liga o som", () => {
    render(<PipView page={{ ...PAGE, listening: false }} />);
    fireEvent.change(screen.getByRole("slider", { name: "Volume da transmissão" }), { target: { value: "0" } });
    expect(emitTo).toHaveBeenLastCalledWith("main", PIP_CONTROL_EVENT, { memberId: ANA.id, volume: 0 });
    expect(screen.getByRole("button", { name: "Ligar o som da transmissão" })).toBeInTheDocument();
  });

  it("acompanha o som mudado pela janela principal, só o da live dela", async () => {
    const view = render(<PipView page={PAGE} />);
    await waitFor(() => expect(handlers.has(PIP_STATE_EVENT)).toBe(true));
    act(() => handlers.get(PIP_STATE_EVENT)!({ payload: { memberId: "outra", listening: false, volume: 10 } }));
    expect(screen.getByRole("slider", { name: "Volume da transmissão" })).toHaveValue("100");
    act(() => handlers.get(PIP_STATE_EVENT)!({ payload: { memberId: ANA.id, listening: false, volume: 40 } }));
    expect(screen.getByRole("slider", { name: "Volume da transmissão" })).toHaveValue("40");
    expect(screen.getByRole("button", { name: "Ligar o som da transmissão" })).toBeInTheDocument();
    view.unmount();
    await waitFor(() => expect(handlers.has(PIP_STATE_EVENT)).toBe(false));
  });

  it("voltar para o app pelo botão ou com dois cliques; um clique arrasta a janelinha", () => {
    render(<PipView page={PAGE} />);
    fireEvent.click(screen.getByRole("button", { name: "Voltar para o app" }));
    expect(invoke).toHaveBeenLastCalledWith("room_set_pip", { memberId: ANA.id, pip: false });

    invoke.mockClear();
    const area = screen.getByRole("region");
    fireEvent.mouseDown(area, { button: 0, detail: 1 });
    expect(startDragging).toHaveBeenCalledTimes(1);
    fireEvent.mouseDown(area, { button: 0, detail: 2 });
    expect(invoke).toHaveBeenCalledWith("room_set_pip", { memberId: ANA.id, pip: false });
    // O botão direito e os controles não arrastam.
    fireEvent.mouseDown(area, { button: 2, detail: 1 });
    fireEvent.mouseDown(screen.getByRole("slider"), { button: 0, detail: 1 });
    expect(startDragging).toHaveBeenCalledTimes(1);
  });
});

describe("janela principal e janelinha", () => {
  let offs: () => void;
  beforeEach(async () => {
    useRoomStore.setState({ members: [ANA], watching: { [ANA.id]: watchOf() } });
    useStreamStore.setState({ pip: ANA.id });
    offs = await listenToRoomEvents();
  });
  afterEach(() => {
    offs();
    useStreamStore.setState({ pip: null });
  });

  it("os controles da janelinha mudam o som da live, e ela fica sabendo como ficou", async () => {
    act(() => handlers.get(PIP_CONTROL_EVENT)!({ payload: { memberId: ANA.id, volume: 50, listening: false } }));
    expect(useRoomStore.getState().watching[ANA.id]).toMatchObject({ volume: 50, listening: false });
    expect(invoke).toHaveBeenCalledWith("set_watch_volume", { memberId: ANA.id, volume: 0.5 });
    expect(invoke).toHaveBeenCalledWith("set_watch_listening", { memberId: ANA.id, listening: false });
    await waitFor(() =>
      expect(emitTo).toHaveBeenLastCalledWith("pip", PIP_STATE_EVENT, {
        memberId: ANA.id,
        listening: false,
        volume: 50,
      }),
    );
  });

  it("sem a live no picture in picture, a janelinha não é avisada", async () => {
    useStreamStore.setState({ pip: null });
    await useRoomStore.getState().setWatchVolume(ANA.id, 30);
    await Promise.resolve();
    expect(emitTo).not.toHaveBeenCalled();
  });
});
