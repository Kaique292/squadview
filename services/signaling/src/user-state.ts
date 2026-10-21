import {
  byteLength,
  MAX_MESSAGE_BYTES,
  MAX_SIGNAL_PAYLOAD_BYTES,
  RATE_LIMIT_MESSAGES,
  RATE_LIMIT_WINDOW_MS,
} from "./protocol";
import { isValidRoomCode } from "./room-code";
import {
  cleanIcon,
  MAX_SERVERS_PER_USER,
  ROOM_ID,
  SERVER_ID,
  type ServerError,
  type ServerOp,
  type ServerPresence,
  type ServerResult,
  type ServerSnapshot,
} from "./server-state";
import { cleanAvatar, cleanProfileName, type Profile } from "./session";
import type { TurnCredentials } from "./turn";

/**
 * O registro de uma pessoa (ver docs/adr/0011-amigos-e-presenca.md): amigos, pedidos, recentes
 * e presença. Sem depender do runtime da Cloudflare: o Durable Object `User` liga isto ao
 * WebSocket, ao armazenamento e às chamadas para o registro dos outros.
 */

export const MAX_FRIENDS = 200;
export const MAX_RECENTS = 30;
export const MAX_PENDING = 50;

/** O que os amigos veem de alguém. */
export interface Presence {
  online: boolean;
  /** Está numa sala (o código nunca vai para os amigos). */
  inRoom: boolean;
  /** Nome da sala, se ela tiver um. */
  room: string | null;
  live: boolean;
}

export const OFFLINE: Presence = { online: false, inRoom: false, room: null, live: false };

export interface Friend extends Profile {
  /** Este amigo entra direto nas minhas salas, sem pedir. */
  direct: boolean;
}

/** Resposta a "quero entrar na sala do amigo". */
export type JoinResult =
  | { kind: "approved"; code: string; room: string | null; by: string }
  | { kind: "pending"; friend: string }
  | { kind: "denied"; by: string }
  | { kind: "not-in-room" }
  | { kind: "not-friend" };

export type UserClientMessage =
  | { type: "profile"; name: string; avatar: string | null }
  | { type: "status"; room: { code: string; name: string | null } | null; live: boolean }
  | { type: "friend-request"; id: string }
  | { type: "friend-accept"; id: string }
  | { type: "friend-decline"; id: string }
  | { type: "friend-cancel"; id: string }
  | { type: "unfriend"; id: string }
  | { type: "set-direct"; id: string; direct: boolean }
  | { type: "join"; id: string }
  | { type: "invite"; id: string }
  // Servidores (ADR 0016): o registro confere quem pede e repassa ao servidor.
  | { type: "server-create"; name: string }
  | { type: "server-join"; invite: string }
  | { type: "server-action"; server: string; op: ServerOp }
  // Mensagens privadas (ADR 0017): só o sinal para os dois PCs se conectarem; o conteúdo vai direto.
  | { type: "dm-signal"; to: string; payload: Record<string, unknown> }
  | { type: "turn-request" };

export interface UserSnapshot {
  me: Profile;
  friends: (Friend & { presence: Presence })[];
  incoming: Profile[];
  outgoing: Profile[];
  recents: Profile[];
}

export type UserServerMessage =
  | ({ type: "state" } & UserSnapshot)
  | { type: "friend-request"; from: Profile }
  | { type: "join-result"; result: JoinResult }
  | { type: "invited"; from: Profile; code: string; room: string | null }
  /** Sessão renovada, mandada a cada conexão (o app guarda no lugar da antiga). */
  | { type: "session"; session: string }
  /** Todos os nossos servidores, ao conectar. */
  | { type: "servers"; servers: ServerSnapshot[] }
  /** Um servidor nosso mudou (ou entramos nele). */
  | { type: "server"; server: ServerSnapshot }
  /** Saímos, fomos tirados ou o servidor foi apagado. */
  | { type: "server-removed"; id: string }
  /** Convite criado: `<id do servidor>.<segredo>`. */
  | { type: "server-invite"; server: string; invite: string }
  /** Sinal de um amigo para conectar a conversa direto (ADR 0017). */
  | { type: "dm-signal"; from: string; payload: Record<string, unknown> }
  /** O amigo não está online agora: a mensagem espera no nosso PC. */
  | { type: "dm-unreachable"; id: string }
  /** Quem está em cada sala de um servidor nosso (ADR 0018). */
  | { type: "server-presence"; server: string; presence: ServerPresence }
  /** Credenciais do repassador TURN, pedidas para a conversa direta. */
  | ({ type: "turn" } & TurnCredentials)
  | {
      type: "error";
      code: "invalid-message" | "rate-limited" | "unknown-person" | "too-many" | "not-in-room" | ServerError;
    };

