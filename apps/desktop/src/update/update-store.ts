import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";

/** Estado da atualização automática (ver docs/adr/0006-atualizacao-automatica.md). */
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; percent: number | null }
  | { kind: "error"; message: string };

interface UpdateState {
  status: UpdateStatus;
  checkForUpdate: () => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

// O objeto Update guarda o recurso nativo do download; não vai para o estado da interface.
let pending: Update | null = null;

export const useUpdateStore = create<UpdateState>((set) => ({
  status: { kind: "idle" },

  async checkForUpdate() {
    try {
      const update = await check();
      if (!update) return;
      pending = update;
      set({ status: { kind: "available", version: update.version } });
    } catch {
      // Sem internet ou GitHub fora do ar: tenta de novo na próxima abertura, sem incomodar.
      set({ status: { kind: "idle" } });
    }
  },

  async install() {
    if (!pending) return;
    let total = 0;
    let received = 0;
    set({ status: { kind: "downloading", percent: null } });
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          set({ status: { kind: "downloading", percent: total ? Math.round((received / total) * 100) : null } });
        }
      });
      await relaunch();
    } catch {
      set({
        status: { kind: "error", message: "Não deu para atualizar agora. Tentamos de novo na próxima abertura." },
      });
    }
  },

  dismiss() {
    set({ status: { kind: "idle" } });
  },
}));
