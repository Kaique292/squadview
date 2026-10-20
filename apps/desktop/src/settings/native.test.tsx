import { act, render, waitFor } from "@testing-library/react";
import { useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { applyGlass } from "./appearance-store";
import { usePresenceSettings, usePresenceSync } from "./presence-store";

const { invoke, win } = vi.hoisted(() => ({
  invoke: vi.fn(),
  win: { setEffects: vi.fn(), clearEffects: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/window", () => ({
  Effect: { Acrylic: "acrylic" },
  getCurrentWindow: () => win,
}));

beforeEach(() => {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  invoke.mockReset().mockResolvedValue(undefined);
  win.setEffects.mockReset().mockResolvedValue(undefined);
  win.clearEffects.mockReset().mockResolvedValue(undefined);
  localStorage.clear();
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("vidro da janela no Windows", () => {
  it("liga o Acrylic só quando o vidro passa de 0% e só quando muda", async () => {
    applyGlass(40);
    expect(win.setEffects).toHaveBeenCalledWith({ effects: ["acrylic"] });
    applyGlass(60);
    expect(win.setEffects).toHaveBeenCalledTimes(1);
    applyGlass(0);
    expect(win.clearEffects).toHaveBeenCalledTimes(1);
  });

  it("se o Windows recusar, tenta de novo na próxima mudança", async () => {
    win.setEffects.mockRejectedValueOnce(new Error("sem DWM"));
    applyGlass(50);
    await waitFor(() => expect(win.setEffects).toHaveBeenCalledTimes(1));
    // Espera a recusa voltar antes de pedir de novo.
    await act(async () => {});
    applyGlass(55);
    expect(win.setEffects).toHaveBeenCalledTimes(2);
  });
});

function PresenceProbe() {
  usePresenceSync();
  return null;
}

describe("status no Discord dentro do app", () => {
  beforeEach(() => {
    useRoomStore.setState({ code: null, members: [], watching: null, joinedAt: null });
    useStreamStore.setState({ status: "idle" });
    usePresenceSettings.setState({ show: true, shareInvite: false });
  });

  it("limpa o status fora de sala e mostra a sala ao entrar", async () => {
    render(<PresenceProbe />);
    expect(invoke).toHaveBeenCalledWith("set_presence", { info: null });
    act(() =>
      useRoomStore.setState({
        code: "F5B6S9",
        joinedAt: 1000,
        members: [{ id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: false }],
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenLastCalledWith("set_presence", {
        info: expect.objectContaining({ members: 1, joinedAtMs: 1000, inviteUrl: null }),
      }),
    );
  });

  it("as chaves das configurações mudam o status e ficam salvas", async () => {
    useRoomStore.setState({ code: "F5B6S9", joinedAt: 1000 });
    render(<PresenceProbe />);
    act(() => usePresenceSettings.getState().setShareInvite(true));
    await waitFor(() =>
      expect(invoke).toHaveBeenLastCalledWith("set_presence", {
        info: expect.objectContaining({ inviteUrl: expect.stringContaining("F5B6S9") }),
      }),
    );
    expect(localStorage.getItem("squadview.presenca.convite")).toBe("1");
    act(() => usePresenceSettings.getState().setShow(false));
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("set_presence", { info: null }));
    expect(localStorage.getItem("squadview.presenca")).toBe("0");
  });

  it("falha do Rust ao mandar o status não aparece para a pessoa", async () => {
    invoke.mockRejectedValue(new Error("Discord fechado"));
    expect(() => render(<PresenceProbe />)).not.toThrow();
    await waitFor(() => expect(invoke).toHaveBeenCalled());
  });
});