export interface UserAttachment {
  status: { room: { code: string; name: string | null } | null; live: boolean };
  rateWindowStart: number;
  rateCount: number;
}

export interface UserConnection {
  readonly key: unknown;
  send(message: UserServerMessage): void;
  close(code: number, reason: string): void;
  getAttachment(): UserAttachment | null;
  setAttachment(attachment: UserAttachment): void;
}

/** Armazenamento do registro (no Durable Object, o KV síncrono do SQLite). */
export interface UserStore {
  get<T>(key: string): T | undefined;
  put(key: string, value: unknown): void;
}

/** Chamadas para os registros dos outros e para as salas. */
export interface Peers {
  incomingRequest(to: string, from: Profile): Promise<void>;
  requestCanceled(to: string, from: string): Promise<void>;
  friendAccepted(to: string, from: Profile): Promise<void>;
  unfriended(to: string, from: string): Promise<void>;
  presenceChanged(to: string, from: string, presence: Presence): Promise<void>;
  presenceOf(id: string): Promise<Presence>;
  requestJoin(friend: string, from: Profile): Promise<JoinResult>;
  invited(to: string, from: Profile, code: string, room: string | null): Promise<void>;
  /** Pede à sala `code` para mostrar o pedido aos membros. `false` se a sala está vazia. */
  askRoom(code: string, from: Profile): Promise<boolean>;
  /** Id novo para um servidor (16 hex). */
  newServerId(): string;
  serverCreate(server: string, name: string, owner: Profile): Promise<ServerSnapshot>;
  /** Um pedido ao servidor; ele mesmo avisa os outros membros (não quem pediu). */
  serverAct(server: string, actor: Profile, op: ServerOp): Promise<ServerResult>;
  serverSnapshot(server: string): Promise<ServerSnapshot | null>;
  /** Avisa o servidor que entramos numa sala dele (`code`), saímos (`null`) ou mudou o ao vivo. */
  serverSetPresence(server: string, member: string, code: string | null, live: boolean): Promise<void>;
  /** Quem está em cada sala do servidor agora. */
  serverPresence(server: string): Promise<ServerPresence>;
  /** Nome ou foto mudou: o servidor atualiza e avisa os outros. */
  serverRefresh(server: string, profile: Profile): Promise<void>;
  /** Entrega o sinal da conversa ao registro do amigo. `false` se ele não está online. */
  dmSignal(to: string, from: string, payload: Record<string, unknown>): Promise<boolean>;
  /** Credenciais do repassador TURN (`null` sem provedor configurado). */
  turn(): Promise<TurnCredentials | null>;
}

const DISCORD_ID = /^[0-9]{15,21}$/;
/** Convite de servidor: `<id do servidor>.<segredo>`. */
const INVITE = /^([0-9a-f]{16})\.([A-Za-z0-9]{12})$/;

