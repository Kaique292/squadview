import { beforeEach, describe, expect, it } from "vitest";
import { type ServerSnapshot, ServerState } from "./server-state";
import type { Profile } from "./session";
import {
  type JoinResult,
  parseUserMessage,
  type Peers,
  type UserAttachment,
  type UserConnection,
  type UserServerMessage,
  UserState,
} from "./user-state";

const ANA = "111111111111111111";
const BETO = "222222222222222222";
const CAIO = "333333333333333333";

class FakeConn implements UserConnection {
  readonly key = this;
  sent: UserServerMessage[] = [];
  closedWith: number | null = null;
  private a: UserAttachment | null = null;
  send(m: UserServerMessage) {
    this.sent.push(m);
  }
  close(code: number) {
    this.closedWith = code;
  }
  getAttachment() {
    return this.a ? structuredClone(this.a) : null;
  }
  setAttachment(a: UserAttachment) {
    this.a = structuredClone(a);
  }
  state() {
    return this.sent.filter((m) => m.type === "state").at(-1) as Extract<UserServerMessage, { type: "state" }>;
  }
  last(type: UserServerMessage["type"]) {
    return this.sent.filter((m) => m.type === type).at(-1);
  }
}

/** Várias pessoas, com os registros falando entre si como os Durable Objects. */
class World {
  people = new Map<string, { state: UserState; conns: FakeConn[]; store: Map<string, unknown> }>();
  /** Salas que receberam pedido de entrada: código → quem pediu. */
  asked: { code: string; from: Profile }[] = [];
  roomIsEmpty = false;
  /** Servidores, como os Durable Objects `Server` (ids sorteados em ordem). */
  servers = new Map<string, ServerState>();
  private nextServer = 1;
  serverDown = false;
  /** Credenciais do repassador (`null`: sem provedor configurado). */
  turnCredentials: { urls: string[]; username: string; credential: string } | null = null;

  server(id: string) {
    let s = this.servers.get(id);
    if (!s) {
      const store = new Map<string, unknown>();
      let n = 0;
      s = new ServerState(
        {
          get: (k) => structuredClone(store.get(k)) as never,
          put: (k, v) => store.set(k, structuredClone(v)),
          delete: (k) => store.delete(k),
        },
        Date.now,
        {
          roomId: () => (++n).toString(16).padStart(8, "0"),
          // Códigos válidos de sala (sem L, I, O): o `status` do app recusa código inválido.
          roomCode: () => `SXK${"ABCDEFGHJKMNPQRSTUVWXYZ"[++n % 23]}22`,
          secret: () => `Convite${String(++n).padStart(5, "0")}`,
        },
      );
      this.servers.set(id, s);
    }
    return s;
  }

  /** O que o Durable Object `Server` faz depois de um pedido: avisa os outros membros. */
  private notify(id: string, except: string, snapshot: ServerSnapshot | null, removed: string[]) {
    for (const m of snapshot?.members ?? []) if (m.id !== except) this.person(m.id).state.onServerChanged(snapshot!);
    for (const r of removed) if (r !== except) this.person(r).state.onServerRemoved(id);
  }

  person(id: string) {
    let p = this.people.get(id);
    if (!p) {
      const store = new Map<string, unknown>();
      const conns: FakeConn[] = [];
      const state = new UserState(
        id,
        () => conns,
        { get: (k) => structuredClone(store.get(k)) as never, put: (k, v) => store.set(k, structuredClone(v)) },
        this.peers(),
      );
      p = { state, conns, store };
      this.people.set(id, p);
    }
    return p;
  }

  async connect(id: string, name: string) {
    const p = this.person(id);
    const conn = new FakeConn();
    p.conns.push(conn);
    await p.state.open(conn);
    await p.state.message(conn, JSON.stringify({ type: "profile", name, avatar: null }));
    return conn;
  }

  async disconnect(id: string, conn: FakeConn) {
    const p = this.person(id);
    await p.state.closed(conn);
    p.conns.splice(p.conns.indexOf(conn), 1);
  }

