import { DurableObject } from "cloudflare:workers";
import { generateRoomCode } from "./room-code";
import { type Attachment, type Connection, randomPeerId, RoomState, type VerifiedProfile } from "./room-state";
import { bearer, route } from "./router";
import { type ServerOp, type ServerResult, type ServerSnapshot, ServerState } from "./server-state";
import { exchangeDiscordToken, type Profile, signSession, verifySession } from "./session";
import { TurnCache, type TurnEnv, turnProvider } from "./turn";
import {
  type JoinResult,
  type Peers,
  type Presence,
  type UserAttachment,
  type UserConnection,
  UserState,
} from "./user-state";

interface Env extends TurnEnv {
  ROOMS: DurableObjectNamespace<Room>;
  USERS: DurableObjectNamespace<User>;
  SERVERS: DurableObjectNamespace<Server>;
  /** Segredo das sessões (`wrangler secret put SESSION_SECRET`). Sem ele, só as salas funcionam. */
  SESSION_SECRET?: string;
}

/** Cabeçalhos internos (só o Worker chama os Durable Objects). */
const ROOM_HEADER = "x-squadview-room";
const USER_HEADER = "x-squadview-user";

/**
 * Sinalização do SquadView: só ajuda os PCs a se encontrarem (nenhum vídeo passa por aqui).
 * As regras ficam em módulos puros e testados (router, room-code, protocol, room-state, session,
 * user-state); este arquivo só liga esses módulos ao runtime da Cloudflare.
 */
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const r = route(request.method, new URL(request.url).pathname);
    switch (r.kind) {
      case "create-room":
        return Response.json({ code: generateRoomCode() }, { status: 201 });
      case "room": {
        const forwarded = new Request(request);
        forwarded.headers.set(ROOM_HEADER, r.code);
        return env.ROOMS.get(env.ROOMS.idFromName(r.code)).fetch(forwarded);
      }
      case "auth-discord": {
        if (!env.SESSION_SECRET) return Response.json({ error: "sem-segredo" }, { status: 503 });
        const body = (await request.json().catch(() => null)) as { accessToken?: unknown } | null;
        const result = await exchangeDiscordToken(env.SESSION_SECRET, body?.accessToken);
        return result ? Response.json(result) : Response.json({ error: "token-recusado" }, { status: 401 });
      }
      case "me": {
        if (!env.SESSION_SECRET) return new Response("Sem segredo", { status: 503 });
        const id = await verifySession(env.SESSION_SECRET, bearer(request.headers.get("Authorization")));
        if (!id) return new Response("Sessão inválida", { status: 401 });
        const forwarded = new Request(request);
        forwarded.headers.set(USER_HEADER, id);
        return env.USERS.get(env.USERS.idFromName(id)).fetch(forwarded);
      }
      case "not-found":
        return new Response("Not found", { status: 404 });
    }
  },
};

/** Uma sala. Usa a Hibernation API: a sala dorme entre mensagens e não gasta tempo de CPU parada. */
export class Room extends DurableObject<Env> {
  private readonly state = new RoomState(() => this.ctx.getWebSockets().map(connection), randomPeerId, Date.now, {
    code: () => this.ctx.storage.kv.get<string>("code") ?? null,
    met: (a, b) => {
      this.ctx.waitUntil(
        user(this.env, a.id)
          .onMet(b)
          .catch(() => {}),
      );
      this.ctx.waitUntil(
        user(this.env, b.id)
          .onMet(a)
          .catch(() => {}),
      );
    },
    joinAnswered: (requester, result) => {
      this.ctx.waitUntil(
        user(this.env, requester)
          .onJoinAnswered(result)
          .catch(() => {}),
      );
    },
    getRequests: () => this.ctx.storage.kv.get("requests") ?? {},
    putRequests: (requests) => this.ctx.storage.kv.put("requests", requests),
  });

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("Use WebSocket", { status: 426 });
    }
    const code = request.headers.get(ROOM_HEADER);
    if (code) this.ctx.storage.kv.put("code", code);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    this.state.open(connection(server));
    return new Response(null, { status: 101, webSocket: client });
  }

  /** Credenciais do repassador, guardadas enquanto a sala está acordada. */
  private readonly turn = new TurnCache(async () => turnProvider(this.env)?.() ?? null);

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const conn = connection(ws);
    const wasIn = !!conn.getAttachment()?.member;
    this.state.message(conn, message, await this.verifiedJoin(message));
    // Acabou de entrar: manda o repassador (só quem é da sala recebe a credencial).
    if (!wasIn && conn.getAttachment()?.member) {
      const turn = await this.turn.get();
      if (turn) conn.send({ type: "turn", ...turn });
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    this.state.closed(connection(ws));
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    this.state.closed(connection(ws));
  }

  /** RPC: um amigo de alguém da sala pediu para entrar. */
  requestJoin(from: VerifiedProfile): boolean {
    return this.state.requestJoin(from, randomPeerId());
  }

  /** Se a mensagem é um `join` com sessão (ou um `verify`), confere a sessão e devolve o id do Discord. */
  private async verifiedJoin(message: string | ArrayBuffer): Promise<string | null> {
    if (typeof message !== "string" || !this.env.SESSION_SECRET || !message.includes('"session"')) return null;
    try {
      const data = JSON.parse(message) as { type?: unknown; session?: unknown };
      return data.type === "join" || data.type === "verify"
        ? await verifySession(this.env.SESSION_SECRET, data.session)
        : null;
    } catch {
      return null;
    }
  }
}