/** Lê o pedido a um servidor que já existe (`null` se não seguir o contrato). */
function parseServerOp(op: unknown): ServerOp | null {
  if (!isPlainObject(op)) return null;
  switch (op.kind) {
    case "update": {
      const out: { kind: "update"; name?: string; icon?: string | null } = { kind: "update" };
      if (op.name !== undefined) {
        const name = cleanProfileName(op.name);
        if (name === null) return null;
        out.name = name;
      }
      if (op.icon !== undefined) {
        const icon = cleanIcon(op.icon);
        if (icon === undefined) return null;
        out.icon = icon;
      }
      return out.name === undefined && out.icon === undefined ? null : out;
    }
    case "delete":
    case "leave":
    case "invite":
      return { kind: op.kind };
    case "room-create": {
      const name = cleanProfileName(op.name);
      return name === null ? null : { kind: "room-create", name };
    }
    case "room-rename": {
      const name = cleanProfileName(op.name);
      return name === null || typeof op.room !== "string" || !ROOM_ID.test(op.room)
        ? null
        : { kind: "room-rename", room: op.room, name };
    }
    case "room-delete":
      return typeof op.room === "string" && ROOM_ID.test(op.room) ? { kind: "room-delete", room: op.room } : null;
    case "kick":
      return typeof op.member === "string" && DISCORD_ID.test(op.member) ? { kind: "kick", member: op.member } : null;
    case "role":
      return typeof op.member === "string" &&
        DISCORD_ID.test(op.member) &&
        (op.role === "admin" || op.role === "member")
        ? { kind: "role", member: op.member, role: op.role }
        : null;
    default:
      return null;
  }
}
const CLOSE_INVALID = 4400;
const CLOSE_RATE = 4429;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Lê uma mensagem do app. `null` se não seguir o contrato. */
export function parseUserMessage(raw: string): UserClientMessage | null {
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isPlainObject(data)) return null;
  switch (data.type) {
    case "profile": {
      const name = cleanProfileName(data.name);
      return name === null ? null : { type: "profile", name, avatar: cleanAvatar(data.avatar) };
    }
    case "status": {
      if (typeof data.live !== "boolean") return null;
      if (data.room === null) return { type: "status", room: null, live: data.live };
      if (!isPlainObject(data.room) || typeof data.room.code !== "string" || !isValidRoomCode(data.room.code))
        return null;
      const name = data.room.name === null || data.room.name === undefined ? null : cleanProfileName(data.room.name);
      return { type: "status", room: { code: data.room.code, name }, live: data.live };
    }
    case "friend-request":
    case "friend-accept":
    case "friend-decline":
    case "friend-cancel":
    case "unfriend":
    case "join":
    case "invite":
      return typeof data.id === "string" && DISCORD_ID.test(data.id) ? { type: data.type, id: data.id } : null;
    case "set-direct":
      return typeof data.id === "string" && DISCORD_ID.test(data.id) && typeof data.direct === "boolean"
        ? { type: "set-direct", id: data.id, direct: data.direct }
        : null;
    case "server-create": {
      const name = cleanProfileName(data.name);
      return name === null ? null : { type: "server-create", name };
    }
    case "server-join":
      return typeof data.invite === "string" && INVITE.test(data.invite)
        ? { type: "server-join", invite: data.invite }
        : null;
    case "server-action": {
      if (typeof data.server !== "string" || !SERVER_ID.test(data.server)) return null;
      const op = parseServerOp(data.op);
      return op ? { type: "server-action", server: data.server, op } : null;
    }
    case "dm-signal":
      // O mesmo limite do `signal` da sala: endereços e a impressão digital do certificado.
      return typeof data.to === "string" &&
        DISCORD_ID.test(data.to) &&
        isPlainObject(data.payload) &&
        byteLength(JSON.stringify(data.payload)) <= MAX_SIGNAL_PAYLOAD_BYTES
        ? { type: "dm-signal", to: data.to, payload: data.payload }
        : null;
    case "turn-request":
      return { type: "turn-request" };
    default:
      return null;
  }
}

export class UserState {
  constructor(
    private readonly id: string,
    private readonly connections: () => UserConnection[],
    private readonly store: UserStore,
    private readonly peers: Peers,
    private readonly now: () => number = Date.now,
  ) {}

  // --- Armazenamento --------------------------------------------------------------------------

  private profile(): Profile {
    return this.store.get<Profile>("profile") ?? { id: this.id, name: "Alguém", avatar: null };
  }
  private friends(): Record<string, Friend> {
    return this.store.get<Record<string, Friend>>("friends") ?? {};
  }
  private incoming(): Record<string, Profile> {
    return this.store.get<Record<string, Profile>>("incoming") ?? {};
  }
  private outgoing(): Record<string, Profile> {
    return this.store.get<Record<string, Profile>>("outgoing") ?? {};
  }
  private recents(): (Profile & { at: number })[] {
    return this.store.get<(Profile & { at: number })[]>("recents") ?? [];
  }
  private presences(): Record<string, Presence> {
    return this.store.get<Record<string, Presence>>("presence") ?? {};
  }
  /** Ids dos nossos servidores, na ordem em que entramos. */
  private servers(): string[] {
    return this.store.get<string[]>("servers") ?? [];
  }
  /** Código de sala → servidor nosso dono dela (montado com os retratos dos servidores). */
  private serverRooms(): Record<string, string> {
    return this.store.get<Record<string, string>>("serverRooms") ?? {};
  }
  /** Onde avisamos o servidor que estamos (para avisar a saída depois). */
  private reported(): { server: string; code: string; live: boolean } | null {
    return this.store.get<{ server: string; code: string; live: boolean } | null>("reported") ?? null;
  }

