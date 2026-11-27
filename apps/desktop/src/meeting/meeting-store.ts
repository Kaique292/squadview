import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { useRoomStore } from "../room/room-store";
import { toast } from "../ui/toast-store";

/** Os modelos do Whisper que dá para baixar (espelha `meeting::ModelKind`). */
export type ModelKind = "small" | "base";

export interface ModelStatus {
  kind: ModelKind;
  /** Tamanho do download, em bytes. */
  size: number;
  installed: boolean;
}

/** Uma linha da aba Reunião. */
export interface MeetingLine {
  id: number;
  /** Quem falou (`"me"` = nós; as falas dos outros chegam no próximo passo do modo reunião). */
  memberId: string;
  /** Quando a fala começou (ms desde 1970). */
  atMs: number;
  kind: "text" | "skipped" | "failed";
  text?: string;
  error?: string;
}

/** Como a linha chega do Rust (`meeting-line`). */
type LinePayload =
  | { kind: "text"; atMs: number; text: string }
  | { kind: "skipped"; atMs: number }
  | { kind: "failed"; atMs: number; error: string };

export const MODEL_LABELS: Record<ModelKind, string> = {
  small: "Preciso",
  base: "Leve",
};

const PREFS_KEY = "squadview.reuniao";

interface MeetingPrefs {
  model: ModelKind;
  /** Idioma da fala (`null`: o Whisper detecta sozinho). */
  language: string | null;
}

const DEFAULT_PREFS: MeetingPrefs = { model: "small", language: "pt" };

function loadPrefs(): MeetingPrefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    return raw ? { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<MeetingPrefs>) } : DEFAULT_PREFS;
  } catch {
    return DEFAULT_PREFS;
  }
}

function savePrefs(prefs: MeetingPrefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Sem armazenamento: vale só nesta sessão.
  }
}

function message(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "Erro desconhecido.";
}

interface MeetingState {
  /** A aba Reunião está aberta ao lado da sala. */
  open: boolean;
  /** `null` até a primeira consulta ao Rust. */
  cpuSupported: boolean | null;
  models: ModelStatus[];
  prefs: MeetingPrefs;
  /** Download em andamento (bytes). */
  download: { kind: ModelKind; received: number; total: number } | null;
  /** Carregando o modelo para começar. */
  starting: boolean;
  transcribing: boolean;
  lines: MeetingLine[];
  setOpen: (open: boolean) => void;
  loadStatus: () => Promise<void>;
  setPrefs: (changes: Partial<MeetingPrefs>) => void;
  downloadModel: (kind: ModelKind) => Promise<void>;
  cancelDownload: () => Promise<void>;
  deleteModel: (kind: ModelKind) => Promise<void>;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  /** Saímos da sala: a transcrição parou junto com a voz, e a aba fecha. */
  reset: () => void;
}

let nextId = 1;

export const useMeetingStore = create<MeetingState>((set, get) => ({
  open: false,
  cpuSupported: null,
  models: [],
  prefs: loadPrefs(),
  download: null,
  starting: false,
  transcribing: false,
  lines: [],

  setOpen(open) {
    set({ open });
    if (open) void get().loadStatus();
  },

  async loadStatus() {
    try {
      const status = await invoke<{ cpuSupported: boolean; models: ModelStatus[] }>("meeting_status");
      set({ cpuSupported: status.cpuSupported, models: status.models });
    } catch {
      // Fora do app (navegador, testes): fica como está.
    }
  },

  setPrefs(changes) {
    const prefs = { ...get().prefs, ...changes };
    set({ prefs });
    savePrefs(prefs);
  },

  async downloadModel(kind) {
    if (get().download) return;
    const total = get().models.find((m) => m.kind === kind)?.size ?? 0;
    set({ download: { kind, received: 0, total } });
    try {
      await invoke("meeting_download_model", { kind });
    } catch (e) {
      const text = message(e);
      if (text !== "Download cancelado.") toast.warning(text, "Reunião");
    } finally {
      set({ download: null });
      await get().loadStatus();
    }
  },

  async cancelDownload() {
    await invoke("meeting_cancel_download").catch(() => {});
  },

  async deleteModel(kind) {
    if (get().transcribing && get().prefs.model === kind) await get().stop();
    try {
      await invoke("meeting_delete_model", { kind });
    } catch (e) {
      toast.warning(message(e), "Reunião");
    }
    await get().loadStatus();
  },

  async start() {
    if (get().starting || get().transcribing) return;
    set({ starting: true });
    const { model, language } = get().prefs;
    try {
      await invoke("meeting_set_transcribing", { enabled: true, model, language });
      set({ transcribing: true });
    } catch (e) {
      toast.warning(message(e), "Reunião");
    } finally {
      set({ starting: false });
    }
  },

  async stop() {
    set({ transcribing: false });
    const { model, language } = get().prefs;
    await invoke("meeting_set_transcribing", { enabled: false, model, language }).catch(() => {});
  },

  reset() {
    set({ open: false, transcribing: false, starting: false, lines: [] });
  },
}));

/** Ouve as linhas da transcrição e o progresso do download do modelo. */
export async function listenToMeetingEvents() {
  // Saiu da sala (ou ela acabou): a voz parou, e a transcrição com ela.
  const offRoom = useRoomStore.subscribe((s, prev) => {
    if (prev.code && s.code !== prev.code) useMeetingStore.getState().reset();
  });
  const offs = await Promise.all([
    listen<LinePayload>("meeting-line", (e) => {
      const line: MeetingLine = { id: nextId++, memberId: "me", ...e.payload };
      // Em ordem de quando a fala começou: uma fala curta pode ficar pronta antes de uma longa.
      useMeetingStore.setState((s) => {
        const lines = [...s.lines, line];
        lines.sort((a, b) => a.atMs - b.atMs || a.id - b.id);
        return { lines };
      });
    }),
    listen<{ kind: ModelKind; received: number; total: number }>("meeting-model-progress", (e) => {
      if (useMeetingStore.getState().download) useMeetingStore.setState({ download: e.payload });
    }),
  ]);
  return () => {
    offRoom();
    offs.forEach((off) => off());
  };
}
