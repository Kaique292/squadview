import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import type { StreamStats } from "../stream/stream-store";
import { playVideoExit } from "../ui/dissolve-effect";
import { toast } from "../ui/toast-store";

/** Espelha `sv_net::Member`. */
export interface Member {
  id: string;
  name: string;
  live: boolean;
}

const NAME_KEY = "squadview.nome";

/** Nome usado da última vez (só neste PC). */
export function savedName(): string {
  try {
    return localStorage.getItem(NAME_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveName(name: string) {
  try {
    localStorage.setItem(NAME_KEY, name);
  } catch {
    // Sem armazenamento local: só não lembra o nome da próxima vez.
  }
}

interface RoomState {
  code: string | null;
  /** Quando entramos na sala (ms), para o tempo conectado. */
  joinedAt: number | null;
  /** Membros da sala, nós primeiro. */
  members: Member[];
  joining: boolean;
  /** Erro ao criar/entrar (aparece no próprio modal). */
  joinError: string | null;
  /** Quem pedimos para assistir, esperando a conexão. */
  pendingWatch: string | null;
  watching: Member | null;
  watchStats: StreamStats | null;
  watchListening: boolean;
  /** Código de um convite por link, esperando a pessoa confirmar a entrada. */
  invite: string | null;
  setInvite: (code: string | null) => void;
  join: (code: string | null, name: string) => Promise<boolean>;
  leave: () => Promise<void>;
  watch: (member: Member) => Promise<void>;
  stopWatching: () => Promise<void>;
  setWatchListening: (listening: boolean) => Promise<void>;
  clearError: () => void;
}

export const useRoomStore = create<RoomState>((set, get) => ({
  code: null,
  invite: null,
  joinedAt: null,
  members: [],
  joining: false,
  joinError: null,
  pendingWatch: null,
  watching: null,
  watchStats: null,
  watchListening: true,

  async join(code, name) {
    set({ joining: true, joinError: null });
    try {
      const info = await invoke<{ code: string }>("room_join", { code, name });
      saveName(name.trim());
      set({ code: info.code, joinedAt: Date.now(), joining: false, watching: null, watchStats: null });
      toast.success(`Mande o código ${info.code} para a galera entrar.`, "Você está na sala");
      return true;
    } catch (e) {
      set({ joining: false, joinError: typeof e === "string" ? e : "Não deu para entrar na sala." });
      return false;
    }
  },

  setInvite(code) {
    if (code && code === get().code) {
      toast.info(`Você já está na sala ${code}.`);
      return;
    }
    set({ invite: code });
  },

  async leave() {
    await invoke("room_leave").catch(() => {});
    set({ code: null, joinedAt: null, members: [], watching: null, watchStats: null, pendingWatch: null });
  },

  async watch(member) {
    if (get().watching?.id === member.id) return;
    set({ pendingWatch: member.id });
    try {
      await invoke("room_watch", { memberId: member.id });
    } catch (e) {
      set({ pendingWatch: null });
      toast.error(typeof e === "string" ? e : "Não deu para assistir.");
    }
  },

  async stopWatching() {
    const photo = await invoke<string | null>("room_stop_watching").catch(() => null);
    set({ watching: null, watchStats: null });
    playVideoExit(photo);
  },

  async setWatchListening(listening) {
    set({ watchListening: listening });
    await invoke("set_watch_listening", { listening }).catch(() => set({ watchListening: !listening }));
  },

  clearError() {
    set({ joinError: null });
  },
}));

/** O primeiro da lista somos nós. */
export function useMe() {
  return useRoomStore((s) => s.members[0] ?? null);
}

/** Liga os eventos da sala vindos do Rust ao store. Chamar uma vez, ao abrir o app. */
export async function listenToRoomEvents() {
  const offs = await Promise.all([
    listen<Member[]>("room-members", (e) => {
      const members = e.payload;
      const { watching } = useRoomStore.getState();
      // Quem estávamos assistindo saiu da sala: o vídeo some sozinho (evento watch-ended).
      useRoomStore.setState({ members, watching: watching && (members.find((m) => m.id === watching.id) ?? watching) });
    }),
    listen<string>("room-error", (e) => toast.error(e.payload, "Sala")),
    listen<Member>("watch-started", (e) => {
      useRoomStore.setState({ watching: e.payload, pendingWatch: null, watchStats: null, watchListening: true });
      toast.success(`Conectado direto com ${e.payload.name}.`, "Assistindo");
    }),
    listen<{ memberId: string; reason: string; snapshot?: string | null }>("watch-ended", (e) => {
      const { watching, pendingWatch } = useRoomStore.getState();
      const wasWatching = watching?.id === e.payload.memberId;
      const wasPending = pendingWatch === e.payload.memberId;
      if (!wasWatching && !wasPending) return;
      useRoomStore.setState({
        watching: wasWatching ? null : watching,
        watchStats: wasWatching ? null : useRoomStore.getState().watchStats,
        pendingWatch: null,
      });
      // Quem transmitia parou: o vídeo some se desfazendo, como para quem transmitia.
      if (wasWatching) playVideoExit(e.payload.snapshot);
      // Fim normal da transmissão é informação; não conseguir conectar é erro.
      if (wasWatching) toast.info(e.payload.reason);
      else toast.error(e.payload.reason, "Não deu para assistir");
    }),
    listen<StreamStats>("watch-stats", (e) => useRoomStore.setState({ watchStats: e.payload })),
    // Convite clicado com o app já aberto.
    listen<string>("invite", () => void takePendingInvite()),
  ]);
  // Convite que abriu o app.
  await takePendingInvite();
  return () => offs.forEach((off) => off());
}

/** Pega o convite guardado pelo Rust (se houver) e pede confirmação para entrar. */
async function takePendingInvite() {
  const code = await invoke<string | null>("take_pending_invite").catch(() => null);
  if (code) useRoomStore.getState().setInvite(code);
}