  /** Guarda (ou tira, com `rooms` vazio) as salas de um servidor no índice código → servidor. */
  private indexServer(server: string, rooms: { code: string }[]): void {
    this.update<Record<string, string>>("serverRooms", {}, (index) => {
      for (const [code, owner] of Object.entries(index)) if (owner === server) delete index[code];
      for (const r of rooms) index[r.code] = server;
    });
  }

  /**
   * Conta ao servidor da nossa sala onde estamos (ADR 0018): entrou, saiu, trocou de sala ou mudou
   * o ao vivo. Fora de uma sala de servidor nosso, avisa só a saída da anterior.
   */
  private async reportServerPresence(gone?: UserConnection): Promise<void> {
    // A conexão que acabou de fechar (`gone`) ainda pode aparecer na lista: não conta.
    const conns = this.connections().filter((c) => c.key !== gone?.key);
    const room = conns.map((c) => c.getAttachment()?.status.room).find((r) => r) ?? null;
    const live = conns.some((c) => c.getAttachment()?.status.live);
    const server = room ? this.serverRooms()[room.code] : undefined;
    const now = server && room ? { server, code: room.code, live } : null;
    const before = this.reported();
    if (before?.server === now?.server && before?.code === now?.code && before?.live === now?.live) return;
    this.store.put("reported", now);
    const calls: Promise<void>[] = [];
    if (before && before.server !== now?.server)
      calls.push(this.peers.serverSetPresence(before.server, this.id, null, false));
    if (now) calls.push(this.peers.serverSetPresence(now.server, this.id, now.code, now.live));
    await Promise.all(calls.map((c) => c.catch(() => {})));
  }

  private update<T>(key: string, fallback: T, change: (value: T) => void): void {
    const value = this.store.get<T>(key) ?? fallback;
    change(value);
    this.store.put(key, value);
  }

  // --- Conexões do app ------------------------------------------------------------------------

  /** O app conectou (sessão já conferida). */
  async open(conn: UserConnection): Promise<void> {
    const first = this.connections().filter((c) => c.key !== conn.key).length === 0;
    conn.setAttachment({ status: { room: null, live: false }, rateWindowStart: this.now(), rateCount: 0 });
    // Atualiza a presença dos amigos (pode ter mudado enquanto estávamos fora).
    const ids = Object.keys(this.friends());
    const fresh = await Promise.all(ids.map((id) => this.peers.presenceOf(id).catch(() => OFFLINE)));
    this.update<Record<string, Presence>>("presence", {}, (p) => {
      for (const key of Object.keys(p)) delete p[key];
      ids.forEach((id, i) => (p[id] = fresh[i]));
    });
    this.sendState(conn);
    await this.sendServers(conn);
    if (first) await this.announce();
  }

  /** A conexão caiu ou foi fechada. */
  async closed(conn: UserConnection): Promise<void> {
    const others = this.connections().filter((c) => c.key !== conn.key);
    if (others.length === 0) await this.announce(OFFLINE);
    else await this.announce();
    await this.reportServerPresence(conn);
  }

