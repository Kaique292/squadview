import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { useFriendsStore } from "../friends/friends-store";
import { sound } from "../sounds/sound-store";
import { toast } from "../ui/toast-store";

/**
 * Mensagens privadas (ADR 0017): conversas direto entre os dois PCs. O histórico fica no Rust
 * (`conversas/`); aqui ficam a conversa aberta, quem está conectado e as não lidas.
 */

/** Espelha `sv_session::dm::Attachment`. */
export interface Attachment {
  /** O arquivo não existe mais no PC de quem mandou: não dá para baixar. */
  gone?: boolean;
  name: string;
  size: number;
  mime: string;
  /** Onde o arquivo está neste PC (`null`: ainda não baixado). */
  path: string | null;
  /** Quanto já foi (bytes). */
  done: number;
}

/** Espelha `sv_session::dm::DmMessage`. */
export interface DmMessage {
  id: string;
  mine: boolean;
  at: number;
  text?: string;
  file?: Attachment;
  status: "pending" | "delivered" | "received";
}

export interface Conversation {
  friend: string;
  last: DmMessage | null;
  pending: number;
}

type DmEvent =
  | { type: "message"; friend: string; message: DmMessage }
  | { type: "online"; friend: string; online: boolean }
  | { type: "unreachable"; friend: string };

/** O que fazer com uma cópia do anexo que mandamos. */
export type CopyChoice = "ask" | "keep" | "send";

const COPY_KEY = "squadview.anexos.copia";
/** Fotos até esse tamanho chegam sozinhas (o resto pede "Baixar"). */
export const AUTO_DOWNLOAD_MAX = 8 * 1024 * 1024;

function savedCopyChoice(): CopyChoice {
  try {
    const v = localStorage.getItem(COPY_KEY);
    return v === "keep" || v === "send" ? v : "ask";
  } catch {
    return "ask";
  }
}

/** Mensagem de anexo que chega sozinha: foto pequena, ainda não baixada. */
export function autoDownloads(m: DmMessage): boolean {
  return (
    !m.mine && !!m.file && m.file.path === null && m.file.mime.startsWith("image/") && m.file.size <= AUTO_DOWNLOAD_MAX
  );
}

interface DmState {
  conversations: Conversation[];
  /** Mensagens de cada conversa já aberta (da mais antiga para a mais nova). */
  messages: Record<string, DmMessage[]>;
  /** Conversa aberta (`null`: a lista de amigos). */
  open: string | null;
  /** Conexão direta aberta com o amigo. */
  online: Record<string, boolean>;
  unread: Record<string, number>;
  /** Anexos esperando a escolha da cópia (o modal da primeira vez). */
  pendingFiles: { friend: string; paths: string[] } | null;
  copyChoice: CopyChoice;
  /** Anexos sendo baixados agora (para mostrar o progresso). */
  downloading: Record<string, boolean>;
  load: () => Promise<void>;
  openChat: (friend: string | null) => Promise<void>;
  sendText: (friend: string, text: string) => Promise<boolean>;
  /** Manda arquivos; na primeira vez (escolha "perguntar"), abre o modal da cópia. */
  sendFiles: (friend: string, paths: string[]) => Promise<void>;
  /** Resposta do modal: guardar cópia ou não, e se lembra a escolha. */
  resolveCopy: (keep: boolean | null, remember: boolean) => Promise<void>;
  setCopyChoice: (choice: CopyChoice) => void;
  /** Baixa um anexo (sem `dest`: na pasta de anexos do app). */
  download: (friend: string, message: DmMessage, dest?: string) => Promise<void>;
  /** Para um download no meio (o que já tinha chegado é apagado). */
  cancelDownload: (friend: string, message: DmMessage) => Promise<void>;
}

export const useDmStore = create<DmState>((set, get) => ({
  conversations: [],
  messages: {},
  open: null,
  online: {},
  unread: {},
  pendingFiles: null,
  copyChoice: savedCopyChoice(),
  downloading: {},

  async load() {
    try {
      const conversations = await invoke<Conversation[]>("dm_conversations");
      for (const c of conversations) if (c.last) seen.add(c.last.id);
      set({ conversations });
    } catch {
      // Sem login: a lista fica vazia ("Nenhuma conversa ainda.").
    }
  },

  async openChat(friend) {
    set({ open: friend });
    if (!friend) return;
    if (friend) useFriendsStore.setState({ open: true });
    set({ unread: { ...get().unread, [friend]: 0 } });
    try {
      const [messages, online] = await Promise.all([
        invoke<DmMessage[]>("dm_messages", { friend }),
        invoke<boolean>("dm_online", { friend }),
      ]);
      for (const m of messages) seen.add(m.id);
      set({ messages: { ...get().messages, [friend]: messages }, online: { ...get().online, [friend]: online } });
      // Fotos pequenas que chegaram enquanto a conversa estava fechada.
      for (const m of messages) if (autoDownloads(m)) void get().download(friend, m);
    } catch (e) {
      toast.error(String(e), "Mensagens");
    }
  },

  async sendText(friend, text) {
    try {
      await invoke("dm_send_text", { friend, text });
      return true;
    } catch (e) {
      toast.warning(String(e), "Mensagens");
      return false;
    }
  },

  async sendFiles(friend, paths) {
    if (paths.length === 0) return;
    const choice = get().copyChoice;
    if (choice === "ask") {
      set({ pendingFiles: { friend, paths } });
      return;
    }
    await sendAll(friend, paths, choice === "keep");
  },

  async resolveCopy(keep, remember) {
    const pending = get().pendingFiles;
    set({ pendingFiles: null });
    // Fechou o modal sem escolher: nada é mandado.
    if (!pending || keep === null) return;
    if (remember) get().setCopyChoice(keep ? "keep" : "send");
    await sendAll(pending.friend, pending.paths, keep);
  },

  setCopyChoice(choice) {
    set({ copyChoice: choice });
    try {
      localStorage.setItem(COPY_KEY, choice);
    } catch {
      // Sem armazenamento: vale até fechar o app.
    }
  },

  async cancelDownload(friend, message) {
    set({ downloading: { ...get().downloading, [message.id]: false } });
    await invoke("dm_cancel_download", { friend, id: message.id }).catch(() => {});
  },

  async download(friend, message, dest) {
    if (get().downloading[message.id]) return;
    set({ downloading: { ...get().downloading, [message.id]: true } });
    try {
      await invoke("dm_download", { friend, id: message.id, dest: dest ?? null });
    } catch (e) {
      set({ downloading: { ...get().downloading, [message.id]: false } });
      toast.warning(String(e), "Anexo");
    }
  },
}));