/** O registro de uma pessoa: amigos, pedidos, recentes e presença (ADR 0011). */
export class User extends DurableObject<Env> {
  private stateFor(id: string): UserState {
    return new UserState(
      id,
      () => this.ctx.getWebSockets().map(userConnection),
      { get: (key) => this.ctx.storage.kv.get(key), put: (key, value) => this.ctx.storage.kv.put(key, value) },
      peers(this.env),
    );
  }

  /** Id do Discord deste registro (o Worker manda na primeira conexão). */
  private get state(): UserState {
    return this.stateFor(this.ctx.storage.kv.get<string>("id") ?? "");
  }

  async fetch(request: Request): Promise<Response> {
    const id = request.headers.get(USER_HEADER);
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket" || !id) {
      return new Response("Use WebSocket", { status: 426 });
    }
    this.ctx.storage.kv.put("id", id);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    const conn = userConnection(server);
    await this.state.open(conn);
    // Sessão renovada a cada conexão: quem usa o app não precisa entrar de novo.
    if (this.env.SESSION_SECRET) {
      const session = await signSession(this.env.SESSION_SECRET, id);
      try {
        server.send(JSON.stringify({ type: "session", session }));
      } catch {
        // Já caiu.
      }
    }
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    await this.state.message(userConnection(ws), message);
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    await this.state.closed(userConnection(ws));
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    await this.state.closed(userConnection(ws));
  }

  // RPC vindo dos registros dos outros e das salas.
  presence(): Presence {
    return this.state.presence();
  }
  onIncomingRequest(from: Profile): void {
    this.state.onIncomingRequest(from);
  }
  onRequestCanceled(from: string): void {
    this.state.onRequestCanceled(from);
  }
  onFriendAccepted(from: Profile): void {
    this.state.onFriendAccepted(from);
  }
  onUnfriended(from: string): void {
    this.state.onUnfriended(from);
  }
  onPresenceChanged(from: string, presence: Presence): void {
    this.state.onPresenceChanged(from, presence);
  }
  onRequestJoin(from: Profile): Promise<JoinResult> {
    return this.state.onRequestJoin(from);
  }
  onJoinAnswered(result: JoinResult): void {
    this.state.onJoinAnswered(result);
  }
  onInvited(from: Profile, code: string, room: string | null): void {
    this.state.onInvited(from, code, room);
  }
  onMet(other: Profile): void {
    this.state.onMet(other);
  }
  onServerChanged(server: ServerSnapshot): void {
    this.state.onServerChanged(server);
  }
  onServerRemoved(id: string): void {
    this.state.onServerRemoved(id);
  }
}

/**
 * Um servidor (ADR 0016): nome, ícone, membros, salas e convites. Só é chamado pelo registro de
 * quem pede (que já conferiu a sessão) e avisa o registro dos outros membros quando algo muda.
 */
