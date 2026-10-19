import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import type { StreamStats } from "../stream/stream-store";
import { toast } from "../ui/toast-store";

/** Espelha `sv_net::Member`. */
export interface Member {
  id: string;
  name: string;
  live: boolean;
}

const NAME_KEY = "squadview.nome";
const VOLUME_KEY = "squadview.volume";
/** Volume máximo de quem assistimos, em % (como no Discord). */
export const MAX_WATCH_VOLUME = 200;

function savedVolume(): number {
  try {
    const v = Number(localStorage.getItem(VOLUME_KEY));
    return localStorage.getItem(VOLUME_KEY) === null || !Number.isFinite(v)
      ? 100
      : Math.min(MAX_WATCH_VOLUME, Math.max(0, Math.round(v)));
  } catch {
    return 100;
  }
}

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

/** Nome que aparece para a sala: o que deram para ela ou, sem nome, o código. */
export function roomTitle(code: string, name: string | null | undefined): string {
  return name?.trim() || `Sala ${code}`;
}

/** Até 3 iniciais do nome da sala, para o ícone na barra de salas ("Galera do LoL" → "GDL"). */
export function roomInitials(code: string, name: string | null | undefined): string {
  const words = name?.trim().split(/\s+/).filter(Boolean) ?? [];
  if (words.length === 0) return code.slice(0, 3);
  if (words.length === 1) return [...words[0]].slice(0, 3).join("").toUpperCase();
  return words
    .slice(0, 3)
    .map((w) => [...w][0])
    .join("")
    .toUpperCase();
}

interface RoomState {
  code: string | null;
  /** Nome da sala (`null`: sem nome, aparece o código). */
  roomName: string | null;
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
  /** Volume de quem assistimos, de 0 a 200 (%). Fica salvo neste PC. */
  watchVolume: number;
  setWatchVolume: (volume: number) => Promise<void>;
  /** Código de um convite por link, esperando a pessoa confirmar a entrada. */
  invite: string | null;
  setInvite: (code: string | null) => void;
  join: (code: string | null, name: string, roomName?: string | null) => Promise<boolean>;
  renameRoom: (name: string) => Promise<void>;
  setNickname: (name: string) => Promise<void>;
  leave: () => Promise<void>;
  watch: (member: Member) => Promise<void>;
  stopWatching: () => Promise<void>;
  setWatchListening: (listening: boolean) => Promise<void>;
  clearError: () => void;
}

export const useRoomStore = create<RoomState>((set, get) => ({
  code: null,
  roomName: null,
  invite: null,
  watchVolume: savedVolume(),
  joinedAt: null,
  members: [],
  joining: false,
  joinError: null,
  pendingWatch: null,
  watching: null,
  watchStats: null,
  watchListening: true,

  async renameRoom(name) {
    try {
      await invoke("room_rename", { name });
    } catch (e) {
      toast.error(String(e), "Nome da sala");
    }
  },

  async setNickname(name) {
    try {
      await invoke("room_set_name", { name });
      saveName(name.trim());
    } catch (e) {
      toast.error(String(e), "Apelido");
    }
  },

  async join(code, name, roomName = null) {
    set({ joining: true, joinError: null });
    try {
      const info = await invoke<{ code: string; roomName?: string | null }>("room_join", { code, name, roomName });
      saveName(name.trim());
      set({
        code: info.code,
        roomName: info.roomName ?? null,
        joinedAt: Date.now(),
        joining: false,
        watching: null,
        watchStats: null,
      });
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
    set({
      code: null,
      roomName: null,
      joinedAt: null,
      members: [],
      watching: null,
      watchStats: null,
      pendingWatch: null,
    });
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
    await invoke("room_stop_watching").catch(() => {});
    set({ watching: null, watchStats: null });
  },

  async setWatchVolume(volume) {
    const v = Math.min(MAX_WATCH_VOLUME, Math.max(0, Math.round(volume)));
    set({ watchVolume: v });
    try {
      localStorage.setItem(VOLUME_KEY, String(v));
    } catch {
      // Sem armazenamento: vale só nesta sessão.
    }
    await invoke("set_watch_volume", { volume: v / 100 }).catch(() => {});
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
    listen<string | null>("room-name", (e) => useRoomStore.setState({ roomName: e.payload })),
    listen<Member[]>("room-members", (e) => {
      const members = e.payload;
      const { watching } = useRoomStore.getState();
      // Quem estávamos assistindo saiu da sala: o vídeo some sozinho (evento watch-ended).
      useRoomStore.setState({ members, watching: watching && (members.find((m) => m.id === watching.id) ?? watching) });
    }),
    listen<string>("room-error", (e) => toast.error(e.payload, "Sala")),
    listen<Member>("watch-started", (e) => {
      useRoomStore.setState({ watching: e.payload, pendingWatch: null, watchStats: null, watchListening: true });
      // O volume escolhido vale para qualquer transmissão que a gente assistir.
      void invoke("set_watch_volume", { volume: useRoomStore.getState().watchVolume / 100 }).catch(() => {});
      toast.success(`Conectado direto com ${e.payload.name}.`, "Assistindo");
    }),
    listen<{ memberId: string; reason: string }>("watch-ended", (e) => {
      const { watching, pendingWatch } = useRoomStore.getState();
      const wasWatching = watching?.id === e.payload.memberId;
      const wasPending = pendingWatch === e.payload.memberId;
      if (!wasWatching && !wasPending) return;
      useRoomStore.setState({
        watching: wasWatching ? null : watching,
        watchStats: wasWatching ? null : useRoomStore.getState().watchStats,
        pendingWatch: null,
      });
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
