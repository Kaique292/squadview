import { act, render, screen } from "@testing-library/react";
import { type Member, useRoomStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { LIVE_MOMENT_MS, StreamTile } from "./StreamTile";
import { useStreamStore } from "./stream-store";
import { watchViewerNotices } from "./viewer-notices";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const ME: Member = { id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: true };
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: false };
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false };

/** `m` assistindo a nossa live. */
const assistindo = (m: Member): Member => ({ ...m, watchingAll: [ME.id] });

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useToastStore.setState({ toasts: [] });
  useStreamStore.setState({ status: "idle", liveSince: null, current: null, stats: null, history: [] });
  useRoomStore.setState({ code: "B8RJ2Q", members: [ME, ANA, BETO], watching: {} });
});

function avisos() {
  return useToastStore.getState().toasts.map((t) => t.message);
}

describe("quem começou a assistir (ADR 0034)", () => {
  let stop: () => void = () => {};
  afterEach(() => stop());

  it("ao vivo, cada pessoa que começa a assistir ganha um aviso com a foto, uma vez", () => {
    stop = watchViewerNotices();
    act(() => useStreamStore.setState({ status: "live" }));
    act(() => useRoomStore.setState({ members: [ME, assistindo(ANA), BETO] }));
    expect(avisos()).toEqual(["Ana começou a assistir"]);
    const aviso = useToastStore.getState().toasts[0];
    expect(aviso.title).toBe("Sua live");
    expect(aviso.avatar?.name).toBe("Ana");
    // A lista chegou de novo igual: nada de aviso repetido.
    act(() => useRoomStore.setState({ members: [ME, assistindo(ANA), BETO] }));
    expect(avisos()).toHaveLength(1);
    act(() => useRoomStore.setState({ members: [ME, assistindo(ANA), assistindo(BETO)] }));
    expect(avisos()).toEqual(["Beto começou a assistir", "Ana começou a assistir"]);
  });

  it("quem para de assistir não gera aviso, e fora do ar nada é avisado", () => {
    stop = watchViewerNotices();
    act(() => useRoomStore.setState({ members: [ME, assistindo(ANA), BETO] }));
    expect(avisos()).toEqual([]);
    act(() => useStreamStore.setState({ status: "live" }));
    // Já estava assistindo quando a live voltou ao ar (ex.: reconectou): avisa.
    expect(avisos()).toEqual(["Ana começou a assistir"]);
    act(() => useRoomStore.setState({ members: [ME, ANA, BETO] }));
    expect(avisos()).toHaveLength(1);
  });

  it("muita gente de uma vez vira um aviso só", () => {
    const outros = ["1", "2", "3", "4"].map((n) => ({ ...BETO, id: `d${n}`.padEnd(16, "0"), name: `P${n}` }));
    stop = watchViewerNotices();
    act(() => useStreamStore.setState({ status: "live" }));
    act(() => useRoomStore.setState({ members: [ME, ...outros.map(assistindo)] }));
    expect(avisos()).toEqual(["4 pessoas começaram a assistir"]);
  });
});

describe("momento de ficar ao vivo (ADR 0034)", () => {
  function tile() {
    return render(
      <StreamTile variant="grid" focused={false} onActivate={() => {}} onChange={() => {}} onDetails={() => {}} />,
    );
  }

  it("começar a live marca quando ela entrou no ar, e trocar de fonte não muda", async () => {
    const source = { kind: "window", id: 1 } as never;
    await act(() => useStreamStore.getState().start(source, useStreamStore.getState().quality));
    const since = useStreamStore.getState().liveSince;
    expect(useStreamStore.getState().status).toBe("live");
    expect(since).not.toBeNull();
    await act(() => useStreamStore.getState().start(source, useStreamStore.getState().quality));
    expect(useStreamStore.getState().liveSince).toBe(since);
  });

  it("logo depois de entrar no ar, o quadro toca a onda uma vez; depois, não repete", () => {
    useStreamStore.setState({ status: "live", liveSince: Date.now() });
    const { container, unmount } = tile();
    expect(container.querySelector("[data-live-wave]")).not.toBeNull();
    expect(screen.getByText("Ao vivo").closest(".animate-entry-pop")).not.toBeNull();
    unmount();
    // O quadro remontou (ex.: foi para o foco) depois do momento: sem onda.
    useStreamStore.setState({ liveSince: Date.now() - LIVE_MOMENT_MS - 1 });
    const again = tile();
    expect(again.container.querySelector("[data-live-wave]")).toBeNull();
  });
});
