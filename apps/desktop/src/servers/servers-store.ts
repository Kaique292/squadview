import { create } from "zustand";
import { displayName, useAccountStore } from "../account/account-store";
import { useFriendsStore } from "../friends/friends-store";
import { serverInviteLink } from "../room/invite";
import { savedName, useRoomStore } from "../room/room-store";
import { toast } from "../ui/toast-store";

/** Servidores com salas fixas (ver docs/adr/0016-servidores-e-salas-fixas.md). */

export type Role = "owner" | "admin" | "member";

export interface ServerRoom {
  id: string;
  name: string;
  /** Código de sala comum: entrar na sala é entrar nele. */
  code: string;
}

export interface ServerMember {
  id: string;
  name: string;
  avatar: string | null;
  role: Role;
}

/** Espelha `ServerSnapshot` do Worker. */
export interface Server {
  id: string;
  name: string;
  /** Data URL de 96 px, ou `null` (as iniciais). */
  icon: string | null;
  rooms: ServerRoom[];
  members: ServerMember[];
}

/** Quem está em cada sala do servidor (id da sala → membros), espelha o Worker (ADR 0018). */
/** Quem está em cada sala. `muted`/`deafened` faltam num Worker antigo. */
export type ServerPresence = Record<string, { id: string; live: boolean; muted?: boolean; deafened?: boolean }[]>;

/** Pedido a um servidor (espelha `sv_net::friends::ServerOp`). */
export type ServerOp =
  | { kind: "update"; name?: string; icon?: string | null }
  | { kind: "delete" | "leave" | "invite" }
  | { kind: "room-create"; name: string }
  | { kind: "room-rename"; room: string; name: string }
  | { kind: "room-delete"; room: string }
  | { kind: "kick"; member: string }
  | { kind: "role"; member: string; role: "admin" | "member" };

/** Mensagens do registro sobre servidores. */
export type ServerMessage =
  | { type: "servers"; servers: Server[] }
  | { type: "server"; server: Server }
  | { type: "server-removed"; id: string }
  | { type: "server-invite"; server: string; invite: string }
  | { type: "server-presence"; server: string; presence: ServerPresence };

/** Erros do servidor, em texto para gente. */
export const SERVER_ERRORS: Record<string, string> = {
  "not-member": "Você não está mais nesse servidor.",
  "not-allowed": "Só o dono ou os administradores podem fazer isso.",
  "unknown-room": "Essa sala não existe mais.",
  "unknown-member": "Essa pessoa não está mais no servidor.",
  "invite-invalid": "Convite inválido ou vencido. Peça um novo.",
  "server-full": "O servidor está cheio (100 pessoas).",
  "owner-cannot-leave": "O dono não sai do servidor: apague o servidor se não quiser mais.",
};

/** Limite atingido ("too-many"), conforme o último pedido de servidor. */
const TOO_MANY: Record<string, string> = {
  create: "Você já está em 20 servidores. Saia de algum para criar outro.",
  join: "Você já está em 20 servidores. Saia de algum para entrar neste.",
  "room-create": "O servidor já tem 20 salas.",
  invite: "Muitos convites ativos. Espere alguns vencerem (7 dias).",
};

/** O último pedido de servidor, para dar o texto certo ao "too-many" que voltar. */
let lastRequest: string | null = null;

/** Texto do erro, se ele veio de um pedido de servidor (`null`: é dos amigos). */
export function serverErrorText(code: string): string | null {
  const request = lastRequest;
  lastRequest = null;
  if (SERVER_ERRORS[code]) return SERVER_ERRORS[code];
  return code === "too-many" && request ? (TOO_MANY[request] ?? null) : null;
}

/**
 * Link de convite para mandar à galera: um link web comum (clicável no Discord e no WhatsApp) que abre o app de
 * quem tem ou leva ao download de quem não tem. O `squadview://servidor/...` cru só funciona com o app instalado e
 * nem vira link na maioria dos chats.
 */
export function serverInviteUrl(invite: string): string {
  return serverInviteLink(invite);
}

/** Tira o convite de um link ou do texto colado (`<id>.<segredo>`). `null` se não achar. */
export function parseServerInvite(text: string): string | null {
  return /([0-9a-f]{16}\.[A-Za-z0-9]{12})/.exec(text.trim())?.[1] ?? null;
}

