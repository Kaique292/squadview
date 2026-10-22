import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { playVideoExit } from "../ui/dissolve-effect";
import { toast } from "../ui/toast-store";
import { OWN_VIDEO, videoKey } from "./video-key";

/**
 * O que a janelinha do picture in picture mostra além do vídeo (espelha `room::PipInfo`): quem
 * transmite, o som da live e o tamanho da imagem, para ela abrir na proporção certa. O store da
 * sala vem sob demanda: ele importa este aqui.
 */
async function pipInfo(memberId: string) {
  const { useRoomStore } = await import("../room/room-store");
  const watch = useRoomStore.getState().watching[memberId];
  return {
    name: watch?.member.name ?? "",
    listening: watch?.listening ?? true,
    volume: watch?.volume ?? 100,
    width: watch?.stats?.width ?? 0,
    height: watch?.stats?.height ?? 0,
  };
}

/** Espelha `sv_capture::CaptureSource` (serde: { kind, handle }). */
export type CaptureSource = { kind: "monitor" | "window"; handle: number };

/** Espelha `sv_capture::Source`. */
export interface Source {
  id: CaptureSource;
  name: string;
  processId: number | null;
  processName: string | null;
}

/** Espelha `sv_core::Resolution`. */
export type Resolution = "720" | "1080" | "1440" | "source";

/** Espelha `sv_core::StreamQuality`. Tudo liberado, sem plano pago. */
export interface StreamQuality {
  resolution: Resolution;
  fps: 15 | 30 | 60;
}

export const DEFAULT_QUALITY: StreamQuality = { resolution: "1080", fps: 60 };

/** Espelha `sv_session::StreamStats`. */
export interface StreamStats {
  rttMs: number;
  audioActive: boolean;
  audioError: string | null;
  audioKbps: number;
  audioLevel: number;
  audioLost: number;
  sourceFps: number;
  fps: number;
  bitrateKbps: number;
  encodeMs: number;
  latencyMs: number;
  latencyP95Ms: number;
  dropped: number;
  width: number;
  height: number;
  encoder: string;
}

/** Espelha `sv_session::SystemLoad` (porcentagens). */
export interface SystemLoad {
  gpu3d: number;
  gpuEncode: number;
  gpuDecode: number;
  cpu: number;
}

/** Espelha `preview::Notice` do app em Rust. */
export interface StreamNotice {
  kind: "no-frames" | "source-lost" | "source-changed";
  message: string;
}

type Status = "idle" | "starting" | "live";

export function sourceKey(id: CaptureSource) {
  return `${id.kind}-${id.handle}`;
}

/** Quantas medições guardar para os gráficos dos detalhes (a cada 500 ms: 1 minuto). */
export const HISTORY_SIZE = 120;

/** O que está sendo transmitido agora (para trocar a fonte, a qualidade ou o som sem parar). */
export interface CurrentStream {
  source: CaptureSource;
  /** Programa cujo som vai junto (`null`: sem som). */
  audioPid: number | null;
  /** O último programa escolhido para o som, para religar o som depois de desligar. */
  lastAudioPid: number | null;
}

/** Miniaturas pedidas ao mesmo tempo (cada uma captura um frame da fonte). */
const THUMBNAIL_CONCURRENCY = 4;