export class Server extends DurableObject<Env> {
  private get state(): ServerState {
    return new ServerState({
      get: (key) => this.ctx.storage.kv.get(key),
      put: (key, value) => this.ctx.storage.kv.put(key, value),
      delete: (key) => void this.ctx.storage.kv.delete(key),
    });
  }

  create(id: string, name: string, owner: Profile): ServerSnapshot {
    return this.state.create(id, name, owner);
  }

  snapshot(): ServerSnapshot | null {
    return this.state.snapshot();
  }

  act(actor: Profile, op: ServerOp): ServerResult {
    const id = this.state.snapshot()?.id;
    const result = this.state.act(actor, op);
    if (result.ok && id) this.notify(id, actor.id, result.snapshot, result.removed);
    return result;
  }

  refresh(profile: Profile): void {
    const snapshot = this.state.snapshot();
    if (snapshot && this.state.refreshProfile(profile)) this.notify(snapshot.id, profile.id, this.state.snapshot(), []);
  }

  /** Avisa os outros membros (quem pediu já recebe a resposta) sem segurar a resposta. */
  private notify(id: string, except: string, snapshot: ServerSnapshot | null, removed: string[]): void {
    for (const m of snapshot?.members ?? []) {
      if (m.id !== except)
        this.ctx.waitUntil(
          user(this.env, m.id)
            .onServerChanged(snapshot!)
            .catch(() => {}),
        );
    }
    for (const r of removed) {
      if (r !== except)
        this.ctx.waitUntil(
          user(this.env, r)
            .onServerRemoved(id)
            .catch(() => {}),
        );
    }
  }
}

function user(env: Env, id: string): DurableObjectStub<User> {
  return env.USERS.get(env.USERS.idFromName(id));
}

function server(env: Env, id: string): DurableObjectStub<Server> {
  return env.SERVERS.get(env.SERVERS.idFromName(id));
}

/** Id de servidor: 16 hex sorteados. */
function newServerId(): string {
  return [...crypto.getRandomValues(new Uint8Array(8))].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** As chamadas de um registro para os outros e para as salas. */
function peers(env: Env): Peers {
  return {
    incomingRequest: async (to, from) => user(env, to).onIncomingRequest(from),
    requestCanceled: async (to, from) => user(env, to).onRequestCanceled(from),
    friendAccepted: async (to, from) => user(env, to).onFriendAccepted(from),
    unfriended: async (to, from) => user(env, to).onUnfriended(from),
    presenceChanged: async (to, from, presence) => user(env, to).onPresenceChanged(from, presence),
    presenceOf: async (id) => user(env, id).presence(),
    requestJoin: async (friend, from) => user(env, friend).onRequestJoin(from),
    invited: async (to, from, code, room) => user(env, to).onInvited(from, code, room),
    askRoom: async (code, from) => env.ROOMS.get(env.ROOMS.idFromName(code)).requestJoin(from),
    newServerId,
    serverCreate: async (id, name, owner) => server(env, id).create(id, name, owner),
    serverAct: async (id, actor, op) => server(env, id).act(actor, op),
    serverSnapshot: async (id) => server(env, id).snapshot(),
    serverRefresh: async (id, profile) => server(env, id).refresh(profile),
  };
}

/** Adapta o WebSocket da Cloudflare à interface que a lógica da sala usa. */
function connection(ws: WebSocket): Connection {
  return {
    key: ws,
    send: (message) => {
      try {
        ws.send(JSON.stringify(message));
      } catch {
        // A conexão já caiu; o webSocketClose cuida da saída.
      }
    },
    close: (code, reason) => {
      try {
        ws.close(code, reason);
      } catch {
        // Já fechada.
      }
    },
    getAttachment: () => ws.deserializeAttachment() as Attachment | null,
    setAttachment: (a) => ws.serializeAttachment(a),
  };
}

/** O mesmo, para as conexões do registro de uma pessoa. */
function userConnection(ws: WebSocket): UserConnection {
  return {
    key: ws,
    send: (message) => {
      try {
        ws.send(JSON.stringify(message));
      } catch {
        // Já caiu.
      }
    },
    close: (code, reason) => {
      try {
        ws.close(code, reason);
      } catch {
        // Já fechada.
      }
    },
    getAttachment: () => ws.deserializeAttachment() as UserAttachment | null,
    setAttachment: (a) => ws.serializeAttachment(a),
  };
}