/** Iniciais do servidor, quando ele não tem ícone ("Turma do Bairro" → "TdB"). */
export function serverInitials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 1) return [...words[0]].slice(0, 2).join("").toUpperCase();
  return words
    .slice(0, 3)
    .map((w) => w[0])
    .join("");
}

interface ServersState {
  servers: Server[];
  /** Servidor aberto na lateral (`null`: amigos ou a sala avulsa). */
  selected: string | null;
  /** Último convite criado em cada servidor (link pronto para copiar). */
  invites: Record<string, string>;
  /** Quem está em cada sala de cada servidor, mesmo nas salas em que não estamos. */
  presence: Record<string, ServerPresence>;
  select: (id: string | null) => void;
  create: (name: string) => Promise<void>;
  join: (invite: string) => Promise<void>;
  act: (server: string, op: ServerOp) => Promise<void>;
  /** Entra na sala do servidor (com o nome da sala, como numa sala comum). */
  enterRoom: (server: Server, room: ServerRoom) => Promise<void>;
}

export const useServersStore = create<ServersState>((set) => ({
  servers: [],
  selected: null,
  invites: {},
  presence: {},

  select(id) {
    set({ selected: id });
    if (id) useFriendsStore.setState({ open: false });
  },

  async create(name) {
    lastRequest = "create";
    await useFriendsStore.getState().act({ type: "server-create", name });
  },

  async join(invite) {
    lastRequest = "join";
    await useFriendsStore.getState().act({ type: "server-join", invite });
  },

  async act(server, op) {
    lastRequest = op.kind;
    await useFriendsStore.getState().act({ type: "server-action", server, op });
  },

  async enterRoom(server, room) {
    if (useRoomStore.getState().code === room.code) return;
    const profile = useAccountStore.getState().profile;
    const name = profile ? displayName(profile) : savedName() || "Você";
    set({ selected: server.id });
    await useRoomStore.getState().join(room.code, name, room.name);
  },
}));

/** O servidor de uma sala (pelo código), para a lateral mostrar o servidor em vez da sala avulsa. */
export function serverOfRoom(servers: Server[], code: string | null): Server | null {
  if (!code) return null;
  return servers.find((s) => s.rooms.some((r) => r.code === code)) ?? null;
}

/** Nosso papel no servidor. */
export function myRole(server: Server, me: string | null | undefined): Role | null {
  return server.members.find((m) => m.id === me)?.role ?? null;
}

/** Trata as mensagens de servidor do registro. `false`: não é sobre servidores. */
export function onServerMessage(msg: { type: string }): boolean {
  const state = useServersStore.getState();
  const m = msg as ServerMessage;
  switch (m.type) {
    case "servers":
      useServersStore.setState({
        servers: m.servers,
        selected: m.servers.some((s) => s.id === state.selected) ? state.selected : null,
      });
      return true;
    case "server": {
      const exists = state.servers.some((s) => s.id === m.server.id);
      useServersStore.setState({
        servers: exists
          ? state.servers.map((s) => (s.id === m.server.id ? m.server : s))
          : [...state.servers, m.server],
      });
      // Acabamos de criar ou de entrar: o servidor abre na lateral.
      if (!exists) state.select(m.server.id);
      return true;
    }
    case "server-removed": {
      const gone = state.servers.find((s) => s.id === m.id);
      useServersStore.setState({
        servers: state.servers.filter((s) => s.id !== m.id),
        selected: state.selected === m.id ? null : state.selected,
      });
      if (gone) toast.info(`Você não está mais em ${gone.name}.`, "Servidor");
      return true;
    }
    case "server-presence":
      useServersStore.setState({ presence: { ...state.presence, [m.server]: m.presence } });
      return true;
    case "server-invite": {
      const link = serverInviteUrl(m.invite);
      useServersStore.setState({ invites: { ...state.invites, [m.server]: link } });
      void navigator.clipboard
        ?.writeText(link)
        .then(() => toast.success("Link do convite copiado. Vale por 7 dias.", "Convite"))
        .catch(() => {});
      return true;
    }
    default:
      return false;
  }
}

/** Saiu da conta: os servidores somem da tela (voltam ao entrar de novo). */
export function clearServers() {
  useServersStore.setState({ servers: [], selected: null, invites: {}, presence: {} });
}
