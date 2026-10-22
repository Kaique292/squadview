import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { type Member, useRoomStore, type Watch } from "../room/room-store";
import { useDissolveStore } from "../ui/dissolve-effect";
import { watchVideoHoles } from "../ui/overlay";
import { seeThroughAreas } from "../ui/see-through";
import { listenToStreamEvents, onVideoEnter, type StreamStats, useStreamStore } from "./stream-store";
import {
  cancelVideoClick,
  DOUBLE_CLICK_MS,
  handleVideoPointer,
  OWN_VIDEO,
  setVideoRect,
  toClient,
} from "./video-pointer";
import { NO_IMAGE_AFTER_MS, WatchTile } from "./WatchTile";

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

const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: true };
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: true };
const stats = (fps: number) => ({ fps, height: 720, latencyMs: 40, bitrateKbps: 6000, rttMs: 20 }) as StreamStats;

function watchOf(member: Member, extra: Partial<Watch> = {}): Watch {
  return { member, stats: null, history: [], listening: true, volume: 100, ...extra };
}

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function tile(props: Partial<Parameters<typeof WatchTile>[0]> = {}) {
  return (
    <WatchTile memberId={ANA.id} variant="grid" focused={false} onActivate={() => {}} onDetails={() => {}} {...props} />
  );
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  useRoomStore.setState({
    members: [ANA, BETO],
    watching: { [ANA.id]: watchOf(ANA), [BETO.id]: watchOf(BETO) },
    pendingWatch: [],
  });
  useStreamStore.setState({ fullscreen: false, fullscreenOf: null, pip: null, videoHover: null });
});

describe("picture in picture", () => {
  it("solta a live pelo menu e traz de volta pelo botão do lugar dela", async () => {
    render(tile());
    fireEvent.contextMenu(screen.getByRole("button", { name: /live de Ana/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Picture in picture/ }));
    // A janelinha abre com quem transmite e o som da live (os controles dela começam iguais aos do menu).
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("room_set_pip", {
        memberId: ANA.id,
        pip: true,
        info: { name: "Ana", listening: true, volume: 100, width: 0, height: 0 },
      }),
    );
    // Onde o vídeo ficava, o app explica e oferece trazer de volta.
    expect(screen.getByRole("status")).toHaveTextContent("Assistindo Ana em picture in picture");
    fireEvent.contextMenu(screen.getByRole("button", { name: /live de Ana/ }));
    expect(await screen.findByRole("menuitem", { name: /Trazer o vídeo de volta/ })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    fireEvent.click(screen.getByRole("button", { name: "Trazer de volta" }));
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("room_set_pip", { memberId: ANA.id, pip: false }));
    expect(screen.getByRole("status")).not.toHaveTextContent("picture in picture");
  });

  it("dois cliques no lugar da live solta trazem ela de volta (não põem em tela cheia)", async () => {
    useStreamStore.setState({ pip: ANA.id });
    render(tile());
    fireEvent.doubleClick(screen.getByRole("button", { name: /live de Ana/ }));
    await waitFor(() => expect(useStreamStore.getState().pip).toBeNull());
    expect(invoke).not.toHaveBeenCalledWith("set_fullscreen", expect.anything());
  });

  it("em tela cheia, soltar a live sai da tela cheia antes", async () => {
    useStreamStore.setState({ fullscreen: true, fullscreenOf: ANA.id });
    await act(() => useStreamStore.getState().setPip(ANA.id));
    expect(invoke).toHaveBeenNthCalledWith(1, "set_fullscreen", { fullscreen: false });
    expect(invoke).toHaveBeenNthCalledWith(2, "room_set_pip", {
      memberId: ANA.id,
      pip: true,
      info: expect.any(Object),
    });
    expect(useStreamStore.getState()).toMatchObject({ fullscreen: false, fullscreenOf: null, pip: ANA.id });
  });

  it("uma por vez: soltar outra só pede a nova (o Rust traz a anterior de volta)", async () => {
    useStreamStore.setState({ pip: ANA.id });
    await useStreamStore.getState().setPip(BETO.id);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("room_set_pip", { memberId: BETO.id, pip: true, info: expect.any(Object) });
    // Pedir a mesma de novo não faz nada; sem nenhuma solta, trazer de volta também não.
    await useStreamStore.getState().setPip(BETO.id);
    useStreamStore.setState({ pip: null });
    await useStreamStore.getState().setPip(null);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("dois cliques, Esc ou fechar na janelinha trazem a live de volta", async () => {
    useStreamStore.setState({ pip: ANA.id });
    const unlisten = await listenToStreamEvents();
    // Aviso de outra live (que não está solta) não mexe em nada.
    emit("video-pip", { pip: false, source: BETO.id });
    expect(useStreamStore.getState().pip).toBe(ANA.id);
    emit("video-pip", { pip: false, source: ANA.id });
    await waitFor(() => expect(useStreamStore.getState().pip).toBeNull());
    expect(invoke).toHaveBeenCalledWith("room_set_pip", { memberId: ANA.id, pip: false });
    unlisten();
    expect(handlers.has("video-pip")).toBe(false);
  });

  it("desfaz se o Rust recusar", async () => {
    invoke.mockRejectedValue("Assista a alguém primeiro.");
    await useStreamStore.getState().setPip(ANA.id);
    expect(useStreamStore.getState().pip).toBeNull();
  });

  it("parar de assistir a live solta (ou em tela cheia) volta a interface ao normal", async () => {
    useStreamStore.setState({ pip: ANA.id });
    await useRoomStore.getState().stopWatching(ANA.id);
    expect(useStreamStore.getState().pip).toBeNull();
    expect(useRoomStore.getState().watching[ANA.id]).toBeUndefined();
    useStreamStore.setState({ fullscreen: true, fullscreenOf: BETO.id });
    await useRoomStore.getState().stopWatching(BETO.id);
    expect(useStreamStore.getState()).toMatchObject({ fullscreen: false, fullscreenOf: null });
    expect(invoke).toHaveBeenCalledWith("room_stop_watching", { memberId: BETO.id });
  });
});

describe("tela cheia de uma live", () => {
  it("já em tela cheia, trocar de vídeo não mexe na janela; desfaz se o Rust recusar", async () => {
    await useStreamStore.getState().setFullscreen(true, ANA.id);
    expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: true });
    invoke.mockClear();
    await useStreamStore.getState().setFullscreen(true, BETO.id);
    expect(invoke).not.toHaveBeenCalled();
    expect(useStreamStore.getState().fullscreenOf).toBe(BETO.id);

    invoke.mockRejectedValue(new Error("falhou"));
    await useStreamStore.getState().setFullscreen(false);
    expect(useStreamStore.getState()).toMatchObject({ fullscreen: true, fullscreenOf: BETO.id });
  });

  it("os dois cliques dentro do vídeo de uma live alternam a tela cheia dela", async () => {
    const unlisten = await listenToStreamEvents();
    emit("video-fullscreen", { kind: "toggle", source: ANA.id });
    await waitFor(() => expect(useStreamStore.getState()).toMatchObject({ fullscreen: true, fullscreenOf: ANA.id }));
    // Outra live pede tela cheia: passa a ser ela, sem sair da tela cheia.
    emit("video-fullscreen", { kind: "toggle", source: BETO.id });
    await waitFor(() => expect(useStreamStore.getState().fullscreenOf).toBe(BETO.id));
    emit("video-fullscreen", { kind: "toggle", source: BETO.id });
    await waitFor(() => expect(useStreamStore.getState().fullscreen).toBe(false));
    unlisten();
  });
});

