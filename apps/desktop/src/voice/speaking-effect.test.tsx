import { render, screen, within } from "@testing-library/react";
import { act } from "react";
import { App } from "../App";
import { listenToRoomEvents, type Member, useRoomStore } from "../room/room-store";
import { DEFAULT_QUALITY, useStreamStore } from "../stream/stream-store";
import { cleanEffect, useSpeakingEffectStore } from "./speaking-effect";
import { DEFAULT_PREFS, listenToVoiceEvents, useVoiceStore } from "./voice-store";

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
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: false, effect: { kind: "rays", color: "#a78bfa" } };
// Um efeito de uma versão mais nova do app: aqui vira o anel padrão.
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false, effect: { kind: "fogo", color: "#ff0000" } };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useSpeakingEffectStore.setState({ effect: null });
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: null, pendingWatch: null });
  useStreamStore.setState({ status: "idle", stats: null, fullscreen: false, quality: DEFAULT_QUALITY });
  await listenToRoomEvents();
  await listenToVoiceEvents((id) => useRoomStore.getState().members.find((m) => m.id === id)?.name);
});

describe("efeito de fala", () => {
  it("só aceita os efeitos conhecidos com cor #rrggbb", () => {
    expect(cleanEffect({ kind: "waves", color: "#FF8800" })).toEqual({ kind: "waves", color: "#ff8800" });
    expect(cleanEffect({ kind: "fogo", color: "#ff8800" })).toBeNull();
    expect(cleanEffect({ kind: "dots", color: "red" })).toBeNull();
    expect(cleanEffect({ kind: "dots", color: "#ff8800;background:url(x)" })).toBeNull();
    expect(cleanEffect(null)).toBeNull();
  });

  it("aplicar salva neste PC e avisa a sala; remover volta ao anel padrão", () => {
    act(() => useSpeakingEffectStore.getState().setEffect({ kind: "dots", color: "#3ef0b0" }));
    expect(JSON.parse(localStorage.getItem("squadview.efeito-de-fala")!)).toEqual({ kind: "dots", color: "#3ef0b0" });
    expect(invoke).toHaveBeenCalledWith("room_set_effect", { effect: { kind: "dots", color: "#3ef0b0" } });
    act(() => useSpeakingEffectStore.getState().setEffect(null));
    expect(localStorage.getItem("squadview.efeito-de-fala")).toBeNull();
    expect(invoke).toHaveBeenLastCalledWith("room_set_effect", { effect: null });
  });

  it("entra na sala já com o efeito escolhido", async () => {
    useRoomStore.setState({ code: null });
    useSpeakingEffectStore.setState({ effect: { kind: "equalizer", color: "#7c3aed" } });
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q" } : undefined));
    await useRoomStore.getState().join(null, "Kaique");
    expect(invoke).toHaveBeenCalledWith(
      "room_join",
      expect.objectContaining({ effect: { kind: "equalizer", color: "#7c3aed" } }),
    );
  });

  it("quem fala aparece com o efeito que escolheu, na lista e no card; sem efeito, o anel verde", () => {
    useSpeakingEffectStore.setState({ effect: { kind: "waves", color: "#ff5a4f" } });
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    const grade = screen.getByRole("list", { name: "Quem está na voz" });
    // Ninguém falando: o efeito existe, mas fica parado e invisível.
    expect(grade.querySelector("[data-effect=rays]")).not.toHaveAttribute("data-active");

    emit("voice-speaking", { memberId: ANA.id, speaking: true });
    emit("voice-speaking", { memberId: BETO.id, speaking: true });
    emit("voice-speaking", { memberId: null, speaking: true });
    expect(grade.querySelector("[data-effect=rays]")).toHaveAttribute("data-active");
    // O nosso vem do que escolhemos aqui.
    expect(grade.querySelector("[data-effect=waves]")).toHaveAttribute("data-active");
    // O card de quem fala com efeito ganha a borda na cor dele.
    const anaCard = within(grade).getByRole("button", { name: "Pôr Ana em foco" });
    expect(anaCard.style.borderColor).toBe("rgb(167, 139, 250)");
    // O efeito desconhecido do Beto vira o anel verde padrão.
    const betoCard = within(grade).getByRole("button", { name: "Pôr Beto em foco" });
    expect(betoCard.querySelector("[data-effect]")).toBeNull();
    expect(betoCard.querySelector(".ring-online")).not.toBeNull();

    // Na lista de membros da lateral também.
    const lista = screen
      .getAllByText("Ana")
      .map((el) => el.closest("li"))
      .find(Boolean)!;
    expect(lista.querySelector("[data-effect=rays]")).toHaveAttribute("data-active");
  });

  it("na lista o efeito aparece inteiro e se mistura com o de quem está do lado", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const membros = screen.getByRole("list", { name: "Membros da sala" });
    // Recortar sem folga cortava o efeito do primeiro e do último da lista.
    expect(membros.className).not.toMatch(/overflow-hidden/);
    expect(membros.className).toMatch(/overflow-clip-margin/);
    // Com `isolate` na foto, o efeito não enxergava o do vizinho e as cores não se misturavam.
    const aura = membros.querySelector("[data-effect=rays]")!;
    expect(aura.parentElement!.className).not.toMatch(/\bisolate\b/);
  });
});
