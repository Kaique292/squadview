import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { App } from "../App";
import { listenToRoomEvents, type Member, useRoomStore, type Watch } from "../room/room-store";
import { type StreamStats, useStreamStore } from "../stream/stream-store";
import { watchAppActivity } from "../ui/app-activity";
import { seeThroughAreas } from "../ui/see-through";
import { DEFAULT_PREFS, useVoiceStore } from "./voice-store";

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

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

/** O jsdom não tem `ResizeObserver`: um que só guarda quem observa. */
class FakeResizeObserver {
  constructor(private readonly cb: () => void) {}
  observe() {}
  disconnect() {}
  fire() {
    this.cb();
  }
}

const ROOM = { left: 0, top: 0, width: 1200, height: 700 };
const TILE = { left: 100, top: 100, width: 640, height: 360 };

let focused = true;
let stopActivity: () => void = () => {};

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  focused = true;
  vi.spyOn(document, "hasFocus").mockImplementation(() => focused);
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  // No jsdom tudo mede 0 × 0 (e área vazia não vira transparente): a sala com 1200 × 700, e o
  // resto (o quadro da live) com 640 × 360 dentro dela.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    // A sala e os fundos dela (que pintam a área inteira) medem a sala.
    const sala = this.getAttribute("aria-label") === "Canal de voz" || this.hasAttribute("data-backdrop");
    return (sala ? ROOM : TILE) as DOMRect;
  });
  // A luz só liga dentro do app (com o Tauri por trás).
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  stopActivity = watchAppActivity();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useStreamStore.setState({ pip: null, fullscreen: false, fullscreenOf: null });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: {}, pendingWatch: [] });
  await listenToRoomEvents();
});

afterEach(() => {
  stopActivity();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

/** Assistindo a Ana; `fps` > 0: a imagem já está chegando. */
function assistindoAna(fps: number) {
  const watch: Watch = {
    member: ANA,
    stats: { fps } as StreamStats,
    history: [],
    listening: true,
    volume: 100,
  };
  useRoomStore.setState({ watching: { [ANA.id]: watch } });
}

function canal() {
  return screen.getByRole("region", { name: "Canal de voz" });
}

function ambientCalls() {
  return invoke.mock.calls.filter((c) => c[0] === "set_watch_ambient").map((c) => c[1]);
}

function focarAna() {
  render(<App />);
  emit("room-members", [ME, ANA]);
  fireEvent.click(within(canal()).getByRole("button", { name: "Pôr a live de Ana em foco" }));
}

describe("luz ambiente da sala (ADR 0034)", () => {
  it("a live em foco, já com imagem, vira a luz: a sala fica transparente e ganha a vinheta", () => {
    assistindoAna(60);
    focarAna();
    expect(canal()).toHaveAttribute("data-ambient");
    expect(canal().querySelector(".ambient-vignette")).not.toBeNull();
    expect(ambientCalls()).toEqual([{ memberId: ANA.id, on: true }]);
    // A área da sala vai para o Rust (pixels físicos) e vira uma área transparente da página.
    expect(invoke).toHaveBeenCalledWith("set_ambient_area", expect.objectContaining({ x: 0, y: 0 }));
    expect(seeThroughAreas().length).toBeGreaterThan(0);
  });

  it("a vinheta não abre buraco para a própria luz, só para as lives", () => {
    assistindoAna(60);
    focarAna();
    const vinheta = canal().querySelector<HTMLElement>(".ambient-vignette")!;
    const fundo = canal().querySelector<HTMLElement>("[data-backdrop]:not(.ambient-vignette)")!;
    // O fundo preto abre a sala inteira (a luz aparece); a vinheta abre só o quadro da live.
    const salaInteira = "H 1199 V 699";
    const quadro = "H 739 V 459";
    expect(fundo.style.clipPath).toContain(salaInteira);
    expect(vinheta.style.clipPath).not.toContain(salaInteira);
    expect(vinheta.style.clipPath).toContain(quadro);
  });

  it("sem imagem ainda, nada de luz (senão apareceria o que está atrás da janela)", () => {
    assistindoAna(0);
    focarAna();
    expect(canal()).not.toHaveAttribute("data-ambient");
    expect(ambientCalls()).toEqual([]);
  });

  it("perder o foco para o jogo apaga a luz, e voltar acende de novo", () => {
    assistindoAna(60);
    focarAna();
    focused = false;
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(canal()).not.toHaveAttribute("data-ambient");
    expect(ambientCalls()).toEqual([
      { memberId: ANA.id, on: true },
      { memberId: ANA.id, on: false },
    ]);
    expect(invoke).toHaveBeenCalledWith("set_ambient_area", { x: 0, y: 0, width: 0, height: 0 });
    // A sala volta ao preto (só o quadro da live continua transparente).
    expect(seeThroughAreas().some((a) => a.width === ROOM.width)).toBe(false);
    focused = true;
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(canal()).toHaveAttribute("data-ambient");
  });

  it("no picture in picture, a live não ilumina a sala", () => {
    assistindoAna(60);
    focarAna();
    act(() => useStreamStore.setState({ pip: ANA.id }));
    expect(canal()).not.toHaveAttribute("data-ambient");
    expect(ambientCalls().at(-1)).toEqual({ memberId: ANA.id, on: false });
  });

  it("tirar a live do foco apaga a luz", () => {
    assistindoAna(60);
    focarAna();
    fireEvent.click(within(canal()).getByRole("button", { name: "Voltar para a grade (live de Ana em foco)" }));
    expect(canal()).not.toHaveAttribute("data-ambient");
    expect(ambientCalls().at(-1)).toEqual({ memberId: ANA.id, on: false });
  });
});