describe("o quadro da live", () => {
  it("não aparece sem a live (ela acabou e o quadro ainda não saiu)", () => {
    const { container } = render(tile({ memberId: "dddddddddddddddd" }));
    expect(container).toBeEmptyDOMElement();
  });

  it("na faixa, só o essencial: sem a qualidade e sem os ...", () => {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA, { stats: stats(60) }) } });
    useStreamStore.setState({ videoHover: ANA.id });
    render(tile({ variant: "strip" }));
    expect(screen.queryByText("720p 60fps")).toBeNull();
    expect(screen.queryByRole("button", { name: /Mais opções/ })).toBeNull();
    expect(screen.getByText("Ao vivo")).toBeInTheDocument();
  });

  it("na faixa, solta ou sem imagem, mostra só o ícone", () => {
    vi.useFakeTimers();
    useStreamStore.setState({ pip: ANA.id });
    const view = render(tile({ variant: "strip" }));
    expect(screen.getByRole("status")).toHaveTextContent("");
    act(() => useStreamStore.setState({ pip: null }));
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    expect(screen.getByRole("status")).not.toHaveTextContent("não está mandando imagem");
    view.unmount();
    vi.useRealTimers();
  });

  it("os ... aparecem com o mouse sobre o vídeo dela (não sobre o de outra live)", async () => {
    const unlisten = await listenToStreamEvents();
    const seen = vi.fn();
    const off = onVideoEnter(seen);
    render(tile());
    emit("video-pointer", { kind: "enter", source: BETO.id });
    expect(screen.queryByRole("button", { name: "Mais opções da live de Ana" })).toBeNull();
    // Passou direto de um vídeo para o outro: conta como entrar de novo.
    emit("video-pointer", { kind: "enter", source: ANA.id });
    expect(seen).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Mais opções da live de Ana" })).toBeInTheDocument();
    // A saída de outro vídeo (atrasada) não apaga o "em cima" deste.
    emit("video-pointer", { kind: "leave", source: BETO.id });
    expect(useStreamStore.getState().videoHover).toBe(ANA.id);
    emit("video-pointer", { kind: "leave", source: ANA.id });
    expect(screen.queryByRole("button", { name: "Mais opções da live de Ana" })).toBeNull();
    off();
    unlisten();
  });

  it("os ... abrem o menu da live; o clique num item não põe o quadro em foco", async () => {
    const activate = vi.fn();
    const details = vi.fn();
    render(tile({ onActivate: activate, onDetails: details }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: /live de Ana/ }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "Mais opções da live de Ana" }), {
      button: 0,
      ctrlKey: false,
    });
    const menu = await screen.findByRole("menu", { name: "Mais opções da live de Ana" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Detalhes da transmissão/ }));
    expect(details).toHaveBeenCalled();
    expect(activate).not.toHaveBeenCalled();
    fireEvent.mouseLeave(screen.getByRole("button", { name: "Pôr a live de Ana em foco" }));
  });

  it("clicar nos ... não põe em foco nem em tela cheia", () => {
    const activate = vi.fn();
    render(tile({ onActivate: activate }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Pôr a live de Ana em foco" }));
    const mais = screen.getByRole("button", { name: "Mais opções da live de Ana" });
    fireEvent.click(mais);
    fireEvent.doubleClick(mais);
    expect(activate).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalledWith("set_fullscreen", expect.anything());
  });

  it("a live acaba com o menu aberto: o menu sai junto com o quadro", async () => {
    render(tile());
    fireEvent.contextMenu(screen.getByRole("button", { name: /live de Ana/ }));
    await screen.findByRole("menu", { name: "Opções da live de Ana" });
    act(() => useRoomStore.setState({ watching: {} }));
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("Enter e espaço põem em foco; outras teclas não", () => {
    const activate = vi.fn();
    render(tile({ onActivate: activate, focused: true }));
    const quadro = screen.getByRole("button", { name: "Voltar para a grade (live de Ana em foco)" });
    fireEvent.keyDown(quadro, { key: "a" });
    fireEvent.keyDown(quadro, { key: " " });
    fireEvent.keyDown(quadro, { key: "Enter" });
    expect(activate).toHaveBeenCalledTimes(2);
  });
});

describe("avisos da imagem", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("conectou e ainda não chegou imagem: diz que está esperando", () => {
    render(tile());
    expect(screen.getByRole("status")).toHaveTextContent("Esperando a imagem de Ana…");
  });

  it("sem imagem por alguns segundos, explica em vez de ficar parado", () => {
    render(tile());
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    expect(screen.getByRole("status")).toHaveTextContent("Ana não está mandando imagem agora");
    expect(screen.getByRole("status")).toHaveTextContent("minimizado");
  });

  it("quando a imagem volta, o aviso some; se parar de novo, a espera recomeça", () => {
    const set = (fps: number) =>
      act(() => useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA, { stats: stats(fps) }) } }));
    render(tile());
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    set(60);
    // A imagem chegando aparece no lugar do aviso (o vídeo fica por baixo da página).
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(screen.getByText("720p 60fps")).toHaveAttribute("data-video-hole");
    set(0);
    expect(screen.getByRole("status")).toHaveTextContent("Esperando a imagem de Ana…");
    act(() => vi.advanceTimersByTime(NO_IMAGE_AFTER_MS));
    expect(screen.getByRole("status")).toHaveTextContent("Ana não está mandando imagem agora");
  });
});