interface StreamState {
  status: Status;
  sources: Source[];
  loadingSources: boolean;
  /** Miniatura por fonte: data URL, `null` quando a fonte não mandou imagem (ex.: minimizada). */
  thumbnails: Record<string, string | null>;
  quality: StreamQuality;
  stats: StreamStats | null;
  /** As últimas medições, da mais antiga para a mais nova (os gráficos dos detalhes). */
  history: StreamStats[];
  system: SystemLoad | null;
  current: CurrentStream | null;
  notice: StreamNotice | null;
  logPath: string | null;
  error: string | null;
  loadSources: () => Promise<void>;
  setQuality: (quality: StreamQuality) => void;
  /** Começa a transmitir; já ao vivo, troca a fonte sem sair do ar. */
  start: (source: CaptureSource, quality: StreamQuality, audioPid?: number | null) => Promise<void>;
  /** Troca a qualidade sem parar a transmissão. */
  changeQuality: (quality: StreamQuality) => Promise<void>;
  /** Liga ou desliga o som que vai junto, sem parar a transmissão. */
  setShareAudio: (share: boolean) => Promise<void>;
  /** Ouvir o som da prévia neste PC (sai duas vezes: use fone). */
  listening: boolean;
  setListening: (listening: boolean) => Promise<void>;
  /** Um vídeo ocupa a tela inteira (a janela do app em tela cheia, sem as barras laterais). */
  fullscreen: boolean;
  /** Qual vídeo está em tela cheia: `OWN_VIDEO` (a nossa live) ou o id de quem assistimos. */
  fullscreenOf: string | null;
  /** Põe (ou tira) em tela cheia o vídeo `of` (sem `of`: a nossa live). */
  setFullscreen: (fullscreen: boolean, of?: string) => Promise<void>;
  /** A live (id de quem transmite) solta numa janela pequena, sempre no topo. Uma por vez. */
  pip: string | null;
  setPip: (memberId: string | null) => Promise<void>;
  stop: () => Promise<void>;
}

export const useStreamStore = create<StreamState>((set, get) => ({
  status: "idle",
  sources: [],
  loadingSources: false,
  thumbnails: {},
  quality: DEFAULT_QUALITY,
  stats: null,
  history: [],
  system: null,
  current: null,
  notice: null,
  logPath: null,
  error: null,

  async loadSources() {
    set({ loadingSources: true, error: null, thumbnails: {} });
    let sources: Source[];
    try {
      sources = await invoke<Source[]>("list_sources");
      set({ sources });
    } catch {
      set({ error: "Não deu para listar as telas e janelas." });
      return;
    } finally {
      set({ loadingSources: false });
    }

    // Miniaturas em paralelo, poucas por vez; cada uma aparece assim que fica pronta.
    const queue = [...sources];
    const worker = async () => {
      for (let s = queue.shift(); s; s = queue.shift()) {
        const key = sourceKey(s.id);
        const url = await invoke<string | null>("source_thumbnail", { source: s.id }).catch(() => null);
        set({ thumbnails: { ...get().thumbnails, [key]: url } });
      }
    };
    await Promise.all(Array.from({ length: THUMBNAIL_CONCURRENCY }, worker));
  },

  setQuality(quality) {
    set({ quality });
  },

  fullscreen: false,
  fullscreenOf: null,
  async setFullscreen(fullscreen, of = OWN_VIDEO) {
    const before = get();
    // Já em tela cheia, trocar de vídeo não mexe na janela.
    if (before.fullscreen && fullscreen) {
      set({ fullscreenOf: of });
      return;
    }
    if (before.fullscreen === fullscreen) return;
    set({ fullscreen, fullscreenOf: fullscreen ? of : null });
    try {
      await invoke("set_fullscreen", { fullscreen });
    } catch {
      set({ fullscreen: before.fullscreen, fullscreenOf: before.fullscreenOf });
    }
  },

  pip: null,
  async setPip(memberId) {
    const before = get().pip;
    if (before === memberId) return;
    // Tela cheia e picture in picture não combinam: soltar o vídeo sai da tela cheia antes.
    if (memberId) await get().setFullscreen(false);
    set({ pip: memberId });
    try {
      // Soltar outra traz a anterior de volta sozinha (o Rust deixa uma por vez).
      if (memberId) await invoke("room_set_pip", { memberId, pip: true, info: await pipInfo(memberId) });
      else if (before) await invoke("room_set_pip", { memberId: before, pip: false });
    } catch {
      set({ pip: before });
    }
  },

  listening: false,
  async setListening(listening) {
    set({ listening });
    try {
      await invoke("set_preview_listening", { listening });
    } catch {
      set({ listening: !listening });
    }
  },

  async start(source, quality, audioPid = null) {
    // Já ao vivo é uma troca: a live continua na tela (e na sala) enquanto a nova fonte começa.
    const live = get().status === "live";
    set({ status: live ? "live" : "starting", notice: null, error: null, quality, listening: false });
    if (!live) set({ stats: null, history: [] });
    try {
      await invoke("start_preview", { source, quality, audioPid });
      const lastAudioPid = audioPid ?? get().current?.lastAudioPid ?? null;
      set({ status: "live", current: { source, audioPid, lastAudioPid } });
    } catch (e) {
      const message = typeof e === "string" ? e : "Não deu para começar a transmissão.";
      if (live) {
        // A troca falhou: o Rust já parou a anterior.
        set({ status: "idle", stats: null, current: null, error: message });
        toast.error(message, "Transmissão");
      } else set({ status: "idle", error: message });
    }
  },

  async changeQuality(quality) {
    const current = get().current;
    if (!current) return;
    await get().start(current.source, quality, current.audioPid);
  },

  async setShareAudio(share) {
    const current = get().current;
    if (!current) return;
    const audioPid = share ? current.lastAudioPid : null;
    if (share && audioPid === null) return;
    await get().start(current.source, get().quality, audioPid);
  },

  async stop() {
    await get().setFullscreen(false);
    // Antes de fechar o vídeo, o Rust manda a foto do último quadro (evento `video-exit`).
    await invoke("stop_preview");
    set({ status: "idle", stats: null, history: [], current: null });
  },
}));