  /** Mensagem do app. */
  async message(conn: UserConnection, raw: string | ArrayBuffer): Promise<void> {
    const attachment = this.allow(conn);
    if (!attachment) return;
    if (typeof raw !== "string" || byteLength(raw) > MAX_MESSAGE_BYTES) return this.fail(conn, CLOSE_INVALID);
    const msg = parseUserMessage(raw);
    if (!msg) return this.fail(conn, CLOSE_INVALID);

    switch (msg.type) {
      case "profile": {
        const before = this.profile();
        const profile = { id: this.id, name: msg.name, avatar: msg.avatar };
        this.store.put("profile", profile);
        this.sendStateToAll();
        // Nome ou foto novos aparecem também nos servidores.
        if (before.name !== profile.name || before.avatar !== profile.avatar) {
          await Promise.all(this.servers().map((s) => this.peers.serverRefresh(s, profile).catch(() => {})));
        }
        return;
      }
      case "status": {
        conn.setAttachment({ ...attachment, status: { room: msg.room, live: msg.live } });
        await Promise.all([this.announce(), this.reportServerPresence()]);
        return;
      }
      case "friend-request":
        return this.sendFriendRequest(conn, msg.id);
      case "friend-accept":
        return this.acceptFriend(conn, msg.id);
      case "friend-decline": {
        if (!this.incoming()[msg.id]) return;
        this.update<Record<string, Profile>>("incoming", {}, (m) => delete m[msg.id]);
        await this.peers.requestCanceled(msg.id, this.id).catch(() => {});
        this.sendStateToAll();
        return;
      }
      case "friend-cancel": {
        if (!this.outgoing()[msg.id]) return;
        this.update<Record<string, Profile>>("outgoing", {}, (m) => delete m[msg.id]);
        await this.peers.requestCanceled(msg.id, this.id).catch(() => {});
        this.sendStateToAll();
        return;
      }
      case "unfriend": {
        if (!this.friends()[msg.id]) return;
        this.forget(msg.id);
        await this.peers.unfriended(msg.id, this.id).catch(() => {});
        this.sendStateToAll();
        return;
      }
      case "set-direct": {
        if (!this.friends()[msg.id]) return conn.send({ type: "error", code: "unknown-person" });
        this.update<Record<string, Friend>>("friends", {}, (f) => (f[msg.id].direct = msg.direct));
        this.sendStateToAll();
        return;
      }
      case "join": {
        if (!this.friends()[msg.id]) return conn.send({ type: "error", code: "unknown-person" });
        const result = await this.peers
          .requestJoin(msg.id, this.profile())
          .catch((): JoinResult => ({ kind: "not-in-room" }));
        conn.send({ type: "join-result", result });
        return;
      }
      case "invite": {
        if (!this.friends()[msg.id]) return conn.send({ type: "error", code: "unknown-person" });
        const room = this.currentRoom();
        if (!room) return conn.send({ type: "error", code: "not-in-room" });
        await this.peers.invited(msg.id, this.profile(), room.code, room.name).catch(() => {});
        return;
      }
      case "server-create": {
        if (this.servers().length >= MAX_SERVERS_PER_USER) return conn.send({ type: "error", code: "too-many" });
        const id = this.peers.newServerId();
        const server = await this.peers.serverCreate(id, msg.name, this.profile()).catch(() => null);
        if (!server) return conn.send({ type: "error", code: "not-allowed" });
        this.update<string[]>("servers", [], (list) => list.push(id));
        this.broadcast({ type: "server", server });
        return;
      }
      case "server-join": {
        const [, server, secret] = INVITE.exec(msg.invite)!;
        if (!this.servers().includes(server) && this.servers().length >= MAX_SERVERS_PER_USER) {
          return conn.send({ type: "error", code: "too-many" });
        }
        return this.serverAction(conn, server, { kind: "join", secret });
      }
      case "server-action":
        return this.serverAction(conn, msg.server, msg.op);
      case "dm-signal": {
        // Só entre amigos: quem não é amigo nem descobre se a pessoa está online.
        if (!this.friends()[msg.to]) return conn.send({ type: "error", code: "unknown-person" });
        const delivered = await this.peers.dmSignal(msg.to, this.id, msg.payload).catch(() => false);
        if (!delivered) conn.send({ type: "dm-unreachable", id: msg.to });
        return;
      }
      case "turn-request": {
        const turn = await this.peers.turn().catch(() => null);
        if (turn) conn.send({ type: "turn", ...turn });
        return;
      }
    }
  }

  // --- Chamadas vindas de outros registros e das salas ------------------------------------------

  /** Presença atual (para um amigo que acabou de conectar). */
  presence(): Presence {
    const conns = this.connections();
    if (conns.length === 0) return OFFLINE;
    const room = this.currentRoom();
    const live = conns.some((c) => c.getAttachment()?.status.live);
    return { online: true, inRoom: room !== null, room: room?.name ?? null, live };
  }