describe("cada vídeo no seu lugar", () => {
  let observed: (() => void) | null = null;
  beforeEach(() => {
    observed = null;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(cb: () => void) {
          observed = cb;
        }
        observe() {}
        disconnect() {}
      },
    );
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    vi.stubGlobal("devicePixelRatio", 2);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      left: 100,
      top: 40,
      width: 320,
      height: 180,
    } as DOMRect);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("a live manda o lugar dela; na faixa recolhida, some; ao sair, esconde", () => {
    const view = render(tile());
    expect(invoke).toHaveBeenCalledWith("set_watch_bounds", {
      memberId: ANA.id,
      x: 200,
      y: 80,
      width: 640,
      height: 360,
      radius: 0,
    });
    expect(observed).not.toBeNull();
    view.rerender(tile({ hidden: true }));
    expect(invoke).toHaveBeenLastCalledWith("set_watch_bounds", {
      memberId: ANA.id,
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      radius: 0,
    });
    invoke.mockClear();
    view.unmount();
    expect(invoke).toHaveBeenCalledWith("set_watch_bounds", {
      memberId: ANA.id,
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      radius: 0,
    });
    expect(invoke).not.toHaveBeenCalledWith("set_video_bounds", expect.anything());
  });

  it("com imagem chegando, o quadro fica transparente para o vídeo por baixo, com os cantos dele", () => {
    const view = render(tile());
    const quadro = screen.getByRole("button", { name: /live de Ana/ });
    // O `rounded-lg` do quadro (o CSS do Tailwind não carrega nos testes).
    quadro.style.borderTopLeftRadius = "8px";
    act(() => observed!());
    // Sem imagem: o quadro preto, o aviso e nenhum buraco no fundo.
    expect(quadro).toHaveClass("bg-black");
    expect(screen.getByRole("status")).toHaveTextContent("Esperando a imagem de Ana");
    expect(seeThroughAreas()).toEqual([]);
    expect(invoke).toHaveBeenCalledWith("set_watch_bounds", expect.objectContaining({ radius: 16 }));

    invoke.mockClear();
    act(() =>
      useRoomStore.setState((s) => ({ watching: { ...s.watching, [ANA.id]: watchOf(ANA, { stats: stats(60) }) } })),
    );
    expect(quadro).not.toHaveClass("bg-black");
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(seeThroughAreas()).toEqual([{ x: 100, y: 40, width: 320, height: 180, radius: 8 }]);
    // Só a transparência muda: o vídeo continua onde estava.
    expect(invoke).not.toHaveBeenCalledWith("set_watch_bounds", expect.anything());

    // No picture in picture o vídeo sai daqui: o quadro volta a ser preto, com o aviso.
    act(() => useStreamStore.setState({ pip: ANA.id }));
    expect(quadro).toHaveClass("bg-black");
    expect(seeThroughAreas()).toEqual([]);

    act(() => useStreamStore.setState({ pip: null }));
    expect(seeThroughAreas()).toHaveLength(1);
    view.unmount();
    expect(seeThroughAreas()).toEqual([]);
  });

  it("a foto da saída de uma live aparece onde ela estava", async () => {
    const unlisten = await listenToStreamEvents();
    const view = render(tile());
    view.unmount();
    emit("video-exit", { image: "data:image/jpeg;base64,AAAA", source: ANA.id });
    expect(useDissolveStore.getState().effect?.rect).toEqual({ x: 100, y: 40, width: 320, height: 180 });
    act(() => useDissolveStore.setState({ effect: null }));
    // De um vídeo que nunca esteve na tela, não há onde tocar.
    emit("video-exit", { image: "data:image/jpeg;base64,AAAA", source: "eeeeeeeeeeeeeeee" });
    expect(useDissolveStore.getState().effect).toBeNull();
    unlisten();
  });

  it("um vigia só para os recortes de todos os vídeos: ele para quando sai o último", () => {
    const first = watchVideoHoles();
    const second = watchVideoHoles();
    invoke.mockClear();
    const body = document.createElement("div");
    body.setAttribute("data-video-hole", "");
    first();
    first();
    expect(invoke).not.toHaveBeenCalledWith("set_video_holes", { holes: [] });
    second();
    // Sem nenhum recorte mandado, parar não precisa limpar nada.
    expect(invoke).not.toHaveBeenCalledWith("set_video_holes", expect.anything());
  });
});

