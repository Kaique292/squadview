import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { App } from "../App";
import { StreamDetailsDialog, sparklinePoints } from "./StreamDetailsDialog";
import { StreamDialog } from "./StreamDialog";
import { StreamTile } from "./StreamTile";
import {
  DEFAULT_QUALITY,
  estimatedMbps,
  listenToStreamEvents,
  type Source,
  type StreamStats,
  useStreamStore,
} from "./stream-store";

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

const SOURCES: Source[] = [
  { id: { kind: "monitor", handle: 1 }, name: "Monitor 1 (principal)", processId: null, processName: null },
  { id: { kind: "window", handle: 42 }, name: "Counter-Strike 2", processId: 7, processName: "cs2.exe" },
];

const STATS: StreamStats = {
  rttMs: 0,
  audioActive: true,
  audioError: null,
  audioKbps: 128,
  audioLevel: 0.5,
  audioLost: 0,
  sourceFps: 74,
  fps: 60,
  bitrateKbps: 8000,
  encodeMs: 2.1,
  latencyMs: 12,
  latencyP95Ms: 20,
  dropped: 0,
  width: 1920,
  height: 1080,
  encoder: "NVIDIA H.264 Encoder MFT",
  lowLayerHeight: 0,
  lowLayerError: null,
  keyframeRequests: 0,
};

beforeEach(() => {
  invoke.mockReset();
  handlers.clear();
  useStreamStore.setState({
    status: "idle",
    sources: [],
    loadingSources: false,
    thumbnails: {},
    quality: DEFAULT_QUALITY,
    stats: null,
    history: [],
    system: null,
    current: null,
    fullscreenOf: null,
    notice: null,
    listening: false,
    fullscreen: false,
    logPath: null,
    error: null,
  });
});

function mockBackend(start: () => Promise<unknown> = async () => undefined) {
  invoke.mockImplementation((cmd: string, args?: { source?: { handle: number } }) => {
    if (cmd === "list_sources") return Promise.resolve(SOURCES);
    if (cmd === "source_thumbnail") {
      // A janela do jogo tem miniatura; o monitor não manda imagem.
      return Promise.resolve(args?.source?.handle === 42 ? "data:image/jpeg;base64,AAAA" : null);
    }
    if (cmd === "start_preview") return start();
    return Promise.resolve();
  });
}

async function openDialog() {
  render(<StreamDialog />);
  fireEvent.click(screen.getByRole("button", { name: "Transmitir minha tela" }));
  return screen.findByRole("dialog");
}

describe("transmitir a tela: escolher a fonte", () => {
  it("separa aplicativos e telas em abas, começando por aplicativos", async () => {
    mockBackend();
    await openDialog();
    expect(await screen.findByRole("button", { name: "Counter-Strike 2" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Monitor 1/ })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Aplicativos" })).toHaveAttribute("aria-selected", "true");

    fireEvent.click(screen.getByRole("tab", { name: "Telas" }));
    // O conteúdo troca com animação: a aba anterior sai antes da nova entrar.
    expect(await screen.findByRole("button", { name: /Monitor 1/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Counter-Strike 2" })).not.toBeInTheDocument();
  });

  it("mostra a miniatura quando chega e um ícone quando a fonte não manda imagem", async () => {
    mockBackend();
    await openDialog();
    const card = await screen.findByRole("button", { name: "Counter-Strike 2" });
    await waitFor(() => expect(card.querySelector("img")).toHaveAttribute("src", "data:image/jpeg;base64,AAAA"));

    fireEvent.click(screen.getByRole("tab", { name: "Telas" }));
    await waitFor(() => expect(useStreamStore.getState().thumbnails["monitor-1"]).toBeNull());
    const monitor = await screen.findByRole("button", { name: /Monitor 1/ });
    expect(monitor.querySelector("img")).toBeNull();
    expect(monitor.querySelector("[data-testid=miniatura-carregando]")).toBeNull();
  });

  it("mostra carregando enquanto a miniatura não chega", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "list_sources" ? Promise.resolve(SOURCES) : new Promise(() => {}),
    );
    await openDialog();
    const card = await screen.findByRole("button", { name: "Counter-Strike 2" });
    expect(card.querySelector("[data-testid=miniatura-carregando]")).not.toBeNull();
  });

  it("guarda a miniatura como vazia quando o pedido falha", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "list_sources" ? Promise.resolve(SOURCES) : Promise.reject(new Error("falhou")),
    );
    await useStreamStore.getState().loadSources();
    expect(useStreamStore.getState().thumbnails).toEqual({ "monitor-1": null, "window-42": null });
  });

  it("avisa quando não há aplicativos abertos", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "list_sources" ? [SOURCES[0]] : null));
    await openDialog();
    expect(await screen.findByText("Nenhum aplicativo aberto para transmitir.")).toBeInTheDocument();
  });

  it("não deixa ficar ao vivo sem escolher uma fonte", async () => {
    mockBackend();
    await openDialog();
    expect(screen.getByRole("button", { name: "Ficar ao vivo" })).toBeDisabled();
  });

  it("avisa quando não consegue listar as fontes", async () => {
    invoke.mockRejectedValue(new Error("falhou"));
    await useStreamStore.getState().loadSources();
    expect(useStreamStore.getState().error).toMatch(/Não deu para listar/);
  });
});