  peers(): Peers {
    return {
      incomingRequest: async (to, from) => this.person(to).state.onIncomingRequest(from),
      requestCanceled: async (to, from) => this.person(to).state.onRequestCanceled(from),
      friendAccepted: async (to, from) => this.person(to).state.onFriendAccepted(from),
      unfriended: async (to, from) => this.person(to).state.onUnfriended(from),
      presenceChanged: async (to, from, p) => this.person(to).state.onPresenceChanged(from, p),
      presenceOf: async (id) => this.person(id).state.presence(),
      requestJoin: async (friend, from) => this.person(friend).state.onRequestJoin(from),
      invited: async (to, from, code, room) => this.person(to).state.onInvited(from, code, room),
      askRoom: async (code, from) => {
        if (this.roomIsEmpty) return false;
        this.asked.push({ code, from });
        return true;
      },
      newServerId: () => (this.nextServer++).toString(16).padStart(16, "0"),
      serverCreate: async (id, name, owner) => this.server(id).create(id, name, owner),
      serverAct: async (id, actor, op) => {
        if (this.serverDown) throw new Error("fora do ar");
        const before = this.server(id).snapshot()?.id;
        const result = this.server(id).act(actor, op);
        if (result.ok && before) this.notify(before, actor.id, result.snapshot, result.removed);
        return result;
      },
      serverSnapshot: async (id) => {
        if (this.serverDown) throw new Error("fora do ar");
        return this.server(id).snapshot();
      },
      dmSignal: async (to, from, payload) => this.person(to).state.onDmSignal(from, payload),
      turn: async () => this.turnCredentials,
      serverSetPresence: async (id, member, code, live, voice) => {
        const srv = this.server(id);
        if (!srv.setPresence(member, code, live, voice)) return;
        const presence = srv.presence();
        for (const m of srv.memberIds()) this.person(m).state.onServerPresence(id, presence);
      },
      serverPresence: async (id) => this.server(id).presence(),
      serverRefresh: async (id, profile) => {
        const s = this.server(id);
        if (s.refreshProfile(profile)) this.notify(id, profile.id, s.snapshot(), []);
      },
    };
  }
}

async function send(world: World, id: string, conn: FakeConn, msg: unknown) {
  await world.person(id).state.message(conn, JSON.stringify(msg));
}

/** Ana e Beto estiveram na mesma sala e viraram amigos. */
async function friends(world: World) {
  const ana = await world.connect(ANA, "Ana");
  const beto = await world.connect(BETO, "Beto");
  world.person(ANA).state.onMet({ id: BETO, name: "Beto", avatar: null });
  world.person(BETO).state.onMet({ id: ANA, name: "Ana", avatar: null });
  await send(world, ANA, ana, { type: "friend-request", id: BETO });
  await send(world, BETO, beto, { type: "friend-accept", id: ANA });
  return { ana, beto };
}

let world: World;
beforeEach(() => {
  world = new World();
});

describe("mensagens do app", () => {
  it("lê as mensagens válidas e recusa o resto", () => {
    expect(parseUserMessage(JSON.stringify({ type: "friend-request", id: ANA }))).toEqual({
      type: "friend-request",
      id: ANA,
    });
    expect(
      parseUserMessage(JSON.stringify({ type: "status", room: { code: "ABCDEF", name: "Galera" }, live: true })),
    ).toEqual({ type: "status", room: { code: "ABCDEF", name: "Galera" }, live: true });
    expect(parseUserMessage(JSON.stringify({ type: "set-direct", id: ANA, direct: true }))).toMatchObject({
      direct: true,
    });
    for (const bad of [
      { type: "friend-request", id: "abc" },
      { type: "status", room: { code: "ab", name: null }, live: false },
      { type: "set-direct", id: ANA },
      { type: "profile", name: "" },
      { type: "outra" },
    ]) {
      expect(parseUserMessage(JSON.stringify(bad))).toBeNull();
    }
    expect(parseUserMessage("não é json")).toBeNull();
  });

  it("fecha a conexão com mensagem inválida", async () => {
    const ana = await world.connect(ANA, "Ana");
    await world.person(ANA).state.message(ana, "{}");
    expect(ana.closedWith).toBe(4400);
  });
});