/**
 * Teto do upload por pessoa assistindo, em Mbps. Mesma fórmula de `StreamQuality::bitrate_kbps`
 * no Rust (0,064 bit por pixel por quadro, entre 1,5 e 25 Mbps), assumindo uma fonte 16:9.
 * Sem saber o tamanho da fonte, supõe que ela preenche a resolução escolhida (4K em "Fonte").
 */
export function estimatedMbps(quality: StreamQuality, sourceHeight = 2160) {
  const height = quality.resolution === "source" ? sourceHeight : Math.min(Number(quality.resolution), sourceHeight);
  const width = Math.round((height * 16) / 9);
  const kbps = Math.min(25_000, Math.max(1_500, (width * height * quality.fps * 64) / 1_000_000));
  return kbps / 1000;
}

/** Liga os eventos vindos do Rust ao store. Chamar uma vez, ao abrir o app. */
export async function listenToStreamEvents() {
  const unlistenStats = await listen<StreamStats>("stream-stats", (e) => {
    const history = [...useStreamStore.getState().history, e.payload].slice(-HISTORY_SIZE);
    useStreamStore.setState({ stats: e.payload, history });
  });
  const unlistenEnded = await listen("stream-ended", () => {
    void useStreamStore.getState().setFullscreen(false);
    useStreamStore.setState({ status: "idle", stats: null, history: [], current: null, notice: null });
  });
  const unlistenNotice = await listen<StreamNotice | null>("stream-notice", (e) =>
    useStreamStore.setState({ notice: e.payload }),
  );
  // A janelinha do picture in picture fechou (voltar, dois cliques ou Alt+F4): o vídeo volta para o app.
  const unlistenPip = await listen<{ pip: boolean; source: string | null }>("video-pip", (e) => {
    const { pip, setPip } = useStreamStore.getState();
    if (!e.payload.pip && pip === e.payload.source) void setPip(null);
  });
  // Esc sai da tela cheia.
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape" && useStreamStore.getState().fullscreen) void useStreamStore.getState().setFullscreen(false);
  };
  window.addEventListener("keydown", onKey);
  const unlistenSystem = await listen<SystemLoad>("system-load", (e) => useStreamStore.setState({ system: e.payload }));
  // Foto do último quadro, logo antes de um vídeo (o nosso ou o de quem assistimos) fechar.
  const unlistenExit = await listen<{ image: string | null; source: string | null }>("video-exit", (e) =>
    playVideoExit(e.payload.image, videoKey(e.payload.source)),
  );
  try {
    useStreamStore.setState({ logPath: await invoke<string | null>("session_log_path") });
  } catch {
    // Sem registro (ex.: pasta sem permissão): a interface só não mostra o caminho.
  }
  return () => {
    unlistenStats();
    unlistenEnded();
    unlistenSystem();
    unlistenExit();
    unlistenNotice();
    unlistenPip();
    window.removeEventListener("keydown", onKey);
  };
}
