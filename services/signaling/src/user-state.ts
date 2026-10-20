import { byteLength, MAX_MESSAGE_BYTES, RATE_LIMIT_MESSAGES, RATE_LIMIT_WINDOW_MS } from "./protocol";
import { isValidRoomCode } from "./room-code";
import { cleanAvatar, cleanProfileName, type Profile } from "./session";

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
  | { type: "invite"; id: string };

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
  | { type: "error"; code: "invalid-message" | "rate-limited" | "unknown-person" | "too-many" | "not-in-room" };

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
}

const DISCORD_ID = /^[0-9]{15,21}$/;
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
    if (first) await this.announce();
  }

  /** A conexão caiu ou foi fechada. */
  async closed(conn: UserConnection): Promise<void> {
    const others = this.connections().filter((c) => c.key !== conn.key);
    if (others.length === 0) await this.announce(OFFLINE);
    else await this.announce();
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
        this.store.put("profile", { id: this.id, name: msg.name, avatar: msg.avatar });
        this.sendStateToAll();
        return;
      }
      case "status": {
        conn.setAttachment({ ...attachment, status: { room: msg.room, live: msg.live } });
        await this.announce();
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
