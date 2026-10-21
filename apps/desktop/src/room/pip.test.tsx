import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { listenToStreamEvents, useStreamStore } from "../stream/stream-store";
import { useRoomStore } from "./room-store";
import { WatchView } from "./WatchView";

const { invoke, handlers } = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return () => handlers.delete(name);
  }),
}));

const LOBO = { id: "bbbbbbbbbbbbbbbb", name: "Lobo", live: true };

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  useRoomStore.setState({ watching: LOBO, watchStats: null });
  useStreamStore.setState({ fullscreen: false, pip: false });
});

describe("picture in picture", () => {
  it("solta o vídeo numa janelinha e traz de volta pelo botão do lugar dele", async () => {
    render(<WatchView member={LOBO} />);
    fireEvent.click(screen.getByRole("button", { name: "Picture in picture" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_set_pip", { pip: true }));
    // Onde o vídeo ficava, o app explica e oferece trazer de volta.
    expect(screen.getByRole("status")).toHaveTextContent("Assistindo Lobo em picture in picture");
    expect(screen.getByRole("button", { name: "Trazer o vídeo de volta" })).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "Trazer de volta" }));
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("room_set_pip", { pip: false }));
    expect(screen.getByRole("status")).not.toHaveTextContent("picture in picture");
  });

  it("em tela cheia, soltar o vídeo sai da tela cheia antes", async () => {
    useStreamStore.setState({ fullscreen: true });
    await act(() => useStreamStore.getState().setPip(true));
    expect(invoke).toHaveBeenNthCalledWith(1, "set_fullscreen", { fullscreen: false });
    expect(invoke).toHaveBeenNthCalledWith(2, "room_set_pip", { pip: true });
    expect(useStreamStore.getState()).toMatchObject({ fullscreen: false, pip: true });
  });

  it("dois cliques, Esc ou fechar na janelinha trazem o vídeo de volta", async () => {
    useStreamStore.setState({ pip: true });
    const unlisten = await listenToStreamEvents();
    act(() => handlers.get("video-pip")!({ payload: false }));
    await waitFor(() => expect(useStreamStore.getState().pip).toBe(false));
    expect(invoke).toHaveBeenCalledWith("room_set_pip", { pip: false });
    unlisten();
    expect(handlers.has("video-pip")).toBe(false);
  });

  it("desfaz se o Rust recusar", async () => {
    invoke.mockRejectedValue("Assista a alguém primeiro.");
    await useStreamStore.getState().setPip(true);
    expect(useStreamStore.getState().pip).toBe(false);
  });

  it("parar de assistir fecha a janelinha junto, e o próximo começa dentro do app", async () => {
    const view = render(<WatchView member={LOBO} />);
    fireEvent.click(screen.getByRole("button", { name: "Picture in picture" }));
    await waitFor(() => expect(useStreamStore.getState().pip).toBe(true));
    fireEvent.click(screen.getByRole("button", { name: /Parar de assistir/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_stop_watching"));
    view.unmount();
    expect(useStreamStore.getState().pip).toBe(false);
  });
});