  /** Alguém dos recentes pediu amizade. */
  onIncomingRequest(from: Profile): void {
    if (from.id === this.id || this.friends()[from.id]) return;
    if (Object.keys(this.incoming()).length >= MAX_PENDING) return;
    // Os dois pediram ao mesmo tempo: já viram amigos.
    if (this.outgoing()[from.id]) {
      this.befriend(from);
      void this.peers.friendAccepted(from.id, this.profile()).catch(() => {});
      this.sendStateToAll();
      return;
    }
    this.update<Record<string, Profile>>("incoming", {}, (m) => (m[from.id] = from));
    this.broadcast({ type: "friend-request", from });
    this.sendStateToAll();
  }

  onRequestCanceled(from: string): void {
    this.update<Record<string, Profile>>("incoming", {}, (m) => delete m[from]);
    this.update<Record<string, Profile>>("outgoing", {}, (m) => delete m[from]);
    this.sendStateToAll();
  }

  /** Quem recebeu nosso pedido aceitou. */
  onFriendAccepted(from: Profile): void {
    if (!this.outgoing()[from.id] && !this.friends()[from.id]) return;
    this.befriend(from);
    this.sendStateToAll();
  }

  onUnfriended(from: string): void {
    this.forget(from);
    this.sendStateToAll();
  }

  onPresenceChanged(from: string, presence: Presence): void {
    if (!this.friends()[from]) return;
    this.update<Record<string, Presence>>("presence", {}, (p) => (p[from] = presence));
    this.sendStateToAll();
  }

  /**
   * Um amigo quer entrar na nossa sala. Se marcamos que ele entra direto, já vai o código;
   * senão o pedido aparece para quem está na sala.
   */
  async onRequestJoin(from: Profile): Promise<JoinResult> {
    const friend = this.friends()[from.id];
    if (!friend) return { kind: "not-friend" };
    const room = this.currentRoom();
    if (!room) return { kind: "not-in-room" };
    const me = this.profile();
    if (friend.direct) return { kind: "approved", code: room.code, room: room.name, by: me.name };
    // Sala de um servidor do qual nós dois somos membros: entra direto (ADR 0018).
    const server = this.serverRooms()[room.code];
    if (server) {
      const snapshot = await this.peers.serverSnapshot(server).catch(() => null);
      const member = (id: string) => !!snapshot?.members.some((m) => m.id === id);
      if (member(this.id) && member(from.id))
        return { kind: "approved", code: room.code, room: room.name, by: me.name };
    }
    const asked = await this.peers.askRoom(room.code, from).catch(() => false);
    return asked ? { kind: "pending", friend: me.name } : { kind: "not-in-room" };
  }

  /** A sala respondeu ao nosso pedido de entrada. */
  onJoinAnswered(result: JoinResult): void {
    this.broadcast({ type: "join-result", result });
  }

  /** Um amigo chamou para a sala dele. */
  onInvited(from: Profile, code: string, room: string | null): void {
    if (!this.friends()[from.id] || !isValidRoomCode(code)) return;
    this.broadcast({ type: "invited", from, code, room });
  }

  /** Um servidor nosso mudou (outro membro mexeu). Se não somos mais membros, sai da lista. */
  onServerChanged(server: ServerSnapshot): void {
    if (!server.members.some((m) => m.id === this.id)) return this.onServerRemoved(server.id);
    this.update<string[]>("servers", [], (list) => {
      if (!list.includes(server.id)) list.push(server.id);
    });
    this.indexServer(server.id, server.rooms);
    this.broadcast({ type: "server", server });
    // Uma sala nova pode ser a nossa (entramos nela antes de o índice saber dela).
    void this.reportServerPresence();
  }

  /** Alguém entrou ou saiu de uma sala de um servidor nosso. */
  onServerPresence(server: string, presence: ServerPresence): void {
    if (!this.servers().includes(server)) return;
    this.broadcast({ type: "server-presence", server, presence });
  }