describe("amizade", () => {
  it("só pede amizade a quem esteve numa sala junto (recentes)", async () => {
    const ana = await world.connect(ANA, "Ana");
    await send(world, ANA, ana, { type: "friend-request", id: BETO });
    expect(ana.last("error")).toEqual({ type: "error", code: "unknown-person" });
  });

  it("pede, o outro aceita e os dois viram amigos", async () => {
    const { ana, beto } = await friends(world);
    expect(ana.state().friends.map((f) => f.id)).toEqual([BETO]);
    expect(beto.state().friends.map((f) => f.id)).toEqual([ANA]);
    expect(beto.sent.some((m) => m.type === "friend-request")).toBe(true);
    // Amigo sai dos recentes.
    expect(ana.state().recents).toEqual([]);
  });

  it("recusar e cancelar tiram o pedido dos dois lados", async () => {
    const ana = await world.connect(ANA, "Ana");
    const beto = await world.connect(BETO, "Beto");
    world.person(ANA).state.onMet({ id: BETO, name: "Beto", avatar: null });
    await send(world, ANA, ana, { type: "friend-request", id: BETO });
    expect(beto.state().incoming.map((p) => p.id)).toEqual([ANA]);
    await send(world, BETO, beto, { type: "friend-decline", id: ANA });
    expect(beto.state().incoming).toEqual([]);
    expect(ana.state().outgoing).toEqual([]);
  });

  it("pedidos cruzados viram amizade", async () => {
    const ana = await world.connect(ANA, "Ana");
    const beto = await world.connect(BETO, "Beto");
    world.person(ANA).state.onMet({ id: BETO, name: "Beto", avatar: null });
    world.person(BETO).state.onMet({ id: ANA, name: "Ana", avatar: null });
    await send(world, ANA, ana, { type: "friend-request", id: BETO });
    await send(world, BETO, beto, { type: "friend-request", id: ANA });
    expect(ana.state().friends.map((f) => f.id)).toEqual([BETO]);
    expect(beto.state().friends.map((f) => f.id)).toEqual([ANA]);
  });

  it("desfazer a amizade tira dos dois lados", async () => {
    const { ana, beto } = await friends(world);
    await send(world, ANA, ana, { type: "unfriend", id: BETO });
    expect(ana.state().friends).toEqual([]);
    expect(beto.state().friends).toEqual([]);
  });

  it("guarda até 30 recentes, o mais novo primeiro, sem repetir", async () => {
    const ana = await world.connect(ANA, "Ana");
    for (let i = 0; i < 35; i++) {
      world.person(ANA).state.onMet({ id: String(400000000000000000n + BigInt(i)), name: `P${i}`, avatar: null });
    }
    world.person(ANA).state.onMet({ id: "400000000000000034", name: "P34", avatar: null });
    expect(ana.state().recents).toHaveLength(30);
    expect(ana.state().recents[0].name).toBe("P34");
  });
});

describe("presença", () => {
  it("os amigos veem online, a sala (sem o código) e offline", async () => {
    const { ana, beto } = await friends(world);
    expect(beto.state().friends[0].presence).toMatchObject({ online: true, inRoom: false });
    await send(world, ANA, ana, { type: "status", room: { code: "ABCDEF", name: "Galera" }, live: true });
    expect(beto.state().friends[0].presence).toEqual({ online: true, inRoom: true, room: "Galera", live: true });
    expect(JSON.stringify(beto.sent)).not.toContain("ABCDEF");
    await world.disconnect(ANA, ana);
    expect(beto.state().friends[0].presence.online).toBe(false);
  });

  it("quem conecta já vê quem está online", async () => {
    const { ana } = await friends(world);
    await world.disconnect(ANA, ana);
    const outra = await world.connect(ANA, "Ana");
    expect(outra.state().friends[0].presence.online).toBe(true);
  });
});

