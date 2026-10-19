import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";

/** Estado da atualização automática (ver docs/adr/0006-atualizacao-automatica.md). */
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "error"; version: string; message: string };

interface UpdateState {
  status: UpdateStatus;
  /** Novidades da versão nova (corpo da release, em Markdown). */
  notes: string | null;
  checkForUpdate: () => Promise<void>;
  install: () => Promise<void>;
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

  async install() {
    const { status } = get();
    if (!pending || status.kind === "idle" || status.kind === "downloading") return;
    const version = status.version;
    let total = 0;
    let received = 0;
    set({ status: { kind: "downloading", version, percent: null } });
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
      set({ status: { kind: "error", version, message: "Não deu para baixar a atualização. Tente de novo." } });
    }
  },
}));

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