  /**
   * Um amigo mandou o sinal da conversa direta. Vai para todas as nossas conexões (o app que
   * estiver aberto responde). `false` se não somos amigos ou não estamos online.
   */
  onDmSignal(from: string, payload: Record<string, unknown>): boolean {
    if (!this.friends()[from] || this.connections().length === 0) return false;
    this.broadcast({ type: "dm-signal", from, payload });
    return true;
  }

  /** Fomos tirados do servidor, ou ele foi apagado. */
  onServerRemoved(id: string): void {
    this.indexServer(id, []);
    if (!this.servers().includes(id)) return;
    this.update<string[]>("servers", [], (list) => list.splice(list.indexOf(id), 1));
    this.broadcast({ type: "server-removed", id });
  }

  /** Estivemos numa sala com `other` (os dois conferidos): entra nos recentes. */
  onMet(other: Profile): void {
    if (other.id === this.id) return;
    this.update<(Profile & { at: number })[]>("recents", [], (list) => {
      const i = list.findIndex((p) => p.id === other.id);
      if (i >= 0) list.splice(i, 1);
      list.unshift({ ...other, at: this.now() });
      list.length = Math.min(list.length, MAX_RECENTS);
    });
    // Mantém nome e foto dos amigos em dia.
    if (this.friends()[other.id]) {
      this.update<Record<string, Friend>>("friends", {}, (f) => (f[other.id] = { ...f[other.id], ...other }));
    }
    this.sendStateToAll();
  }

  // --- Interno ----------------------------------------------------------------------------------

  /** Repassa um pedido ao servidor e mostra o resultado em todas as nossas conexões. */
  private async serverAction(conn: UserConnection, server: string, op: ServerOp): Promise<void> {
    if (op.kind !== "join" && !this.servers().includes(server)) {
      return conn.send({ type: "error", code: "not-member" });
    }
    const result = await this.peers
      .serverAct(server, this.profile(), op)
      .catch((): ServerResult => ({ ok: false, error: op.kind === "join" ? "invite-invalid" : "not-member" }));
    if (!result.ok) return conn.send({ type: "error", code: result.error });
    if (result.invite) conn.send({ type: "server-invite", server, invite: `${server}.${result.invite}` });
    if (!result.snapshot || result.removed.includes(this.id)) return this.onServerRemoved(server);
    this.onServerChanged(result.snapshot);
    // Acabamos de entrar pelo convite: quem já está nas salas aparece na hora (antes, só ao
    // reconectar ou quando alguém mudava de sala).
    if (op.kind === "join") await this.sendServerPresence(server);
  }

  /** Manda a todas as nossas conexões quem está em cada sala do servidor agora. */
  private async sendServerPresence(server: string): Promise<void> {
    const presence = await this.peers.serverPresence(server).catch(() => null);
    if (presence) this.broadcast({ type: "server-presence", server, presence });
  }

  /** Manda todos os nossos servidores; os que acabaram (ou de onde saímos) saem da lista. */
  private async sendServers(conn: UserConnection): Promise<void> {
    const ids = this.servers();
    // `undefined`: não deu para perguntar agora (fica na lista para a próxima conexão).
    const found = await Promise.all(ids.map((id) => this.peers.serverSnapshot(id).catch(() => undefined)));
    const mine = (s: ServerSnapshot | null | undefined) => !!s && s.members.some((m) => m.id === this.id);
    const gone = new Set(ids.filter((_, i) => found[i] !== undefined && !mine(found[i])));
    if (gone.size)
      this.store.put(
        "servers",
        ids.filter((id) => !gone.has(id)),
      );
    const servers = found.filter((s): s is ServerSnapshot => mine(s));
    for (const s of servers) this.indexServer(s.id, s.rooms);
    conn.send({ type: "servers", servers });
    // E quem está em cada sala deles agora.
    const presences = await Promise.all(servers.map((s) => this.peers.serverPresence(s.id).catch(() => null)));
    servers.forEach((s, i) => {
      const presence = presences[i];
      if (presence) conn.send({ type: "server-presence", server: s.id, presence });
    });
  }