describe("entrar na sala de um amigo", () => {
  it("só entre amigos", async () => {
    const ana = await world.connect(ANA, "Ana");
    await send(world, ANA, ana, { type: "join", id: CAIO });
    expect(ana.last("error")).toEqual({ type: "error", code: "unknown-person" });
  });

  it("amigo fora de sala: avisa", async () => {
    const { beto } = await friends(world);
    await send(world, BETO, beto, { type: "join", id: ANA });
    expect(beto.last("join-result")).toEqual({ type: "join-result", result: { kind: "not-in-room" } });
  });

  it("o pedido vai para a sala, sem entregar o código", async () => {
    const { ana, beto } = await friends(world);
    await send(world, ANA, ana, { type: "status", room: { code: "ABCDEF", name: "Galera" }, live: false });
    await send(world, BETO, beto, { type: "join", id: ANA });
    expect(beto.last("join-result")).toEqual({ type: "join-result", result: { kind: "pending", friend: "Ana" } });
    expect(world.asked).toEqual([{ code: "ABCDEF", from: { id: BETO, name: "Beto", avatar: null } }]);
    // A sala respondeu: só aí o código chega.
    const approved: JoinResult = { kind: "approved", code: "ABCDEF", room: "Galera", by: "Caio" };
    world.person(BETO).state.onJoinAnswered(approved);
    expect(beto.last("join-result")).toEqual({ type: "join-result", result: approved });
  });

  it("quem entra direto recebe o código na hora", async () => {
    const { ana, beto } = await friends(world);
    await send(world, ANA, ana, { type: "set-direct", id: BETO, direct: true });
    await send(world, ANA, ana, { type: "status", room: { code: "ABCDEF", name: null }, live: false });
    await send(world, BETO, beto, { type: "join", id: ANA });
    expect(beto.last("join-result")).toEqual({
      type: "join-result",
      result: { kind: "approved", code: "ABCDEF", room: null, by: "Ana" },
    });
    expect(world.asked).toEqual([]);
  });

  it("convite de um amigo chega com o código; de quem não é amigo, não", async () => {
    const { ana, beto } = await friends(world);
    await send(world, ANA, ana, { type: "invite", id: BETO });
    expect(ana.last("error")).toEqual({ type: "error", code: "not-in-room" });
    await send(world, ANA, ana, { type: "status", room: { code: "ABCDEF", name: "Galera" }, live: false });
    await send(world, ANA, ana, { type: "invite", id: BETO });
    expect(beto.last("invited")).toMatchObject({ from: { id: ANA }, code: "ABCDEF", room: "Galera" });
    world.person(BETO).state.onInvited({ id: CAIO, name: "Caio", avatar: null }, "ABCDEF", null);
    expect(beto.sent.filter((m) => m.type === "invited")).toHaveLength(1);
  });
});

