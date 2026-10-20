import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";
import { forgetRoomToRejoin, rememberRoomToRejoin } from "../room/room-store";

/** Estado da atualização automática (ver docs/adr/0006-atualizacao-automatica.md). */
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "error"; version: string; message: string };

/** O andamento da atualização na abertura, para as barras da splash. */
export interface StartupUpdateProgress {
  version: string;
  download: number | null;
  install: number;
}

/** Quanto a barra de "Instalando" leva para encher antes de o instalador fechar o app. */
export const INSTALL_FILL_MS = 900;

interface UpdateState {
  status: UpdateStatus;
  /** Novidades da versão nova (corpo da release, em Markdown). */
  notes: string | null;
  checkForUpdate: () => Promise<void>;
  install: () => Promise<void>;
  /**
   * Abriu o app e havia versão nova: baixa e instala já, mostrando o andamento na splash (como o
   * Discord). O instalador fecha e reabre o app; se der errado, volta e a abertura continua.
   */
  installAtStartup: (
    report: (p: StartupUpdateProgress) => void,
    sleep?: (ms: number) => Promise<void>,
  ) => Promise<void>;
}

/** Novidades da versão que está sendo instalada, para mostrar ao abrir de novo. */
const WHATS_NEW_KEY = "squadview.novidades";

export interface WhatsNew {
  version: string;
  notes: string | null;
}

function remember(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Sem armazenamento: só não volta para a sala nem mostra as novidades.
  }
}

/** Lê e apaga (vale uma vez só). */
function takeOnce<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    localStorage.removeItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

/** As novidades da versão que acabou de ser instalada (uma vez só, depois de atualizar). */
export function takeWhatsNew(): WhatsNew | null {
  return takeOnce<WhatsNew>(WHATS_NEW_KEY);
}

/** De quanto em quanto tempo procurar versão nova com o app aberto. */
export const CHECK_EVERY_MS = 60 * 60 * 1000;

// O objeto Update guarda o recurso nativo do download; não vai para o estado da interface.
let pending: Update | null = null;

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: { kind: "idle" },
  notes: null,

  async checkForUpdate() {
    // Já achou (ou está baixando): não troca o que a pessoa está vendo.
    if (get().status.kind !== "idle") return;
    try {
      const update = await check();
      if (!update) return;
      pending = update;
      set({ status: { kind: "available", version: update.version }, notes: update.body ?? null });
    } catch {
      // Sem internet ou GitHub fora do ar: tenta de novo mais tarde, sem incomodar.
    }
  },

  async installAtStartup(report, sleep = wait) {
    const { status } = get();
    if (!pending || status.kind !== "available") return;
    const version = status.version;
    let total = 0;
    let received = 0;
    set({ status: { kind: "downloading", version, percent: null } });
    remember(WHATS_NEW_KEY, { version, notes: get().notes });
    try {
      report({ version, download: 0, install: 0 });
      await pending.download((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          const percent = total ? Math.min(100, Math.round((received / total) * 100)) : null;
          set({ status: { kind: "downloading", version, percent } });
          report({ version, download: percent, install: 0 });
        }
      });
      // Baixou tudo: a barra de download vai até o fim, e a de instalação enche antes de o
      // instalador fechar o app (ele instala em silêncio e abre de novo).
      report({ version, download: 100, install: 0 });
      for (const step of [35, 70, 100]) {
        await sleep(INSTALL_FILL_MS / 3);
        report({ version, download: 100, install: step });
      }
      await pending.install();
      await relaunch();
    } catch {
      takeOnce(WHATS_NEW_KEY);
      set({ status: { kind: "error", version, message: "Não deu para baixar a atualização. Tente de novo." } });
    }
  },

  async install() {
    const { status } = get();
    if (!pending || status.kind === "idle" || status.kind === "downloading") return;
    const version = status.version;
    let total = 0;
    let received = 0;
    set({ status: { kind: "downloading", version, percent: null } });
    // O app fecha para instalar: guarda a sala para voltar e as novidades para mostrar depois.
    rememberRoomToRejoin();
    remember(WHATS_NEW_KEY, { version, notes: get().notes });
    try {
      // No Windows o instalador fecha o app, instala e abre de novo (passa pela splash).
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          set({
            status: { kind: "downloading", version, percent: total ? Math.round((received / total) * 100) : null },
          });
        }
      });
      await relaunch();
    } catch {
      // Não atualizou: não há para onde voltar nem novidades.
      forgetRoomToRejoin();
      takeOnce(WHATS_NEW_KEY);
      set({ status: { kind: "error", version, message: "Não deu para baixar a atualização. Tente de novo." } });
    }
  },
}));

const wait = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export interface NotesSection {
  title: string;
  items: string[];
}

/**
 * Transforma as notas da release (Markdown do release-please) em seções simples para o modal:
 * tira os links dos commits, o negrito do escopo e as linhas de versão.
 */
export function parseNotes(markdown: string | null): NotesSection[] {
  if (!markdown) return [];
  const sections: NotesSection[] = [];
  for (const raw of markdown.split(/\r?\n/)) {
    const line = raw.trim();
    const heading = /^###\s+(.+)$/.exec(line);
    if (heading) {
      sections.push({ title: heading[1].trim(), items: [] });
      continue;
    }
    const item = /^[*-]\s+(.+)$/.exec(line);
    if (!item) continue;
    const text = item[1]
      .replace(/\s*\(\[[0-9a-f]{7,}\]\([^)]*\)\)/gi, "")
      .replace(/\*\*[^*]+:\*\*\s*/g, "")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .trim();
    if (!text) continue;
    if (sections.length === 0) sections.push({ title: "Novidades", items: [] });
    sections[sections.length - 1].items.push(text.charAt(0).toUpperCase() + text.slice(1));
  }
  return sections.filter((s) => s.items.length > 0);
}