  private async sendFriendRequest(conn: UserConnection, id: string): Promise<void> {
    if (id === this.id || this.friends()[id]) return;
    const other = this.recents().find((p) => p.id === id);
    // Só quem esteve numa sala com a gente (conferido), para ninguém mandar pedido por id.
    if (!other) return conn.send({ type: "error", code: "unknown-person" });
    if (this.incoming()[id]) return this.acceptFriend(conn, id);
    if (Object.keys(this.outgoing()).length >= MAX_PENDING) return conn.send({ type: "error", code: "too-many" });
    const { at: _at, ...profile } = other;
    this.update<Record<string, Profile>>("outgoing", {}, (m) => (m[id] = profile));
    this.sendStateToAll();
    await this.peers.incomingRequest(id, this.profile()).catch(() => {});
  }

  private async acceptFriend(conn: UserConnection, id: string): Promise<void> {
    const from = this.incoming()[id];
    if (!from) return conn.send({ type: "error", code: "unknown-person" });
    if (Object.keys(this.friends()).length >= MAX_FRIENDS) return conn.send({ type: "error", code: "too-many" });
    this.befriend(from);
    this.sendStateToAll();
    await this.peers.friendAccepted(id, this.profile()).catch(() => {});
    // Já mostra se o novo amigo está online.
    const presence = await this.peers.presenceOf(id).catch(() => OFFLINE);
    this.onPresenceChanged(id, presence);
    await this.peers.presenceChanged(id, this.id, this.presence()).catch(() => {});
  }

  private befriend(p: Profile): void {
    this.update<Record<string, Friend>>("friends", {}, (f) => (f[p.id] = { ...p, direct: f[p.id]?.direct ?? false }));
    this.update<Record<string, Profile>>("incoming", {}, (m) => delete m[p.id]);
    this.update<Record<string, Profile>>("outgoing", {}, (m) => delete m[p.id]);
  }

  private forget(id: string): void {
    this.update<Record<string, Friend>>("friends", {}, (f) => delete f[id]);
    this.update<Record<string, Presence>>("presence", {}, (p) => delete p[id]);
  }

  private currentRoom(): { code: string; name: string | null } | null {
    for (const c of this.connections()) {
      const room = c.getAttachment()?.status.room;
      if (room) return room;
    }
    return null;
  }

  /** Conta a presença para os amigos (`presence` para forçar, como offline ao sair). */
  private async announce(presence: Presence = this.presence()): Promise<void> {
    const ids = Object.keys(this.friends());
    await Promise.all(ids.map((id) => this.peers.presenceChanged(id, this.id, presence).catch(() => {})));
  }

  snapshot(): UserSnapshot {
    const presences = this.presences();
    const friends = Object.values(this.friends())
      .map((f) => ({ ...f, presence: presences[f.id] ?? OFFLINE }))
      .sort((a, b) => Number(b.presence.online) - Number(a.presence.online) || a.name.localeCompare(b.name, "pt-BR"));
    const friendIds = new Set(friends.map((f) => f.id));
    return {
      me: this.profile(),
      friends,
      incoming: Object.values(this.incoming()),
      outgoing: Object.values(this.outgoing()),
      recents: this.recents()
        .filter((p) => !friendIds.has(p.id))
        .map(({ at: _at, ...p }) => p),
    };
  }

  private sendState(conn: UserConnection): void {
    conn.send({ type: "state", ...this.snapshot() });
  }

  private sendStateToAll(): void {
    for (const c of this.connections()) this.sendState(c);
  }

  private broadcast(message: UserServerMessage): void {
    for (const c of this.connections()) c.send(message);
  }

  private allow(conn: UserConnection): UserAttachment | null {
    const now = this.now();
    const a = conn.getAttachment() ?? { status: { room: null, live: false }, rateWindowStart: now, rateCount: 0 };
    const fresh = now - a.rateWindowStart >= RATE_LIMIT_WINDOW_MS;
    const next = { ...a, rateWindowStart: fresh ? now : a.rateWindowStart, rateCount: fresh ? 1 : a.rateCount + 1 };
    conn.setAttachment(next);
    if (next.rateCount > RATE_LIMIT_MESSAGES) {
      conn.send({ type: "error", code: "rate-limited" });
      conn.close(CLOSE_RATE, "rate-limited");
      return null;
    }
    return next;
  }

  private fail(conn: UserConnection, code: number): void {
    conn.send({ type: "error", code: "invalid-message" });
    conn.close(code, "invalid-message");
  }
}