describe("servidores", () => {
  const S1 = "0000000000000001";
  type Msg<T extends UserServerMessage["type"]> = Extract<UserServerMessage, { type: T }>;
  const lastServer = (c: FakeConn) => (c.last("server") as Msg<"server">)?.server;
  const action = (server: string, op: unknown) => ({ type: "server-action", server, op });

  /** Ana cria um servidor e chama o Beto pelo convite. */
  async function anaComBeto() {
    const ana = await world.connect(ANA, "Ana");
    const beto = await world.connect(BETO, "Beto");
    await send(world, ANA, ana, { type: "server-create", name: "Turma do bairro" });
    await send(world, ANA, ana, action(S1, { kind: "invite" }));
    const { invite } = ana.last("server-invite") as Msg<"server-invite">;
    await send(world, BETO, beto, { type: "server-join", invite });
    return { ana, beto };
  }

  it("lê os pedidos de servidor e recusa os inválidos", () => {
    expect(parseUserMessage(JSON.stringify({ type: "server-create", name: " Turma " }))).toEqual({
      type: "server-create",
      name: "Turma",
    });
    expect(parseUserMessage(JSON.stringify(action(S1, { kind: "room-create", name: "Jogos" })))).toEqual({
      type: "server-action",
      server: S1,
      op: { kind: "room-create", name: "Jogos" },
    });
    const icon = "data:image/webp;base64,UklGRiQAAABXRUJQ";
    expect(parseUserMessage(JSON.stringify(action(S1, { kind: "update", icon })))).toMatchObject({
      op: { kind: "update", icon },
    });
    for (const bad of [
      { type: "server-create", name: "" },
      { type: "server-join", invite: "nao-e-convite" },
      action("xyz", { kind: "invite" }),
      action(S1, { kind: "update" }),
      action(S1, { kind: "update", icon: "javascript:alert(1)" }),
      action(S1, { kind: "update", icon: `data:image/png;base64,${"A".repeat(13000)}` }),
      action(S1, { kind: "role", member: BETO, role: "owner" }),
      action(S1, { kind: "room-delete", room: "sala" }),
      action(S1, { kind: "apagar-tudo" }),
    ]) {
      expect(parseUserMessage(JSON.stringify(bad)), JSON.stringify(bad)).toBeNull();
    }
  });

  it("cria o servidor com uma sala Geral, e quem cria é o dono", async () => {
    const ana = await world.connect(ANA, "Ana");
    expect(ana.last("servers")).toEqual({ type: "servers", servers: [] });
    await send(world, ANA, ana, { type: "server-create", name: "Turma do bairro" });
    const server = lastServer(ana);
    expect(server).toMatchObject({ id: S1, name: "Turma do bairro", icon: null });
    expect(server.rooms).toEqual([{ id: expect.any(String), name: "Geral", code: expect.any(String) }]);
    expect(server.members).toEqual([{ id: ANA, name: "Ana", avatar: null, role: "owner" }]);
  });

  it("convite traz gente nova, e todos os membros ficam sabendo", async () => {
    const { ana, beto } = await anaComBeto();
    expect(lastServer(beto).members.map((m) => [m.name, m.role])).toEqual([
      ["Ana", "owner"],
      ["Beto", "member"],
    ]);
    expect(lastServer(ana).members).toHaveLength(2);
    // Ao conectar de novo, o Beto já recebe o servidor.
    const outra = await world.connect(BETO, "Beto");
    expect((outra.last("servers") as Msg<"servers">).servers.map((s) => s.id)).toEqual([S1]);
  });

  it("convite errado ou vencido não deixa entrar", async () => {
    const ana = await world.connect(ANA, "Ana");
    const beto = await world.connect(BETO, "Beto");
    await send(world, ANA, ana, { type: "server-create", name: "Turma" });
    await send(world, BETO, beto, { type: "server-join", invite: `${S1}.ConviteErrad` });
    expect(beto.last("error")).toEqual({ type: "error", code: "invite-invalid" });
    expect(beto.last("server")).toBeUndefined();
  });

  it("só dono e administradores mexem no servidor; o dono promove", async () => {
    const { ana, beto } = await anaComBeto();
    await send(world, BETO, beto, action(S1, { kind: "room-create", name: "Jogos" }));
    expect(beto.last("error")).toEqual({ type: "error", code: "not-allowed" });

    await send(world, ANA, ana, action(S1, { kind: "role", member: BETO, role: "admin" }));
    expect(lastServer(beto).members.find((m) => m.id === BETO)?.role).toBe("admin");
    await send(world, BETO, beto, action(S1, { kind: "room-create", name: "Jogos" }));
    await send(world, BETO, beto, action(S1, { kind: "update", name: "Turma nova" }));
    expect(lastServer(ana)).toMatchObject({ name: "Turma nova" });
    expect(lastServer(ana).rooms.map((r) => r.name)).toEqual(["Geral", "Jogos"]);
    // Administrador não tira o dono nem promove ninguém.
    await send(world, BETO, beto, action(S1, { kind: "kick", member: ANA }));
    expect(beto.last("error")).toEqual({ type: "error", code: "not-allowed" });

    const jogos = lastServer(ana).rooms[1];
    await send(world, ANA, ana, action(S1, { kind: "room-rename", room: jogos.id, name: "Ranked" }));
    expect(lastServer(beto).rooms[1]).toEqual({ ...jogos, name: "Ranked" });
    await send(world, ANA, ana, action(S1, { kind: "room-delete", room: jogos.id }));
    expect(lastServer(beto).rooms).toHaveLength(1);
  });

  it("tirar alguém faz o servidor sumir da lista dele", async () => {
    const { ana, beto } = await anaComBeto();
    await send(world, ANA, ana, action(S1, { kind: "kick", member: BETO }));
    expect(beto.last("server-removed")).toEqual({ type: "server-removed", id: S1 });
    expect(lastServer(ana).members).toHaveLength(1);
    const outra = await world.connect(BETO, "Beto");
    expect(outra.last("servers")).toEqual({ type: "servers", servers: [] });
    // Fora do servidor, não dá para pedir nada a ele.
    await send(world, BETO, outra, action(S1, { kind: "invite" }));
    expect(outra.last("error")).toEqual({ type: "error", code: "not-member" });
  });

  it("membro sai sozinho; o dono não sai, apaga, e todos perdem o servidor", async () => {
    const { ana, beto } = await anaComBeto();
    await send(world, ANA, ana, action(S1, { kind: "leave" }));
    expect(ana.last("error")).toEqual({ type: "error", code: "owner-cannot-leave" });

    await send(world, ANA, ana, action(S1, { kind: "delete" }));
    expect(ana.last("server-removed")).toEqual({ type: "server-removed", id: S1 });
    expect(beto.last("server-removed")).toEqual({ type: "server-removed", id: S1 });
  });

  it("nome e foto novos aparecem para os outros membros", async () => {
    const { beto } = await anaComBeto();
    const ana = world.person(ANA).conns[0];
    await send(world, ANA, ana, { type: "profile", name: "Aninha", avatar: null });
    expect(lastServer(beto).members.find((m) => m.id === ANA)?.name).toBe("Aninha");
  });

  it("servidor fora do ar não derruba a conexão nem some da lista", async () => {
    const { beto } = await anaComBeto();
    world.serverDown = true;
    await send(world, BETO, beto, action(S1, { kind: "invite" }));
    expect(beto.last("error")).toEqual({ type: "error", code: "not-member" });
    expect(beto.closedWith).toBeNull();
    const outra = await world.connect(BETO, "Beto");
    expect(outra.last("servers")).toEqual({ type: "servers", servers: [] });
    world.serverDown = false;
    const depois = await world.connect(BETO, "Beto");
    expect((depois.last("servers") as Msg<"servers">).servers).toHaveLength(1);
  });

  it(`no máximo ${20} servidores por pessoa`, async () => {
    const ana = await world.connect(ANA, "Ana");
    for (let i = 0; i < 20; i++) await send(world, ANA, ana, { type: "server-create", name: `S${i}` });
    await send(world, ANA, ana, { type: "server-create", name: "Mais um" });
    expect(ana.last("error")).toEqual({ type: "error", code: "too-many" });
  });
});