async function sendAll(friend: string, paths: string[], keepCopy: boolean) {
  for (const path of paths) {
    try {
      await invoke("dm_send_file", { friend, path, keepCopy });
    } catch (e) {
      toast.warning(String(e), "Anexo");
    }
  }
}

/** Uma mensagem nova ou que mudou: entra na conversa aberta e no resumo da lista. */
/** Ids de mensagens que já passaram por aqui (para não contar uma atualização como nova). */
const seen = new Set<string>();

function onMessage(friend: string, message: DmMessage) {
  const s = useDmStore.getState();
  const list = s.messages[friend];
  const messages = list
    ? {
        ...s.messages,
        [friend]: list.some((m) => m.id === message.id)
          ? list.map((m) => (m.id === message.id ? message : m))
          : [...list, message],
      }
    : s.messages;
  // Nova de verdade só na primeira vez: depois são atualizações dela (entregue, progresso de um
  // anexo). Pela lista não dá para saber com a conversa fechada (ela só carrega ao abrir).
  const isNew = !seen.has(message.id) && !list?.some((m) => m.id === message.id);
  seen.add(message.id);
  const others = s.conversations.filter((c) => c.friend !== friend);
  const current = s.conversations.find((c) => c.friend === friend);
  const last = !current?.last || message.at >= current.last.at ? message : current.last;
  const pending = (messages[friend] ?? [message]).filter((m) => m.mine && m.status === "pending").length;
  const unread =
    isNew && !message.mine && s.open !== friend ? { ...s.unread, [friend]: (s.unread[friend] ?? 0) + 1 } : s.unread;
  const downloading =
    message.file?.path && s.downloading[message.id] ? { ...s.downloading, [message.id]: false } : s.downloading;
  useDmStore.setState({
    messages,
    conversations: [{ friend, last, pending }, ...others].sort((a, b) => (b.last?.at ?? 0) - (a.last?.at ?? 0)),
    unread,
    downloading,
  });
  if (isNew && autoDownloads(message)) void useDmStore.getState().download(friend, message);
  // Mensagem nova de alguém: toca o aviso, menos se a conversa com ele está aberta e na frente.
  if (isNew && !message.mine && !(s.open === friend && document.hasFocus())) sound("message");
}

/** Liga os eventos das conversas ao store. Chamar uma vez, ao abrir o app. */
export async function listenToDmEvents() {
  const off = await listen<DmEvent>("dm-event", (e) => {
    const ev = e.payload;
    switch (ev.type) {
      case "message":
        onMessage(ev.friend, ev.message);
        return;
      case "online":
        useDmStore.setState({ online: { ...useDmStore.getState().online, [ev.friend]: ev.online } });
        return;
      case "unreachable":
        useDmStore.setState({ online: { ...useDmStore.getState().online, [ev.friend]: false } });
    }
  });
  // Entrou com o Discord (ou o app abriu já logado): carrega a lista; saiu: limpa.
  const offFriends = useFriendsStore.subscribe((s, prev) => {
    if (s.status === "online" && prev.status !== "online") void useDmStore.getState().load();
    // Um amigo ficou online: o que estava esperando por ele vai agora (sem precisar mandar outra).
    for (const f of s.friends) {
      const was = prev.friends.find((p) => p.id === f.id)?.presence.online ?? false;
      if (f.presence.online && !was) retryPending(f.id);
    }
    if (s.status === "signed-out" && prev.status !== "signed-out") {
      seen.clear();
      useDmStore.setState({ conversations: [], messages: {}, open: null, online: {}, unread: {} });
    }
  });
  if (useFriendsStore.getState().status === "online") await useDmStore.getState().load();
  // E de tempos em tempos, enquanto houver algo esperando um amigo online sem conexão aberta
  // (a primeira tentativa pode ter falhado, ou ele abriu o app agora).
  const timer = setInterval(() => {
    const { conversations, online } = useDmStore.getState();
    const friends = useFriendsStore.getState().friends;
    for (const c of conversations) {
      const friendOnline = friends.find((f) => f.id === c.friend)?.presence.online;
      if (c.pending > 0 && friendOnline && !online[c.friend]) retryPending(c.friend);
    }
  }, RETRY_EVERY_MS);
  return () => {
    off();
    offFriends();
    clearInterval(timer);
  };
}

/** De quanto em quanto tentar de novo o que está esperando um amigo online. */
export const RETRY_EVERY_MS = 15_000;

/** Pede ao Rust para tentar mandar de novo o que está pendente para o amigo. */
function retryPending(friend: string) {
  if (!useDmStore.getState().conversations.some((c) => c.friend === friend && c.pending > 0)) return;
  invoke("dm_retry", { friend }).catch(() => {});
}

/** Tamanho para gente ler. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / 1024 / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} GB`;
}