describe("transmitir a tela: qualidade", () => {
  async function pickGame() {
    mockBackend();
    await openDialog();
    fireEvent.click(await screen.findByRole("button", { name: "Counter-Strike 2" }));
  }

  it("mostra o que vai transmitir, com 1080p e 60 fps marcados e tudo liberado", async () => {
    await pickGame();
    expect(screen.getByText("O que você está transmitindo")).toBeInTheDocument();
    expect(screen.getByText("Counter-Strike 2")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "1080p" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "60" })).toHaveAttribute("aria-checked", "true");
    for (const name of ["720p", "1440p", "Fonte", "15", "30"]) {
      expect(screen.getByRole("radio", { name })).toBeEnabled();
    }
    expect(screen.queryByText(/nitro/i)).not.toBeInTheDocument();
  });

  it("volta para a escolha da fonte com Trocar ou Voltar", async () => {
    await pickGame();
    fireEvent.click(screen.getByRole("button", { name: "Trocar" }));
    expect(screen.getByRole("tab", { name: "Aplicativos" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Counter-Strike 2" }));
    fireEvent.click(screen.getByRole("button", { name: /Voltar/ }));
    expect(screen.getByRole("tab", { name: "Telas" })).toBeInTheDocument();
  });

  it("fica ao vivo com a fonte e a qualidade escolhidas e fecha o modal", async () => {
    await pickGame();
    fireEvent.click(screen.getByRole("radio", { name: "1440p" }));
    fireEvent.click(screen.getByRole("radio", { name: "30" }));
    expect(screen.getByText(/Mbps de upload/)).toHaveTextContent("7.1");
    fireEvent.click(screen.getByRole("button", { name: "Ficar ao vivo" }));

    await waitFor(() => expect(useStreamStore.getState().status).toBe("live"));
    expect(invoke).toHaveBeenCalledWith("start_preview", {
      source: { kind: "window", handle: 42 },
      quality: { resolution: "1440", fps: 30 },
      audioPid: 7,
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("lembra a qualidade escolhida na próxima vez", async () => {
    await pickGame();
    fireEvent.click(screen.getByRole("radio", { name: "720p" }));
    expect(useStreamStore.getState().quality).toEqual({ resolution: "720", fps: 60 });
  });

  it("explica a opção Fonte", async () => {
    await pickGame();
    fireEvent.click(screen.getByRole("radio", { name: "Fonte" }));
    expect(screen.getByText(/tamanho original/)).toBeInTheDocument();
  });

  it("mostra o erro do Rust e continua com o modal aberto", async () => {
    mockBackend(() => Promise.reject("Não deu para iniciar o encoder de vídeo: sem placa"));
    await openDialog();
    fireEvent.click(await screen.findByRole("button", { name: "Counter-Strike 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Ficar ao vivo" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("sem placa");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("usa uma mensagem genérica quando o erro não é texto", async () => {
    invoke.mockRejectedValue(new Error("objeto"));
    await useStreamStore.getState().start({ kind: "monitor", handle: 1 }, DEFAULT_QUALITY);
    expect(useStreamStore.getState()).toMatchObject({ status: "idle", error: "Não deu para começar a transmissão." });
  });
});

describe("estimativa de upload", () => {
  it("segue a fórmula do Rust e respeita piso e teto", () => {
    expect(estimatedMbps({ resolution: "1080", fps: 60 })).toBeCloseTo(7.96, 1);
    expect(estimatedMbps({ resolution: "1440", fps: 30 })).toBeCloseTo(7.08, 1);
    expect(estimatedMbps({ resolution: "720", fps: 15 })).toBe(1.5);
    expect(estimatedMbps({ resolution: "source", fps: 60 })).toBe(25);
    // Fonte menor que a resolução escolhida: não aumenta.
    expect(estimatedMbps({ resolution: "1440", fps: 60 }, 1080)).toBeCloseTo(7.96, 1);
  });
});

describe("transmitir a tela: áudio", () => {
  it("numa janela, manda o som do próprio jogo por padrão e deixa desligar", async () => {
    mockBackend();
    await openDialog();
    fireEvent.click(await screen.findByRole("button", { name: "Counter-Strike 2" }));
    const sw = screen.getByRole("switch", { name: "Transmitir o som de cs2.exe" });
    expect(sw).toHaveAttribute("aria-checked", "true");
    fireEvent.click(sw);
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(screen.getByRole("button", { name: "Ficar ao vivo" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", expect.objectContaining({ audioPid: null })),
    );
  });

  it("numa tela, começa sem som e deixa escolher o aplicativo", async () => {
    mockBackend();
    await openDialog();
    fireEvent.click(screen.getByRole("tab", { name: "Telas" }));
    fireEvent.click(await screen.findByRole("button", { name: /Monitor 1/ }));
    const select = screen.getByRole("combobox", { name: "Som de qual aplicativo?" });
    expect(select).toHaveValue("");
    fireEvent.change(select, { target: { value: "7" } });
    fireEvent.click(screen.getByRole("button", { name: "Ficar ao vivo" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_preview", expect.objectContaining({ audioPid: 7 })));
  });
});

function details() {
  render(<StreamDetailsDialog open onOpenChange={() => {}} />);
  return screen.getByRole("dialog", { name: "Detalhes da transmissão" });
}

describe("detalhes da transmissão", () => {
  it("mostra o som da live e liga o som da prévia", async () => {
    invoke.mockResolvedValue(undefined);
    useStreamStore.setState({ status: "live", stats: { ...STATS, audioLost: 2 } });
    const dialog = details();
    expect(within(dialog).getByRole("meter", { name: "Nível do som do jogo" })).toHaveAttribute("aria-valuenow", "50");
    expect(within(dialog).getByText(/128 kbps · 2 pacotes perdidos/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("switch", { name: "Ouvir o som da prévia" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_preview_listening", { listening: true }));
    expect(useStreamStore.getState().listening).toBe(true);
  });

  it("diz quando a camada de 720p está ligada para quem tem internet fraca", () => {
    useStreamStore.setState({ status: "live", stats: STATS });
    const dialog = details();
    expect(within(dialog).queryByText(/Camada de 720p/)).not.toBeInTheDocument();
    act(() => useStreamStore.setState({ stats: { ...STATS, lowLayerHeight: 720 } }));
    expect(within(dialog).getByText(/Camada de 720p ligada para quem tem internet fraca/)).toBeInTheDocument();
    act(() =>
      useStreamStore.setState({ stats: { ...STATS, lowLayerError: "a camada de 720p não abriu: sem encoder" } }),
    );
    expect(within(dialog).getByText(/a camada de 720p não abriu: sem encoder/)).toBeInTheDocument();
  });

  it("volta a chave se o Rust recusar ligar o som", async () => {
    invoke.mockRejectedValue(new Error("falhou"));
    await useStreamStore.getState().setListening(true);
    expect(useStreamStore.getState().listening).toBe(false);
  });

  it("explica quando não há som ou deu erro na captura", () => {
    useStreamStore.setState({ status: "live", stats: { ...STATS, audioActive: false } });
    const dialog = details();
    expect(within(dialog).getByText(/Sem som: ligue “Compartilhar áudio da transmissão”/)).toBeInTheDocument();
    expect(within(dialog).queryByRole("switch")).not.toBeInTheDocument();
    act(() =>
      useStreamStore.setState({
        stats: { ...STATS, audioActive: false, audioError: "não deu para capturar o áudio desse aplicativo" },
      }),
    );
    expect(within(dialog).getByText(/não deu para capturar o áudio/)).toBeInTheDocument();
  });

  it("mostra 'Medindo…' até chegar a primeira estatística", () => {
    expect(within(details()).getByText("Medindo…")).toBeInTheDocument();
  });

  it("segue funcionando se o caminho do registro não vier", async () => {
    invoke.mockRejectedValue(new Error("sem registro"));
    const unlisten = await listenToStreamEvents();
    expect(useStreamStore.getState().logPath).toBeNull();
    unlisten();
  });

  it("mostra os números com o gráfico do último minuto, as barras de uso e destaca latência alta", () => {
    useStreamStore.setState({
      status: "live",
      quality: { resolution: "720", fps: 30 },
      stats: { ...STATS, latencyMs: 80 },
      history: [STATS, { ...STATS, fps: 58 }, { ...STATS, latencyMs: 80 }],
      system: { gpu3d: 95, gpuEncode: 72, gpuDecode: 0, cpu: 20 },
    });
    const dialog = details();
    expect(within(dialog).getByText("720p 30fps")).toBeInTheDocument();
    expect(within(dialog).getByText("1920×1080")).toBeInTheDocument();
    const bitrate = within(dialog).getByRole("figure", { name: "Bitrate" });
    expect(bitrate).toHaveTextContent("8.0 Mbps");
    // O gráfico desenha a linha e a área (que deslizam com uma transição).
    expect(bitrate.querySelectorAll("path.sparkline")).toHaveLength(2);
    expect(within(dialog).getByRole("figure", { name: "Latência (captura → tela)" })).toHaveTextContent("80 ms");
    expect(within(dialog).getByText("80")).toHaveClass("text-warning");
    // Barras: verde, amarela (acima de 70%) e vermelha (acima de 90%).
    const encoder = within(dialog).getByRole("meter", { name: "Encoder da GPU" });
    expect(encoder).toHaveAttribute("aria-valuenow", "72");
    expect(encoder.firstElementChild).toHaveClass("bg-warning");
    expect(within(dialog).getByRole("meter", { name: "GPU (3D, o jogo)" }).firstElementChild).toHaveClass("bg-live");
    expect(within(dialog).getByRole("meter", { name: "CPU" }).firstElementChild).toHaveClass("bg-online");
    expect(within(dialog).getByText(/NVIDIA H.264 Encoder MFT/)).toBeInTheDocument();
  });

  it("o gráfico põe a medição mais nova na direita e cabe na altura", () => {
    const points = sparklinePoints([0, 10, 5]);
    expect(points.at(-1)![0]).toBe(240);
    expect(Math.min(...points.map(([, y]) => y))).toBeGreaterThan(0);
    expect(points[0][1]).toBe(56);
  });

  it("recebe estatísticas, o histórico, o uso do sistema e o fim da live pelos eventos do Rust", async () => {
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "session_log_path" ? "D:/SquadView/logs/sessao.jsonl" : undefined),
    );
    const unlisten = await listenToStreamEvents();
    useStreamStore.setState({ status: "live" });
    render(<App />);
    act(() => handlers.get("stream-stats")!({ payload: STATS }));
    act(() => handlers.get("stream-stats")!({ payload: { ...STATS, fps: 59 } }));
    act(() => handlers.get("system-load")!({ payload: { gpu3d: 71, gpuEncode: 9, gpuDecode: 3, cpu: 22 } }));
    expect(useStreamStore.getState().history.map((h) => h.fps)).toEqual([60, 59]);

    // Fora de uma sala, a live ocupa a área toda.
    const live = screen.getByRole("region", { name: "Sua transmissão" });
    fireEvent.contextMenu(within(live).getByRole("button", { name: /sua transmissão em foco/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Detalhes da transmissão/ }));
    const dialog = await screen.findByRole("dialog", { name: "Detalhes da transmissão" });
    expect(within(dialog).getByRole("figure", { name: "Quadros transmitidos" })).toHaveTextContent(
      "o jogo entrega 74 fps",
    );
    expect(within(dialog).getByRole("meter", { name: "Encoder da GPU" })).toHaveAttribute("aria-valuenow", "9");
    expect(dialog).toHaveTextContent("D:/SquadView/logs/sessao.jsonl");
    fireEvent.keyDown(dialog, { key: "Escape" });

    act(() => handlers.get("stream-ended")!({ payload: null }));
    // Acabou a transmissão fora de uma sala: volta para a página inicial (Amigos).
    expect(screen.getByRole("region", { name: "Amigos" })).toBeInTheDocument();
    expect(useStreamStore.getState().history).toEqual([]);
    unlisten();
    expect(handlers.size).toBe(0);
  });
});

describe("vídeo dentro do app", () => {
  it("entra e sai da tela cheia pelo menu e por dois cliques, escondendo as barras laterais", async () => {
    invoke.mockResolvedValue(undefined);
    useStreamStore.setState({ status: "live", stats: STATS });
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Salas" })).toBeVisible();

    fireEvent.contextMenu(screen.getByRole("button", { name: /sua transmissão em foco/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /^Tela cheia/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: true }));
    expect(screen.getByRole("navigation", { name: "Salas", hidden: true })).toHaveClass("hidden");

    fireEvent.doubleClick(screen.getByRole("button", { name: /sua transmissão em foco/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: false }));
  });

  it("o Esc sai da tela cheia", async () => {
    invoke.mockResolvedValue(undefined);
    const unlisten = await listenToStreamEvents();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(invoke).not.toHaveBeenCalledWith("set_fullscreen", expect.anything());
    await useStreamStore.getState().setFullscreen(true);
    expect(useStreamStore.getState().fullscreen).toBe(true);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(useStreamStore.getState().fullscreen).toBe(false));
    unlisten();
  });

  it("desfaz a tela cheia se o Rust recusar", async () => {
    invoke.mockRejectedValue(new Error("falhou"));
    await useStreamStore.getState().setFullscreen(true);
    expect(useStreamStore.getState().fullscreen).toBe(false);
  });

  it("parar a transmissão sai da tela cheia", async () => {
    invoke.mockResolvedValue(undefined);
    useStreamStore.setState({ status: "live", fullscreen: true });
    await useStreamStore.getState().stop();
    expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: false });
    expect(useStreamStore.getState().fullscreen).toBe(false);
  });
});

describe("posição do vídeo nativo", () => {
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
    vi.stubGlobal("devicePixelRatio", 1.5);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  // Regressão: a faixa abrindo (e o quadro entrando animado) muda o lugar sem mudar o tamanho, e
  // o vídeo da nossa live ficava fora do quadro; com a faixa recolhida, continuava aparecendo.
  it("segue o quadro quando ele anda numa animação e some com a faixa recolhida", () => {
    invoke.mockResolvedValue(undefined);
    useStreamStore.setState({ status: "live", stats: STATS });
    const rect = { left: 100, top: 400, width: 200, height: 112 };
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ ...rect, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect,
    );
    const tile = (hidden: boolean) => (
      <StreamTile
        variant="strip"
        hidden={hidden}
        focused={false}
        onActivate={() => {}}
        onChange={() => {}}
        onDetails={() => {}}
      />
    );
    const { rerender } = render(tile(false));
    expect(invoke).toHaveBeenLastCalledWith("set_video_bounds", expect.objectContaining({ x: 150, y: 600 }));

    // A faixa terminou de abrir: o quadro subiu sem mudar de tamanho.
    rect.top = 300;
    act(() => document.body.dispatchEvent(new Event("transitionend", { bubbles: true })));
    expect(invoke).toHaveBeenLastCalledWith("set_video_bounds", expect.objectContaining({ x: 150, y: 450 }));

    rerender(tile(true));
    expect(invoke).toHaveBeenLastCalledWith("set_video_bounds", { x: 0, y: 0, width: 0, height: 0, radius: 0 });
  });

  it("manda a área do vídeo em pixels físicos, só quando muda, e esconde ao sair", () => {
    invoke.mockResolvedValue(undefined);
    useStreamStore.setState({ status: "live", stats: STATS });
    const rect = { left: 100, top: 40, width: 800, height: 450 };
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ ...rect, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect,
    );
    const { unmount } = render(
      <StreamTile variant="focus" focused={false} onActivate={() => {}} onChange={() => {}} onDetails={() => {}} />,
    );
    expect(invoke).toHaveBeenCalledWith("set_video_bounds", { x: 150, y: 60, width: 1200, height: 675, radius: 0 });

    invoke.mockClear();
    act(() => observed!());
    expect(invoke).not.toHaveBeenCalled();

    rect.width = 1000;
    act(() => observed!());
    expect(invoke).toHaveBeenCalledWith("set_video_bounds", { x: 150, y: 60, width: 1500, height: 675, radius: 0 });

    unmount();
    expect(invoke).toHaveBeenCalledWith("set_video_bounds", { x: 0, y: 0, width: 0, height: 0, radius: 0 });
  });

  it("não esconde o vídeo com um modal aberto: ele fica por baixo da interface e o modal, por cima", async () => {
    mockBackend();
    await openDialog();
    fireEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(invoke).not.toHaveBeenCalledWith("set_video_hidden", expect.anything());
  });
});