describe("mensagens privadas: sinal da conexão direta", () => {
  type Msg<T extends UserServerMessage["type"]> = Extract<UserServerMessage, { type: T }>;
  const OFFER = { kind: "dm-offer", candidates: ["203.0.113.7:51000"], fingerprint: "ab".repeat(32) };

  it("lê o sinal e o pedido de TURN, e recusa o que não segue o contrato", () => {
    expect(parseUserMessage(JSON.stringify({ type: "dm-signal", to: BETO, payload: OFFER }))).toEqual({
      type: "dm-signal",
      to: BETO,
      payload: OFFER,
    });
    expect(parseUserMessage(JSON.stringify({ type: "turn-request" }))).toEqual({ type: "turn-request" });
    for (const bad of [
      { type: "dm-signal", to: "nao-e-id", payload: OFFER },
      { type: "dm-signal", to: BETO, payload: "texto" },
      { type: "dm-signal", to: BETO, payload: [1, 2] },
      { type: "dm-signal", to: BETO, payload: { grande: "x".repeat(9000) } },
    ]) {
      expect(parseUserMessage(JSON.stringify(bad)), JSON.stringify(bad).slice(0, 60)).toBeNull();
    }
  });

  it("entrega o sinal só ao amigo, em todas as conexões dele, com quem mandou", async () => {
    const { ana } = await friends(world);
    const outraDoBeto = await world.connect(BETO, "Beto");
    await send(world, ANA, ana, { type: "dm-signal", to: BETO, payload: OFFER });
    for (const conn of world.person(BETO).conns) {
      expect(conn.last("dm-signal")).toEqual({ type: "dm-signal", from: ANA, payload: OFFER });
    }
    expect(outraDoBeto.last("dm-signal")).toBeDefined();
    expect(ana.last("dm-unreachable")).toBeUndefined();
  });

  it("amigo offline: avisa quem mandou, sem guardar nada", async () => {
    const { ana, beto } = await friends(world);
    await world.disconnect(BETO, beto);
    await send(world, ANA, ana, { type: "dm-signal", to: BETO, payload: OFFER });
    expect(ana.last("dm-unreachable")).toEqual({ type: "dm-unreachable", id: BETO });
    // Ao voltar, o Beto não recebe sinal velho.
    const volta = await world.connect(BETO, "Beto");
    expect(volta.last("dm-signal")).toBeUndefined();
  });

  it("quem não é amigo não manda nem recebe sinal", async () => {
    const ana = await world.connect(ANA, "Ana");
    const caio = await world.connect(CAIO, "Caio");
    await send(world, ANA, ana, { type: "dm-signal", to: CAIO, payload: OFFER });
    expect(ana.last("error")).toEqual({ type: "error", code: "unknown-person" });
    expect(caio.last("dm-signal")).toBeUndefined();
    // Mesmo chegando por outro caminho, o registro do Caio recusa (não são amigos).
    expect(world.person(CAIO).state.onDmSignal(ANA, OFFER)).toBe(false);
  });

  it("registro do amigo fora do ar conta como offline", async () => {
    const { ana } = await friends(world);
    const peers = world.peers();
    const quebrado = new UserState(
      ANA,
      () => world.person(ANA).conns,
      { get: () => undefined, put: () => {} },
      { ...peers, dmSignal: async () => Promise.reject(new Error("fora do ar")) },
    );
    quebrado["friends"] = () => ({ [BETO]: { id: BETO, name: "Beto", avatar: null, direct: false } });
    await quebrado.message(ana, JSON.stringify({ type: "dm-signal", to: BETO, payload: OFFER }));
    expect(ana.last("dm-unreachable")).toEqual({ type: "dm-unreachable", id: BETO });
  });

  it("manda as credenciais do repassador quando pedidas (e nada sem provedor)", async () => {
    const ana = await world.connect(ANA, "Ana");
    await send(world, ANA, ana, { type: "turn-request" });
    expect(ana.last("turn")).toBeUndefined();
    world.turnCredentials = { urls: ["turn:turn.exemplo:3478"], username: "u", credential: "c" };
    await send(world, ANA, ana, { type: "turn-request" });
    expect(ana.last("turn") as Msg<"turn">).toEqual({
      type: "turn",
      urls: ["turn:turn.exemplo:3478"],
      username: "u",
      credential: "c",
    });
  });
});

