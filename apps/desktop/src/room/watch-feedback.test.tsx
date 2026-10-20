import { act, render, screen } from "@testing-library/react";
import type { StreamStats } from "../stream/stream-store";
import { useRoomStore } from "./room-store";
import { NO_IMAGE_AFTER_MS, WatchView } from "./WatchView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const LOBO = { id: "bbbbbbbbbbbbbbbb", name: "Lobo", live: true };
const stats = (fps: number) => ({ fps, latencyMs: 40, bitrateKbps: 6000, rttMs: 20 }) as StreamStats;

beforeEach(() => {
  vi.useFakeTimers();
  useRoomStore.setState({ watching: LOBO, watchStats: null });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("assistindo: avisos da imagem", () => {
  it("conectou e ainda não chegou imagem: diz que está esperando", () => {
    render(<WatchView member={LOBO} />);
    expect(screen.getByRole("status")).toHaveTextContent("Conectado. Esperando a imagem de Lobo…");
  });

  it("sem imagem por alguns segundos, explica em vez de ficar parado", () => {
    render(<WatchView member={LOBO} />);
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    expect(screen.getByRole("status")).toHaveTextContent("Lobo não está mandando imagem agora");
    expect(screen.getByRole("status")).toHaveTextContent("minimizado");
  });

  it("quando a imagem volta, o aviso some; se parar de novo, a espera recomeça", () => {
    render(<WatchView member={LOBO} />);
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    act(() => useRoomStore.setState({ watchStats: stats(60) }));
    expect(screen.getByRole("status")).toHaveTextContent("Esperando a imagem de Lobo…");
    act(() => useRoomStore.setState({ watchStats: stats(0) }));
    expect(screen.getByRole("status")).not.toHaveTextContent("não está mandando imagem");
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    expect(screen.getByRole("status")).toHaveTextContent("Lobo não está mandando imagem agora");
  });
});
