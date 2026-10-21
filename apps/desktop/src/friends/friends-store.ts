import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { displayName, useAccountStore } from "../account/account-store";
import { savedName, useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { clearServers, onServerMessage, type ServerOp, serverErrorText } from "../servers/servers-store";
import { toast } from "../ui/toast-store";

/** Amigos e presença (ver docs/adr/0011-amigos-e-presenca.md). */

export interface Person {
  id: string;
  name: string;
  avatar: string | null;
}

export interface Presence {
  online: boolean;
  inRoom: boolean;
  room: string | null;
  live: boolean;
}

export interface Friend extends Person {
  /** Entra direto nas minhas salas, sem pedir. */
  direct: boolean;
  presence: Presence;
}

/** Espelha a mensagem `state` do registro. */
interface Snapshot {
  me: Person | null;
  friends: Friend[];
  incoming: Person[];
  outgoing: Person[];
  recents: Person[];
}

type JoinResult =
  | { kind: "approved"; code: string; room: string | null; by: string }
  | { kind: "pending"; friend: string }
  | { kind: "denied"; by: string }
  | { kind: "not-in-room" }
  | { kind: "not-friend" };

/** Mensagens do registro que a interface usa. */
type RegistryMessage =
  | ({ type: "state" } & Snapshot)
  | { type: "friend-request"; from: Person }
  | { type: "join-result"; result: JoinResult }
  | { type: "invited"; from: Person; code: string; room: string | null }
  | { type: "error"; code: string };

/** O que o app pode pedir ao registro (espelha `sv_net::friends::FriendAction`). */
export type FriendAction =
  | { type: "profile"; name: string; avatar: string | null }
  | { type: "status"; room: { code: string; name: string | null } | null; live: boolean }
  | { type: "friend-request" | "friend-accept" | "friend-decline" | "friend-cancel" | "unfriend"; id: string }
  | { type: "set-direct"; id: string; direct: boolean }
  | { type: "join" | "invite"; id: string }
  // Servidores (ADR 0016).
  | { type: "server-create"; name: string }
  | { type: "server-join"; invite: string }
  | { type: "server-action"; server: string; op: ServerOp };

/** Pedido de alguém de fora para entrar na sala em que estamos. */
export interface JoinRequest {
  id: string;
  from: Person;
}

/** Convite de um amigo para a sala dele. */
export interface Invite {
  from: Person;
  code: string;
  room: string | null;
}

export type FriendsStatus = "signed-out" | "connecting" | "online";

interface FriendsState extends Snapshot {
  status: FriendsStatus;
  /** A página de amigos está aberta (no lugar da sala). */
  open: boolean;
  setOpen: (open: boolean) => void;
  joinRequests: JoinRequest[];
  invites: Invite[];
  /** Esperando alguém da sala desse amigo aceitar nosso pedido. */
  waitingFor: string | null;
  act: (action: FriendAction) => Promise<void>;
  joinFriend: (friend: Friend) => Promise<void>;
  answerJoin: (id: string, accept: boolean) => Promise<void>;
  acceptInvite: (invite: Invite) => Promise<void>;
  dismissInvite: (invite: Invite) => void;
}

const EMPTY: Snapshot = { me: null, friends: [], incoming: [], outgoing: [], recents: [] };

const ERRORS: Record<string, string> = {
  "unknown-person": "Só dá para adicionar quem já esteve numa sala com você, entrando com o Discord.",
  "too-many": "Muitos pedidos pendentes. Espere responderem.",
  "not-in-room": "Entre numa sala primeiro.",
};

export const useFriendsStore = create<FriendsState>((set, get) => ({
  ...EMPTY,
  status: "signed-out",
  open: false,
  joinRequests: [],
  invites: [],
  waitingFor: null,

  setOpen(open) {
    set({ open });
  },

  async act(action) {
    try {
      await invoke("friends_action", { action });
    } catch (e) {
      toast.error(String(e), "Amigos");
    }
  },

  async joinFriend(friend) {
    if (inMyRoom(friend.id)) {
      toast.info(`${friend.name} já está na sua sala.`, "Entrar na sala");
      return;
    }
    set({ waitingFor: friend.name });
    await get().act({ type: "join", id: friend.id });
  },

  async answerJoin(id, accept) {
    set({ joinRequests: get().joinRequests.filter((r) => r.id !== id) });
    try {
      await invoke("room_answer_join", { id, accept });
    } catch (e) {
      toast.error(String(e), "Pedido para entrar");
    }
  },

  async acceptInvite(invite) {
    get().dismissInvite(invite);
    await enterRoom(invite.code);
  },

  dismissInvite(invite) {
    set({ invites: get().invites.filter((i) => i !== invite) });
  },
}));

/** A pessoa (id do Discord) está na sala em que estamos. */
export function inMyRoom(id: string): boolean {
  const { code, members } = useRoomStore.getState();
  return code !== null && members.some((m) => m.discord?.id === id);
}

/** O mesmo, como hook (a lista da sala muda e o botão acompanha). */
export function useInMyRoom(id: string): boolean {
  return useRoomStore((s) => s.code !== null && s.members.some((m) => m.discord?.id === id));
}

/** Entra numa sala recebida de um amigo, com o nosso nome de sempre. */
async function enterRoom(code: string) {
  // Já estamos nela (ex.: aprovaram um pedido antigo): entrar de novo duplicava a gente na sala.
  if (code === useRoomStore.getState().code) {
    useFriendsStore.setState({ open: false });
    return;
  }
  const profile = useAccountStore.getState().profile;
  const name = profile ? displayName(profile) : savedName() || "Você";
  const ok = await useRoomStore.getState().join(code, name);
  if (ok) useFriendsStore.setState({ open: false });
}

function onJoinResult(result: JoinResult) {
  const waitingFor = useFriendsStore.getState().waitingFor;
  switch (result.kind) {
    case "pending":
      toast.info(`Pedido enviado. Quando alguém da sala de ${result.friend} aceitar, você entra.`, "Entrar na sala");
      return;
    case "approved":
      useFriendsStore.setState({ waitingFor: null });
      toast.success(`${result.by} deixou você entrar.`, "Entrando na sala");
      void enterRoom(result.code);
      return;
    case "denied":
      useFriendsStore.setState({ waitingFor: null });
      toast.warning(`${result.by} recusou o pedido.`, "Entrar na sala");
      return;
    case "not-in-room":
      useFriendsStore.setState({ waitingFor: null });
      toast.info(`${waitingFor ?? "Seu amigo"} não está numa sala agora.`, "Entrar na sala");
      return;
    case "not-friend":
      useFriendsStore.setState({ waitingFor: null });
      toast.warning("Vocês não são mais amigos.", "Entrar na sala");
  }
}

function onMessage(msg: RegistryMessage) {
  if (onServerMessage(msg)) return;
  switch (msg.type) {
    case "state": {
      useFriendsStore.setState({
        me: msg.me,
        friends: msg.friends,
        incoming: msg.incoming,
        outgoing: msg.outgoing,
        recents: msg.recents,
      });
      return;
    }
    case "friend-request":
      toast.info(`${msg.from.name} quer ser seu amigo. Veja em Amigos.`, "Pedido de amizade");
      return;
    case "join-result":
      onJoinResult(msg.result);
      return;
    case "invited":
      if (msg.code === useRoomStore.getState().code) return;
      useFriendsStore.setState({
        invites: [...useFriendsStore.getState().invites.filter((i) => i.from.id !== msg.from.id), msg],
      });
      return;
    case "error": {
      // Erro de um pedido a servidor (o último que fizemos): texto do servidor.
      const server = serverErrorText(msg.code);
      if (server) return toast.warning(server, "Servidor");
      toast.warning(ERRORS[msg.code] ?? "Não deu certo agora. Tente de novo.", "Amigos");
    }
  }
}

/** Conta ao registro quem somos (nome e foto) e onde estamos (sala e transmissão). */
function sendProfileAndStatus() {
  const { act, status } = useFriendsStore.getState();
  if (status !== "online") return;
  const profile = useAccountStore.getState().profile;
  if (profile) void act({ type: "profile", name: displayName(profile), avatar: profile.avatar });
  sendStatus();
}

function sendStatus() {
  const { act, status } = useFriendsStore.getState();
  if (status !== "online") return;
  const { code, roomName } = useRoomStore.getState();
  const live = useStreamStore.getState().status === "live";
  void act({ type: "status", room: code ? { code, name: roomName } : null, live });
}

/** Liga os eventos dos amigos vindos do Rust ao store. Chamar uma vez, ao abrir o app. */
export async function listenToFriendsEvents() {
  const offs = await Promise.all([
    listen<FriendsStatus>("friends-status", (e) => {
      useFriendsStore.setState({ status: e.payload });
      if (e.payload === "signed-out") {
        useFriendsStore.setState({ ...EMPTY });
        clearServers();
      }
      if (e.payload === "online") sendProfileAndStatus();
    }),
    listen<RegistryMessage>("friends-event", (e) => onMessage(e.payload)),
    listen<JoinRequest>("room-join-request", (e) => {
      const rest = useFriendsStore.getState().joinRequests.filter((r) => r.id !== e.payload.id);
      useFriendsStore.setState({ joinRequests: [...rest, e.payload] });
    }),
    listen<string>("room-join-request-closed", (e) => {
      useFriendsStore.setState({
        joinRequests: useFriendsStore.getState().joinRequests.filter((r) => r.id !== e.payload),
      });
    }),
  ]);
  // Entrou, saiu ou renomeou a sala, ou começou a transmitir: os amigos ficam sabendo.
  const offRoom = useRoomStore.subscribe((s, prev) => {
    if (s.code !== prev.code) useFriendsStore.setState({ joinRequests: [] });
    // Quem pediu para entrar e já está na sala (outro membro aceitou): o pedido some.
    else if (s.members !== prev.members) {
      const { joinRequests } = useFriendsStore.getState();
      const open = joinRequests.filter((r) => !inMyRoom(r.from.id));
      if (open.length !== joinRequests.length) useFriendsStore.setState({ joinRequests: open });
    }
    // Entrou numa sala ou começou a assistir alguém: sai da página de amigos para mostrar a sala
    // ou o vídeo (antes o vídeo ficava escondido atrás dela).
    const enteredRoom = s.code !== null && s.code !== prev.code;
    const startedWatching =
      Object.keys(s.watching).length > Object.keys(prev.watching).length ||
      s.pendingWatch.length > prev.pendingWatch.length;
    if ((enteredRoom || startedWatching) && useFriendsStore.getState().open) {
      useFriendsStore.setState({ open: false });
    }
    if (s.code !== prev.code || s.roomName !== prev.roomName) sendStatus();
  });
  const offStream = useStreamStore.subscribe((s, prev) => {
    if ((s.status === "live") !== (prev.status === "live")) sendStatus();
    // Começou a transmitir: mostra a própria transmissão.
    if (s.status === "live" && prev.status !== "live") useFriendsStore.setState({ open: false });
  });
  const offAccount = useAccountStore.subscribe((s, prev) => {
    if (s.profile !== prev.profile) sendProfileAndStatus();
  });
  const status = await invoke<FriendsStatus>("friends_status").catch((): FriendsStatus => "signed-out");
  useFriendsStore.setState({ status });
  if (status === "online") sendProfileAndStatus();
  return () => {
    offs.forEach((off) => off());
    offRoom();
    offStream();
    offAccount();
  };
}

/** Texto curto da presença, como o status embaixo do nome no Discord. */
export function presenceText(p: Presence): string {
  if (!p.online) return "Offline";
  if (p.live) return p.room ? `Transmitindo em ${p.room}` : "Transmitindo";
  if (p.inRoom) return p.room ? `Na sala ${p.room}` : "Numa sala";
  return "Online";
}