describe("presença nas salas do servidor (ADR 0018)", () => {
  const S1 = "0000000000000001";
  type Msg<T extends UserServerMessage["type"]> = Extract<UserServerMessage, { type: T }>;
  const action = (server: string, op: unknown) => ({ type: "server-action", server, op });
  const presence = (c: FakeConn) => (c.last("server-presence") as Msg<"server-presence">)?.presence;

  /** Ana (dona) e Beto (membro) no mesmo servidor, e amigos. Devolve a sala Geral. */
  async function servidorComAmigos() {
    const { ana, beto } = await friends(world);
    await send(world, ANA, ana, { type: "server-create", name: "Turma do bairro" });
    await send(world, ANA, ana, action(S1, { kind: "invite" }));
    const { invite } = ana.last("server-invite") as Msg<"server-invite">;
    await send(world, BETO, beto, { type: "server-join", invite });
    const geral = (ana.last("server") as Msg<"server">).server.rooms[0];
    return { ana, beto, geral };
  }

  it("quem entra, sai ou fica ao vivo numa sala do servidor aparece para todos os membros", async () => {
    const { ana, beto, geral } = await servidorComAmigos();
    await send(world, BETO, beto, { type: "status", room: { code: geral.code, name: "Geral" }, live: false });
    // A Ana está fora da sala e mesmo assim vê o Beto lá dentro.
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: false, muted: false, deafened: false }] });
    await send(world, BETO, beto, { type: "status", room: { code: geral.code, name: "Geral" }, live: true });
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: true, muted: false, deafened: false }] });
    // Ao conectar de novo, já chega quem está em cada sala.
    const outra = await world.connect(ANA, "Ana");
    expect(presence(outra)).toEqual({ [geral.id]: [{ id: BETO, live: true, muted: false, deafened: false }] });
    await send(world, BETO, beto, { type: "status", room: null, live: false });
    expect(presence(ana)).toEqual({ [geral.id]: [] });
  });

  // Regressão: quem via a sala de fora não sabia que o amigo estava mudo ou silenciado.
  it("o mudo e o silenciado de quem está na sala aparecem para quem está fora", async () => {
    const { ana, beto, geral } = await servidorComAmigos();
    const room = { code: geral.code, name: "Geral" };
    await send(world, BETO, beto, { type: "status", room, live: false, muted: true, deafened: true });
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: false, muted: true, deafened: true }] });
    await send(world, BETO, beto, { type: "status", room, live: false, muted: true, deafened: false });
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: false, muted: true, deafened: false }] });
    // App até a 0.11.1 (sem os campos): conta como falando.
    await send(world, BETO, beto, { type: "status", room, live: false });
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: false, muted: false, deafened: false }] });
  });

  it("quem entra pelo convite já vê quem está nas salas, sem reconectar", async () => {
    const ana = await world.connect(ANA, "Ana");
    await send(world, ANA, ana, { type: "server-create", name: "Turma do bairro" });
    const geral = (ana.last("server") as Msg<"server">).server.rooms[0];
    await send(world, ANA, ana, { type: "status", room: { code: geral.code, name: "Geral" }, live: true });
    await send(world, ANA, ana, action(S1, { kind: "invite" }));
    const { invite } = ana.last("server-invite") as Msg<"server-invite">;
    const beto = await world.connect(BETO, "Beto");
    await send(world, BETO, beto, { type: "server-join", invite });
    // O retrato chega antes da presença (a lateral precisa do Beto na lista para mostrá-lo).
    const types = beto.sent.map((m) => m.type);
    expect(types.lastIndexOf("server")).toBeLessThan(types.lastIndexOf("server-presence"));
    expect(presence(beto)).toEqual({ [geral.id]: [{ id: ANA, live: true, muted: false, deafened: false }] });
  });

  it("fechar o app tira a pessoa da sala; sala que não é do servidor não aparece", async () => {
    const { ana, beto, geral } = await servidorComAmigos();
    await send(world, BETO, beto, { type: "status", room: { code: "ABCDEF", name: "Outra" }, live: false });
    expect(ana.last("server-presence")).toBeUndefined();
    await send(world, BETO, beto, { type: "status", room: { code: geral.code, name: "Geral" }, live: false });
    expect(presence(ana)).toEqual({ [geral.id]: [{ id: BETO, live: false, muted: false, deafened: false }] });
    await world.disconnect(BETO, beto);
    expect(presence(ana)).toEqual({ [geral.id]: [] });
  });

  it("o servidor só conta membros e salas que existem", () => {
    const s = world.server(S1);
    s.create(S1, "Turma", { id: ANA, name: "Ana", avatar: null });
    const geral = s.snapshot()!.rooms[0];
    expect(s.setPresence(CAIO, geral.code, false)).toBe(false);
    expect(s.setPresence(ANA, "ABCDEF", false)).toBe(false);
    expect(s.setPresence(ANA, geral.code, false)).toBe(true);
    // Repetir o mesmo não avisa de novo.
    expect(s.setPresence(ANA, geral.code, false)).toBe(false);
    expect(s.presence()).toEqual({ [geral.id]: [{ id: ANA, live: false, muted: false, deafened: false }] });
    expect(s.setPresence(ANA, null, false)).toBe(true);
    expect(s.setPresence(ANA, null, false)).toBe(false);
  });

  it("amigo membro do servidor entra direto na sala dele, sem pedir", async () => {
    const { ana, beto, geral } = await servidorComAmigos();
    await send(world, BETO, beto, { type: "status", room: { code: geral.code, name: "Geral" }, live: false });
    await send(world, ANA, ana, { type: "join", id: BETO });
    expect(ana.last("join-result")).toEqual({
      type: "join-result",
      result: { kind: "approved", code: geral.code, room: "Geral", by: "Beto" },
    });
    expect(world.asked).toEqual([]);
  });

  it("numa sala que não é de servidor, o amigo continua pedindo", async () => {
    const { ana, beto } = await servidorComAmigos();
    await send(world, BETO, beto, { type: "status", room: { code: "ABCDEF", name: "Outra" }, live: false });
    await send(world, ANA, ana, { type: "join", id: BETO });
    expect(ana.last("join-result")).toEqual({ type: "join-result", result: { kind: "pending", friend: "Beto" } });
  });

  it("quem sai do servidor some das salas dele e o índice esquece as salas", async () => {
    const { ana, beto, geral } = await servidorComAmigos();
    await send(world, BETO, beto, { type: "status", room: { code: geral.code, name: "Geral" }, live: false });
    await send(world, BETO, beto, action(S1, { kind: "leave" }));
    expect(world.server(S1).presence()).toEqual({ [geral.id]: [] });
    // Fora do servidor, pedir para entrar volta a pedir.
    await send(world, ANA, ana, { type: "join", id: BETO });
    expect(ana.last("join-result")).toMatchObject({ result: { kind: "pending" } });
  });
});
