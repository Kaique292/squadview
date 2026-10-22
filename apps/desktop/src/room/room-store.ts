import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { avatarUrl, type DiscordProfile, displayName, useAccountStore } from "../account/account-store";
import { PIP_CONTROL_EVENT, PIP_STATE_EVENT, type PipControl, type PipState } from "../stream/pip";
import { HISTORY_SIZE, type StreamStats, useStreamStore } from "../stream/stream-store";
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
  /** Quem este membro está assistindo (id; a primeira, se forem várias). Ausente até o 0.4.0. */
  watching?: string | null;
  /** Todas as lives que este membro assiste (ADR 0019). Ausente em apps até o 0.8.0. */
  watchingAll?: string[];
  /** Efeito de fala escolhido (ADR 0015). Ausente em apps até o 0.5.0; use `useMemberEffect`. */
  effect?: unknown;
  /** Fliperama aberto por este membro (ADR 0023): o jogo, as vagas e quem joga. */
  arcade?: ArcadeInfo | null;
}

/** Espelha `sv_net::Arcade`. */
export interface ArcadeInfo {
  game: string | null;
  slots: number;
  players: string[];
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
/** Quantas lives dá para assistir ao mesmo tempo (o mesmo limite do Worker, ADR 0019). */
export const MAX_WATCHING = 4;

/** Uma live que estamos assistindo: quem transmite, as medições e o som dela. */
export interface Watch {
  member: Member;
  stats: StreamStats | null;
  /** As últimas medições, da mais antiga para a mais nova (os gráficos dos detalhes). */
  history: StreamStats[];
  listening: boolean;
  /** De 0 a 200 (%). */
  volume: number;
}

/** `member` assiste `streamerId`: pela lista nova (ADR 0019) ou pelo campo antigo (apps até o 0.8.0). */
export function watches(member: Member, streamerId: string): boolean {
  return member.watchingAll?.includes(streamerId) ?? member.watching === streamerId;
}

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
  pendingWatch: string[];
  /** As lives que estamos assistindo, por id de quem transmite (na ordem em que começaram). */
  watching: Record<string, Watch>;
  /** Volume de uma live, de 0 a 200 (%). O último escolhido fica salvo neste PC para as próximas. */
  setWatchVolume: (memberId: string, volume: number) => Promise<void>;
  /** Código de um convite por link, esperando a pessoa confirmar a entrada. */
  invite: string | null;
  setInvite: (code: string | null) => void;
  join: (code: string | null, name: string, roomName?: string | null) => Promise<boolean>;
  renameRoom: (name: string) => Promise<void>;
  setNickname: (name: string) => Promise<void>;
  leave: () => Promise<void>;
  watch: (member: Member) => Promise<void>;
  stopWatching: (memberId: string) => Promise<void>;
  setWatchListening: (memberId: string, listening: boolean) => Promise<void>;
  clearError: () => void;
}

export const useRoomStore = create<RoomState>((set, get) => ({
  code: null,
  roomName: null,
  reconnecting: false,
  invite: null,
  joinedAt: null,
  members: [],
  joining: false,
  joinError: null,
  pendingWatch: [],
  watching: {},

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
        watching: {},
        pendingWatch: [],
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
      watching: {},
      pendingWatch: [],
    });
  },

  async watch(member) {
    const { watching, pendingWatch } = get();
    if (watching[member.id] || pendingWatch.includes(member.id)) return;
    if (Object.keys(watching).length + pendingWatch.length >= MAX_WATCHING) {
      toast.warning(`Dá para assistir até ${MAX_WATCHING} lives ao mesmo tempo. Pare uma para assistir outra.`);
      return;
    }
    set({ pendingWatch: [...pendingWatch, member.id] });
    try {
      await invoke("room_watch", { memberId: member.id });
    } catch (e) {
      set({ pendingWatch: get().pendingWatch.filter((id) => id !== member.id) });
      toast.error(typeof e === "string" ? e : "Não deu para assistir.");
    }
  },

  async stopWatching(memberId) {
    leaveVideo(memberId);
    await invoke("room_stop_watching", { memberId }).catch(() => {});
    set({ watching: without(get().watching, memberId) });
  },

  async setWatchVolume(memberId, volume) {
    const v = Math.min(MAX_WATCH_VOLUME, Math.max(0, Math.round(volume)));
    const watch = get().watching[memberId];
    if (!watch) return;
    set({ watching: { ...get().watching, [memberId]: { ...watch, volume: v } } });
    syncPip(memberId);
    try {
      localStorage.setItem(VOLUME_KEY, String(v));
    } catch {
      // Sem armazenamento: vale só nesta sessão.
    }
    await invoke("set_watch_volume", { memberId, volume: v / 100 }).catch(() => {});
  },

  async setWatchListening(memberId, listening) {
    const update = (on: boolean) => {
      const watch = get().watching[memberId];
      if (watch) set({ watching: { ...get().watching, [memberId]: { ...watch, listening: on } } });
      syncPip(memberId);
    };
    update(listening);
    await invoke("set_watch_listening", { memberId, listening }).catch(() => update(!listening));
  },

  clearError() {
    set({ joinError: null });
  },
}));

/** `record` sem a chave `key`. */
function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));
}

