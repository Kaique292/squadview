import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

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
  system: SystemLoad | null;
  notice: StreamNotice | null;
  logPath: string | null;
  error: string | null;
  loadSources: () => Promise<void>;
  setQuality: (quality: StreamQuality) => void;
  start: (source: CaptureSource, quality: StreamQuality, audioPid?: number | null) => Promise<void>;
  /** Ouvir o som da prévia neste PC (sai duas vezes: use fone). */
  listening: boolean;
  setListening: (listening: boolean) => Promise<void>;
  /** O stream ocupa a tela inteira (a janela do app em tela cheia, sem as barras laterais). */
  fullscreen: boolean;
  setFullscreen: (fullscreen: boolean) => Promise<void>;
  stop: () => Promise<void>;
}

export const useStreamStore = create<StreamState>((set, get) => ({
  status: "idle",
  sources: [],
  loadingSources: false,
  thumbnails: {},
  quality: DEFAULT_QUALITY,
  stats: null,
  system: null,
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
  async setFullscreen(fullscreen) {
    if (get().fullscreen === fullscreen) return;
    set({ fullscreen });
    try {
      await invoke("set_fullscreen", { fullscreen });
    } catch {
      set({ fullscreen: !fullscreen });
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
    set({ status: "starting", stats: null, notice: null, error: null, quality, listening: false });
    try {
      await invoke("start_preview", { source, quality, audioPid });
      set({ status: "live" });
    } catch (e) {
      set({ status: "idle", error: typeof e === "string" ? e : "Não deu para começar a transmissão." });
    }
  },

  async stop() {
    await get().setFullscreen(false);
    await invoke("stop_preview");
    set({ status: "idle", stats: null });
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
  const unlistenStats = await listen<StreamStats>("stream-stats", (e) => useStreamStore.setState({ stats: e.payload }));
  const unlistenEnded = await listen("stream-ended", () => {
    void useStreamStore.getState().setFullscreen(false);
    useStreamStore.setState({ status: "idle", stats: null, notice: null });
  });
  const unlistenNotice = await listen<StreamNotice | null>("stream-notice", (e) =>
    useStreamStore.setState({ notice: e.payload }),
  );
  // Pedidos de tela cheia feitos dentro do vídeo nativo (que recebe o teclado e os cliques dele).
  const unlistenFullscreen = await listen<"toggle" | "exit">("video-fullscreen", (e) => {
    const { fullscreen, setFullscreen } = useStreamStore.getState();
    void setFullscreen(e.payload === "toggle" ? !fullscreen : false);
  });
  // Esc com o foco na interface também sai da tela cheia.
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape" && useStreamStore.getState().fullscreen) void useStreamStore.getState().setFullscreen(false);
  };
  window.addEventListener("keydown", onKey);
  const unlistenSystem = await listen<SystemLoad>("system-load", (e) => useStreamStore.setState({ system: e.payload }));
  try {
    useStreamStore.setState({ logPath: await invoke<string | null>("session_log_path") });
  } catch {
    // Sem registro (ex.: pasta sem permissão): a interface só não mostra o caminho.
  }
  return () => {
    unlistenStats();
    unlistenEnded();
    unlistenSystem();
    unlistenNotice();
    unlistenFullscreen();
    window.removeEventListener("keydown", onKey);
  };
}
