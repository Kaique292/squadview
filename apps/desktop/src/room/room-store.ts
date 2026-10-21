import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { avatarUrl, type DiscordProfile, displayName, useAccountStore } from "../account/account-store";
import type { StreamStats } from "../stream/stream-store";
import { toast } from "../ui/toast-store";
import { useSpeakingEffectStore } from "../voice/speaking-effect";
import { toRust, useVoiceStore } from "../voice/voice-store";

/** Espelha `sv_net::Member`. */
export interface Member {
  id: string;
  name: string;
  live: boolean;
  /** Microfone silenciado (chat de voz). Ausente em apps antigos. */
  muted?: boolean;
  /** Ensurdecido (não ouve ninguém). */
  deafened?: boolean;
  /** Conta do Discord vinculada (só id e hash da foto). Ausente em apps antigos. */
  discord?: { id: string; avatar: string | null } | null;
  /** A conta foi conferida pelo SquadView (entrou com o Discord nesta versão): pode virar amigo. */
  verified?: boolean;
  /** Quem este membro está assistindo (id). Ausente em apps até o 0.4.0. */
  watching?: string | null;
  /** Efeito de fala escolhido (ADR 0015). Ausente em apps até o 0.5.0; use `useMemberEffect`. */
  effect?: unknown;
}

/** O que vai para a sala da nossa conta do Discord: só o id e o hash da foto. */
function discordOf(profile: DiscordProfile | null) {
  return profile ? { id: profile.id, avatar: profile.avatar } : null;
}

/**
 * Foto de um membro: a nossa vem do perfil salvo (muda na hora ao vincular); a dos outros, da
 * conta do Discord que eles vincularam. Sem conta, `undefined` (a inicial colorida).
 */
export function memberPhoto(member: Member, isMe: boolean, profile: DiscordProfile | null, size = 128) {
  if (isMe) return profile ? avatarUrl(profile, size) : undefined;
  return member.discord ? avatarUrl(member.discord, size) : undefined;
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
  /** A ligação com a sala caiu e o app está tentando voltar. */
  reconnecting: boolean;
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
  reconnecting: false,
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
      // A voz liga junto com a sala, com os dispositivos, volumes, mudo e surdo salvos.
      const voice = toRust(useVoiceStore.getState().prefs);
      const info = await invoke<{ code: string; roomName?: string | null }>("room_join", {
        code,
        name,
        discord: discordOf(useAccountStore.getState().profile),
        roomName,
        voice,
        effect: useSpeakingEffectStore.getState().effect,
      });
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
    listen<"reconnecting" | "connected" | "lost">("room-connection", (e) => {
      if (e.payload === "reconnecting") {
        useRoomStore.setState({ reconnecting: true });
        toast.warning("A conexão com a sala caiu. Tentando voltar…", "Reconectando");
      } else if (e.payload === "connected") {
        useRoomStore.setState({ reconnecting: false });
        toast.success("De volta à sala.", "Reconectado");
      } else {
        useRoomStore.setState({
          code: null,
          roomName: null,
          reconnecting: false,
          joinedAt: null,
          members: [],
          watching: null,
          watchStats: null,
          pendingWatch: null,
        });
        toast.error("Não deu para voltar para a sala. Entre de novo pelo código ou pelo link.", "Conexão perdida");
      }
    }),
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
  // Vinculou ou desvinculou o Discord já na sala: a galera vê o nome e a foto novos na hora.
  const offAccount = useAccountStore.subscribe((s, prev) => {
    if (s.profile === prev.profile || !useRoomStore.getState().code) return;
    const name = s.profile ? displayName(s.profile) : (useRoomStore.getState().members[0]?.name ?? savedName());
    if (!name) return;
    if (s.profile) saveName(name);
    void invoke("room_set_profile", { name, discord: discordOf(s.profile) }).catch(() => {});
  });
  // Convite que abriu o app.
  await takePendingInvite();
  return () => {
    offs.forEach((off) => off());
    offAccount();
  };
}

/** Sala para voltar depois de atualizar (o app fecha para instalar). */
const REJOIN_KEY = "squadview.voltar-para-sala";
/** Depois disso, a volta para a sala já não faz sentido (a atualização travou ou demorou). */
export const REJOIN_WITHIN_MS = 10 * 60 * 1000;

/** Guarda a sala atual para voltar depois de atualizar. */
export function rememberRoomToRejoin(now = Date.now()) {
  const code = useRoomStore.getState().code;
  if (!code) return;
  try {
    localStorage.setItem(REJOIN_KEY, JSON.stringify({ code, at: now }));
  } catch {
    // Sem armazenamento: só não volta sozinho.
  }
}

/** A atualização não aconteceu: não há para onde voltar. */
export function forgetRoomToRejoin() {
  try {
    localStorage.removeItem(REJOIN_KEY);
  } catch {
    // Nada a apagar.
  }
}

/** A sala de antes da atualização, se ela foi há pouco (vale uma vez só). */
export function takeRoomToRejoin(now = Date.now()): string | null {
  try {
    const raw = localStorage.getItem(REJOIN_KEY);
    localStorage.removeItem(REJOIN_KEY);
    const saved = raw ? (JSON.parse(raw) as { code: string; at: number }) : null;
    return saved && now - saved.at < REJOIN_WITHIN_MS ? saved.code : null;
  } catch {
    return null;
  }
}

/** Depois de uma atualização feita numa sala, entra de novo nela com o nosso nome de sempre. */
export async function rejoinAfterUpdate() {
  const code = takeRoomToRejoin();
  if (!code || useRoomStore.getState().code) return;
  const profile = useAccountStore.getState().profile;
  const name = profile ? displayName(profile) : savedName() || "Você";
  if (await useRoomStore.getState().join(code, name)) {
    toast.success("Você voltou para a sala depois da atualização.", "Atualizado");
  }
}

/** Pega o convite guardado pelo Rust (se houver) e pede confirmação para entrar. */
async function takePendingInvite() {
  const code = await invoke<string | null>("take_pending_invite").catch(() => null);
  if (!code) return;
  // Convite de servidor (`servidor:<convite>`): abre o "Entrar num servidor" já preenchido.
  if (code.startsWith("servidor:")) useServerInviteStore.setState({ invite: code.slice("servidor:".length) });
  else useRoomStore.getState().setInvite(code);
}

/** Convite de servidor recebido por link, esperando a pessoa confirmar. */
export const useServerInviteStore = create<{ invite: string | null }>(() => ({ invite: null }));