/** A live de `memberId` fechou: se estava solta ou em tela cheia, a interface volta ao normal. */
function leaveVideo(memberId: string) {
  const stream = useStreamStore.getState();
  if (stream.pip === memberId) useStreamStore.setState({ pip: null });
  if (stream.fullscreenOf === memberId) void stream.setFullscreen(false);
}

/** O primeiro da lista somos nós. */
export function useMe() {
  return useRoomStore((s) => s.members[0] ?? null);
}

/** O som da live no picture in picture mudou: os controles da janelinha acompanham. */
function syncPip(memberId: string) {
  const watch = useRoomStore.getState().watching[memberId];
  if (!watch || useStreamStore.getState().pip !== memberId) return;
  const state: PipState = { memberId, listening: watch.listening, volume: watch.volume };
  void Promise.resolve()
    .then(() => emitTo("pip", PIP_STATE_EVENT, state))
    .catch(() => {});
}

/** Quanto a volta para a sala pode demorar antes de avisar da queda. */
export const RECONNECT_NOTICE_MS = 3_000;
/** O aviso de queda agendado, e se ele chegou a aparecer (só aí avisa da volta). */
const reconnectNotice: { timer?: ReturnType<typeof setTimeout>; shown: boolean } = { shown: false };

/** Liga os eventos da sala vindos do Rust ao store. Chamar uma vez, ao abrir o app. */
export async function listenToRoomEvents() {
  const offs = await Promise.all([
    // Os controles da janelinha do picture in picture mexem no som da live por aqui.
    listen<PipControl>(PIP_CONTROL_EVENT, (e) => {
      const { memberId, listening, volume } = e.payload;
      const room = useRoomStore.getState();
      if (volume !== undefined) void room.setWatchVolume(memberId, volume);
      if (listening !== undefined) void room.setWatchListening(memberId, listening);
    }),
    listen<string | null>("room-name", (e) => useRoomStore.setState({ roomName: e.payload })),
    listen<"reconnecting" | "connected" | "lost">("room-connection", (e) => {
      const warned = reconnectNotice.shown;
      clearTimeout(reconnectNotice.timer);
      reconnectNotice.shown = false;
      if (e.payload === "reconnecting") {
        useRoomStore.setState({ reconnecting: true });
        // Quase toda queda volta em ~1 s sem ninguém notar (ADR 0022): só avisa se demorar.
        reconnectNotice.timer = setTimeout(() => {
          reconnectNotice.shown = true;
          toast.warning("A conexão com a sala caiu. Tentando voltar…", "Reconectando");
        }, RECONNECT_NOTICE_MS);
      } else if (e.payload === "connected") {
        useRoomStore.setState({ reconnecting: false });
        if (warned) toast.success("De volta à sala.", "Reconectado");
      } else {
        useRoomStore.setState({
          code: null,
          roomName: null,
          reconnecting: false,
          joinedAt: null,
          members: [],
          watching: {},
          pendingWatch: [],
        });
        toast.error("Não deu para voltar para a sala. Entre de novo pelo código ou pelo link.", "Conexão perdida");
      }
    }),
    listen<Member[]>("room-members", (e) => {
      const members = e.payload;
      // Quem assistimos mudou de nome ou de foto: o quadro da live acompanha. Quem saiu da sala
      // some sozinho (evento watch-ended).
      const watching = Object.fromEntries(
        Object.entries(useRoomStore.getState().watching).map(([id, w]) => [
          id,
          { ...w, member: members.find((m) => m.id === id) ?? w.member },
        ]),
      );
      useRoomStore.setState({ members, watching });
    }),
    listen<string>("room-error", (e) => toast.error(e.payload, "Sala")),
    listen<Member>("watch-started", (e) => {
      const member = e.payload;
      const { watching, pendingWatch } = useRoomStore.getState();
      // O volume escolhido da última vez vale para a próxima live que a gente assistir.
      const volume = savedVolume();
      useRoomStore.setState({
        watching: { ...watching, [member.id]: { member, stats: null, history: [], listening: true, volume } },
        pendingWatch: pendingWatch.filter((id) => id !== member.id),
      });
      void invoke("set_watch_volume", { memberId: member.id, volume: volume / 100 }).catch(() => {});
      toast.success(`Conectado direto com ${member.name}.`, "Assistindo");
    }),
    listen<{ memberId: string; reason: string }>("watch-ended", (e) => {
      const { memberId, reason } = e.payload;
      const { watching, pendingWatch } = useRoomStore.getState();
      const wasWatching = memberId in watching;
      const wasPending = pendingWatch.includes(memberId);
      if (!wasWatching && !wasPending) return;
      useRoomStore.setState({
        watching: without(watching, memberId),
        pendingWatch: pendingWatch.filter((id) => id !== memberId),
      });
      if (wasWatching) leaveVideo(memberId);
      // Fim normal da transmissão é informação; não conseguir conectar é erro.
      if (wasWatching) toast.info(reason);
      else toast.error(reason, "Não deu para assistir");
    }),
    listen<{ memberId: string; stats: StreamStats }>("watch-stats", (e) => {
      const { memberId, stats } = e.payload;
      const all = useRoomStore.getState().watching;
      const watch = all[memberId];
      if (!watch) return;
      const history = [...watch.history, stats].slice(-HISTORY_SIZE);
      useRoomStore.setState({ watching: { ...all, [memberId]: { ...watch, stats, history } } });
    }),
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