describe("o mouse de cada vídeo", () => {
  afterEach(() => {
    setVideoRect(null, ANA.id);
    setVideoRect(null);
  });

  it("converte o ponto de cada vídeo pelo lugar dele", () => {
    setVideoRect({ left: 10, top: 20 });
    setVideoRect({ left: 500, top: 300 }, ANA.id);
    expect(toClient(4, 6)).toEqual({ clientX: 14, clientY: 26 });
    expect(toClient(4, 6, ANA.id)).toEqual({ clientX: 504, clientY: 306 });
    expect(toClient(4, 6, BETO.id)).toBeNull();
    expect(OWN_VIDEO).not.toBe(ANA.id);
  });

  it("o clique de uma live espera só pelos dois cliques dela", () => {
    vi.useFakeTimers();
    setVideoRect({ left: 0, top: 0 }, ANA.id);
    const alvo = document.createElement("div");
    const click = vi.fn();
    alvo.addEventListener("click", click);
    Object.assign(document, { elementFromPoint: () => alvo });
    handleVideoPointer({ kind: "click", x: 5, y: 5, source: ANA.id }, () => {});
    // A tela cheia de outro vídeo não cancela o clique desta live.
    cancelVideoClick(BETO.id);
    vi.advanceTimersByTime(DOUBLE_CLICK_MS);
    expect(click).toHaveBeenCalledTimes(1);
    handleVideoPointer({ kind: "click", x: 5, y: 5, source: ANA.id }, () => {});
    cancelVideoClick(ANA.id);
    vi.advanceTimersByTime(DOUBLE_CLICK_MS);
    expect(click).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
});
