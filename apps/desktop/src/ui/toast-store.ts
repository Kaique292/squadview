import { create } from "zustand";

/**
 * Tipos de notificação:
 * - `info`: mensagem normal do sistema;
 * - `success`: deu certo;
 * - `warning`: aviso (algo pode dar errado, mas segue);
 * - `alert`: alerta (precisa da atenção do usuário agora);
 * - `error`: deu errado.
 */
export type ToastKind = "info" | "success" | "warning" | "alert" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  title?: string;
  message: string;
  /** Quanto tempo fica na tela, em ms (0 = até o usuário fechar). */
  duration: number;
  /** Saindo (animação de saída em andamento). */
  leaving: boolean;
}

/** Tempo padrão por tipo: erros e alertas ficam mais. */
const DEFAULT_DURATION: Record<ToastKind, number> = {
  info: 4000,
  success: 4000,
  warning: 6000,
  alert: 8000,
  error: 8000,
};
/** Duração da animação de saída (igual à do CSS `toast-out`). */
export const TOAST_EXIT_MS = 220;
/** Mais que isso, as mais antigas saem. */
const MAX_TOASTS = 5;

interface ToastState {
  toasts: Toast[];
  push: (toast: { kind: ToastKind; message: string; title?: string; duration?: number }) => number;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],

  push({ kind, message, title, duration }) {
    // A mesma mensagem já na tela: não repete (ex.: o mesmo erro chegando duas vezes).
    const same = get().toasts.find((t) => !t.leaving && t.message === message && t.kind === kind);
    if (same) return same.id;
    const id = nextId++;
    const toast: Toast = { id, kind, title, message, duration: duration ?? DEFAULT_DURATION[kind], leaving: false };
    const active = [toast, ...get().toasts];
    set({ toasts: active });
    // As mais antigas além do limite saem com animação.
    active
      .filter((t) => !t.leaving)
      .slice(MAX_TOASTS)
      .forEach((t) => get().dismiss(t.id));
    return id;
  },

  dismiss(id) {
    const t = get().toasts.find((x) => x.id === id);
    if (!t || t.leaving) return;
    set({ toasts: get().toasts.map((x) => (x.id === id ? { ...x, leaving: true } : x)) });
    setTimeout(() => set({ toasts: get().toasts.filter((x) => x.id !== id) }), TOAST_EXIT_MS);
  },
}));

/** Atalhos: `toast.success("Entrou na sala")`. */
export const toast = {
  info: (message: string, title?: string) => useToastStore.getState().push({ kind: "info", message, title }),
  success: (message: string, title?: string) => useToastStore.getState().push({ kind: "success", message, title }),
  warning: (message: string, title?: string) => useToastStore.getState().push({ kind: "warning", message, title }),
  alert: (message: string, title?: string) => useToastStore.getState().push({ kind: "alert", message, title }),
  error: (message: string, title?: string) => useToastStore.getState().push({ kind: "error", message, title }),
};
