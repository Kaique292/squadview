import { beforeEach, describe, expect, it } from "vitest";
import {
  MAX_MEMBERS,
  MAX_MESSAGE_BYTES,
  RATE_LIMIT_MESSAGES,
  RATE_LIMIT_WINDOW_MS,
  type ServerMessage,
} from "./protocol";
import { type Attachment, CLOSE, type Connection, randomPeerId, RoomState } from "./room-state";

/** Estado de um "WebSocket" falso. */
class FakeConnection implements Connection {
  readonly key = this;
  sent: ServerMessage[] = [];
  closedWith: { code: number; reason: string } | null = null;
  private attachment: Attachment | null = null;

  send(message: ServerMessage) {
    this.sent.push(message);
  }
  close(code: number, reason: string) {
    this.closedWith = { code, reason };
  }
  getAttachment() {
    return this.attachment ? structuredClone(this.attachment) : null;
  }
  setAttachment(a: Attachment) {
    this.attachment = structuredClone(a);
  }
  last() {
    return this.sent.at(-1);
  }
}

let conns: FakeConnection[];
let now: number;
let ids: number;
let room: RoomState;

beforeEach(() => {
  conns = [];
  now = 0;
  ids = 0;
  room = new RoomState(
    () => conns.filter((c) => c.closedWith === null).map(wrap),
    () => (ids++).toString(16).padStart(16, "0"),
    () => now,
  );
});

/**
 * Como o adaptador do Durable Object, cria um objeto novo a cada chamada; só a `key` se repete.
 * Isso garante que a sala compara conexões pela identidade, e não pelo objeto.
 */
function wrap(c: FakeConnection): Connection {
  return {
    key: c,
    send: (m) => c.send(m),
    close: (code, reason) => c.close(code, reason),
    getAttachment: () => c.getAttachment(),
    setAttachment: (a) => c.setAttachment(a),
  };
}

function connect() {
  const c = new FakeConnection();
  conns.push(c);
  room.open(wrap(c));
  return c;
}

function send(c: FakeConnection, msg: unknown) {
  room.message(wrap(c), JSON.stringify(msg));
}

function join(name: string) {
  const c = connect();
  send(c, { type: "join", name, protocol: 1 });
  return c;
}

function idOf(c: FakeConnection) {
  const welcome = c.sent.find((m) => m.type === "welcome");
  if (welcome?.type !== "welcome") throw new Error("sem welcome");
  return welcome.you;
}

describe("entrar na sala", () => {
  it("recebe o próprio id e a lista de quem já está na sala", () => {
    const ana = join("Ana");
    expect(ana.last()).toEqual({ type: "welcome", you: idOf(ana), members: [], room: null });

    const beto = join("Beto");
    expect(beto.last()).toEqual({
      type: "welcome",
      you: idOf(beto),
      members: [
        { id: idOf(ana), name: "Ana", live: false, muted: false, deafened: false, discord: null, verified: false },
      ],
      room: null,
    });
  });

  it("avisa os outros quando alguém entra, mas não avisa a própria pessoa", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    expect(ana.last()).toEqual({
      type: "member-joined",
      member: {
        id: idOf(beto),
        name: "Beto",
        live: false,
        muted: false,
        deafened: false,
        discord: null,
        verified: false,
      },
    });
    expect(ana.sent.filter((m) => m.type === "member-joined")).toHaveLength(1);
    expect(beto.sent.filter((m) => m.type === "member-joined")).toHaveLength(0);
  });

  it("recusa versão diferente do protocolo e fecha a conexão", () => {
    const c = connect();
    send(c, { type: "join", name: "Ana", protocol: 2 });
    expect(c.last()).toEqual({ type: "error", code: "version-mismatch" });
    expect(c.closedWith?.code).toBe(CLOSE.versionMismatch);
  });

  it(`recusa o ${MAX_MEMBERS + 1}º membro`, () => {
    for (let i = 0; i < MAX_MEMBERS; i++) join(`m${i}`);
    const extra = join("Extra");
    expect(extra.last()).toEqual({ type: "error", code: "room-full" });
    expect(extra.closedWith?.code).toBe(CLOSE.roomFull);
  });

  it("não deixa entrar duas vezes pela mesma conexão", () => {
    const ana = join("Ana");
    send(ana, { type: "join", name: "Ana de novo", protocol: 1 });
    expect(ana.last()).toEqual({ type: "error", code: "invalid-message" });
    expect(ana.closedWith?.code).toBe(CLOSE.invalid);
  });

  it("gera outro id se o sorteado já estiver em uso", () => {
    let calls = 0;
    const repeated = new RoomState(
      () => conns.filter((c) => c.closedWith === null).map(wrap),
      () => (calls++ < 2 ? "aaaaaaaaaaaaaaaa" : "bbbbbbbbbbbbbbbb"),
      () => now,
    );
    room = repeated;
    const ana = join("Ana");
    const beto = join("Beto");
    expect(idOf(ana)).toBe("aaaaaaaaaaaaaaaa");
    expect(idOf(beto)).toBe("bbbbbbbbbbbbbbbb");
  });
});

describe("nome da sala e apelido", () => {
  function joinNaming(name: string, roomName: string) {
    const c = connect();
    send(c, { type: "join", name, protocol: 1, room: roomName });
    return c;
  }

  it("quem cria a sala escolhe o nome, e quem chega depois recebe esse nome", () => {
    const ana = joinNaming("Ana", "Galera do LoL");
    expect(ana.last()).toMatchObject({ type: "welcome", room: "Galera do LoL" });
    // Quem entra numa sala que já existe não troca o nome dela.
    const beto = joinNaming("Beto", "Outro nome");
    expect(beto.last()).toMatchObject({ type: "welcome", room: "Galera do LoL" });
    const caio = join("Caio");
    expect(caio.last()).toMatchObject({ type: "welcome", room: "Galera do LoL" });
  });

  it("qualquer membro renomeia a sala e todos ficam sabendo, inclusive quem chega depois", () => {
    const ana = joinNaming("Ana", "Sala 1");
    const beto = join("Beto");
    send(beto, { type: "room", name: "Squad" });
    expect(ana.last()).toEqual({ type: "room-updated", name: "Squad" });
    expect(beto.last()).toEqual({ type: "room-updated", name: "Squad" });
    expect(join("Caio").last()).toMatchObject({ type: "welcome", room: "Squad" });
  });

  it("troca o apelido e avisa os outros", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(beto, { type: "rename", name: "Betão" });
    expect(ana.last()).toEqual({
      type: "member-updated",
      member: {
        id: idOf(beto),
        name: "Betão",
        live: false,
        muted: false,
        deafened: false,
        discord: null,
        verified: false,
      },
    });
    expect(join("Caio").last()).toMatchObject({
      members: expect.arrayContaining([
        { id: idOf(beto), name: "Betão", live: false, muted: false, deafened: false, discord: null, verified: false },
      ]),
    });
  });

  it("mostra a conta do Discord de quem entra e de quem vincula depois", () => {
    const discord = { id: "123456789012345678", avatar: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" };
    const ana = connect();
    send(ana, { type: "join", name: "Ana", protocol: 1, discord });
    const beto = join("Beto");
    expect(beto.last()).toMatchObject({ members: [{ name: "Ana", discord }] });

    // Beto vincula o Discord já na sala: Ana fica sabendo.
    const dele = { id: "876543210987654321", avatar: null };
    send(beto, { type: "rename", name: "Betão", discord: dele });
    expect(ana.last()).toMatchObject({ type: "member-updated", member: { name: "Betão", discord: dele } });
    // Só o apelido: a conta continua.
    send(beto, { type: "rename", name: "Beto" });
    expect(ana.last()).toMatchObject({ member: { name: "Beto", discord: dele } });
    // Desvinculou.
    send(beto, { type: "rename", name: "Beto", discord: null });
    expect(ana.last()).toMatchObject({ member: { discord: null } });
  });

  it("a sala esvaziada esquece o nome", () => {
    const ana = joinNaming("Ana", "Sala antiga");
    send(ana, { type: "leave" });
    expect(join("Beto").last()).toMatchObject({ type: "welcome", room: null });
  });
});

describe("antes de entrar", () => {
  it("responde not-joined e não fecha", () => {
    const c = connect();
    send(c, { type: "status", live: true });
    expect(c.last()).toEqual({ type: "error", code: "not-joined" });
    expect(c.closedWith).toBeNull();
  });
});

describe("mensagens inválidas", () => {
  it("fecha quem manda JSON fora do contrato e avisa os outros que ele saiu", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    room.message(wrap(beto), "não é json");
    expect(beto.last()).toEqual({ type: "error", code: "invalid-message" });
    expect(beto.closedWith?.code).toBe(CLOSE.invalid);
    expect(ana.last()).toEqual({ type: "member-left", id: idOf(beto) });
  });

  it("fecha quem manda binário ou mensagem grande demais", () => {
    const a = connect();
    room.message(wrap(a), new ArrayBuffer(4));
    expect(a.closedWith?.code).toBe(CLOSE.invalid);

    const b = connect();
    room.message(wrap(b), JSON.stringify({ type: "leave", junk: "x".repeat(MAX_MESSAGE_BYTES) }));
    expect(b.closedWith?.code).toBe(CLOSE.invalid);
  });
});

describe("status ao vivo", () => {
  it("avisa os outros quando alguém começa a transmitir", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(ana, { type: "status", live: true });
    expect(beto.last()).toEqual({
      type: "member-updated",
      member: {
        id: idOf(ana),
        name: "Ana",
        live: true,
        muted: false,
        deafened: false,
        discord: null,
        verified: false,
        watching: null,
        watchingAll: [],
        effect: null,
      },
    });

    const caio = join("Caio");
    const welcome = caio.last();
    expect(welcome?.type === "welcome" && welcome.members.find((m) => m.name === "Ana")?.live).toBe(true);
  });
});

describe("estado de voz", () => {
  it("avisa os outros quando alguém silencia o microfone ou ensurdece", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(beto, { type: "status", live: false, muted: true, deafened: false, discord: null, verified: false });
    expect(ana.last()).toEqual({
      type: "member-updated",
      member: {
        id: idOf(beto),
        name: "Beto",
        live: false,
        muted: true,
        deafened: false,
        discord: null,
        verified: false,
        watching: null,
        watchingAll: [],
        effect: null,
      },
    });
    send(beto, { type: "status", live: true, muted: true, deafened: true });
    expect(ana.last()).toMatchObject({ member: { live: true, muted: true, deafened: true } });
  });

  it("conta para a sala quem cada membro está assistindo", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(ana, { type: "status", live: true });
    send(beto, { type: "status", live: false, watching: idOf(ana) });
    expect(ana.last()).toMatchObject({ type: "member-updated", member: { id: idOf(beto), watching: idOf(ana) } });
    send(beto, { type: "status", live: false, watching: null });
    expect(ana.last()).toMatchObject({ member: { id: idOf(beto), watching: null, watchingAll: [] } });
    // Várias lives ao mesmo tempo (ADR 0019).
    const caio = join("Caio");
    send(caio, { type: "status", live: true });
    send(beto, { type: "status", live: false, watching: idOf(ana), watchingAll: [idOf(ana), idOf(caio)] });
    expect(ana.last()).toMatchObject({
      member: { id: idOf(beto), watching: idOf(ana), watchingAll: [idOf(ana), idOf(caio)] },
    });
  });
});

describe("efeito de fala", () => {
  it("passa para a sala o efeito que cada membro escolheu, e o anel padrão quando ele tira", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(beto, { type: "status", live: false, effect: { kind: "rays", color: "#a78bfa" } });
    expect(ana.last()).toMatchObject({
      type: "member-updated",
      member: { id: idOf(beto), effect: { kind: "rays", color: "#a78bfa" } },
    });
    // Quem entra depois já recebe o efeito no welcome.
    const caio = join("Caio");
    const welcome = caio.sent.find((m) => m.type === "welcome");
    expect(welcome?.type === "welcome" && welcome.members.find((m) => m.name === "Beto")?.effect).toEqual({
      kind: "rays",
      color: "#a78bfa",
    });
    send(beto, { type: "status", live: false, effect: null });
    expect(ana.last()).toMatchObject({ member: { id: idOf(beto), effect: null } });
  });
});

describe("repasse de sinalização", () => {
  it("entrega o payload só para o destinatário, com o remetente", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    const caio = join("Caio");
    const before = caio.sent.length;
    send(ana, { type: "signal", to: idOf(beto), payload: { candidates: ["1.2.3.4:5000"], fingerprint: "ab" } });
    expect(beto.last()).toEqual({
      type: "signal",
      from: idOf(ana),
      payload: { candidates: ["1.2.3.4:5000"], fingerprint: "ab" },
    });
    expect(caio.sent.length).toBe(before);
  });

  it("responde unknown-peer para destino que não está na sala (ou para si mesmo)", () => {
    const ana = join("Ana");
    send(ana, { type: "signal", to: "ffffffffffffffff", payload: {} });
    expect(ana.last()).toEqual({ type: "error", code: "unknown-peer" });
    send(ana, { type: "signal", to: idOf(ana), payload: {} });
    expect(ana.last()).toEqual({ type: "error", code: "unknown-peer" });
    expect(ana.closedWith).toBeNull();
  });
});

describe("sair da sala", () => {
  it("leave avisa os outros e fecha normalmente", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    send(beto, { type: "leave" });
    expect(ana.last()).toEqual({ type: "member-left", id: idOf(beto) });
    expect(beto.closedWith?.code).toBe(CLOSE.normal);
  });

  it("queda da conexão avisa os outros uma vez só", () => {
    const ana = join("Ana");
    const beto = join("Beto");
    room.closed(wrap(beto));
    room.closed(wrap(beto));
    expect(ana.sent.filter((m) => m.type === "member-left")).toHaveLength(1);
  });

  it("queda de quem nunca entrou não avisa ninguém", () => {
    const ana = join("Ana");
    const before = ana.sent.length;
    room.closed(connect());
    room.closed(new FakeConnection());
    expect(ana.sent.length).toBe(before);
  });
});

describe("limite de taxa", () => {
  it(`fecha quem manda mais de ${RATE_LIMIT_MESSAGES} mensagens em 10 s, mesmo só de status`, () => {
    const ana = join("Ana");
    for (let i = 0; i < RATE_LIMIT_MESSAGES; i++) send(ana, { type: "status", live: i % 2 === 0 });
    expect(ana.closedWith?.code).toBe(CLOSE.rateLimited);
    expect(ana.sent).toContainEqual({ type: "error", code: "rate-limited" });
  });

  it("libera de novo quando a janela passa", () => {
    const ana = join("Ana");
    for (let i = 0; i < RATE_LIMIT_MESSAGES - 1; i++) send(ana, { type: "status", live: true });
    now += RATE_LIMIT_WINDOW_MS;
    for (let i = 0; i < 10; i++) send(ana, { type: "status", live: true });
    expect(ana.closedWith).toBeNull();
  });

  it("aceita uma conexão sem estado salvo (ex.: depois de a sala acordar)", () => {
    const c = new FakeConnection();
    conns.push(c);
    send(c, { type: "join", name: "Ana", protocol: 1 });
    expect(c.last()?.type).toBe("welcome");
  });
});

describe("ids", () => {
  it("têm 16 caracteres hex", () => {
    expect(randomPeerId()).toMatch(/^[0-9a-f]{16}$/);
  });
});

describe("amigos na sala (ADR 0011)", () => {
  const ANA = "111111111111111111";
  const BETO = "222222222222222222";
  let met: [string, string][];
  let answers: { requester: string; result: unknown }[];
  let requests: Record<string, unknown>;

  beforeEach(() => {
    met = [];
    answers = [];
    requests = {};
    room = new RoomState(
      () => conns.filter((c) => c.closedWith === null).map(wrap),
      () => (ids++).toString(16).padStart(16, "0"),
      () => now,
      {
        code: () => "ABCDEF",
        met: (a, b) => met.push([a.id, b.id]),
        joinAnswered: (requester, result) => answers.push({ requester, result }),
        getRequests: () => structuredClone(requests) as never,
        putRequests: (r) => (requests = structuredClone(r)),
      },
    );
  });

  function joinVerified(name: string, id: string, discord?: unknown) {
    const c = connect();
    room.message(wrap(c), JSON.stringify({ type: "join", name, protocol: 1, discord, session: "x" }), id);
    return c;
  }

  it("a conta do Discord de quem é conferido é a da sessão, não a que o app diz", () => {
    joinVerified("Ana", ANA, { id: BETO, avatar: "a".repeat(32) });
    const caio = join("Caio");
    expect(caio.last()).toMatchObject({ members: [{ name: "Ana", discord: { id: ANA, avatar: null } }] });
  });

  it("conferido não troca a própria conta pela de outra pessoa", () => {
    const ana = joinVerified("Ana", ANA);
    const caio = join("Caio");
    send(ana, { type: "rename", name: "Ana", discord: { id: BETO, avatar: null } });
    expect(caio.last()).toMatchObject({ member: { discord: { id: ANA } } });
  });

  it("quem entra com o Discord já na sala é conferido sem sair e entrar", () => {
    joinVerified("Ana", ANA);
    const beto = join("Beto");
    const ana = conns[0];
    expect(ana.last()).toMatchObject({ type: "member-joined", member: { name: "Beto", verified: false } });
    // Beto entrou com o Discord: manda a sessão e vira conferido para todos, e entra nos recentes.
    room.message(wrap(beto), JSON.stringify({ type: "verify", session: "x" }), BETO);
    expect(ana.last()).toMatchObject({
      type: "member-updated",
      member: { name: "Beto", verified: true, discord: { id: BETO } },
    });
    expect(met).toEqual([[BETO, ANA]]);
    // Sessão que não confere, ou de outra conta depois de conferido: ignora.
    const before = ana.sent.length;
    room.message(wrap(beto), JSON.stringify({ type: "verify", session: "x" }), null);
    room.message(wrap(beto), JSON.stringify({ type: "verify", session: "x" }), ANA);
    expect(ana.sent.length).toBe(before);
  });

  it("conferidos na mesma sala viram recentes um do outro", () => {
    joinVerified("Ana", ANA);
    join("Caio");
    joinVerified("Beto", BETO);
    expect(met).toEqual([[BETO, ANA]]);
  });

  it("pedido de entrada aparece para todos e o primeiro que responde decide", () => {
    const ana = join("Ana");
    const caio = join("Caio");
    const from = { id: BETO, name: "Beto", avatar: null };
    expect(room.requestJoin(from, "00000000000000aa")).toBe(true);
    expect(ana.last()).toEqual({ type: "join-request", id: "00000000000000aa", from });
    expect(caio.last()).toEqual({ type: "join-request", id: "00000000000000aa", from });
    send(caio, { type: "join-answer", id: "00000000000000aa", accept: true });
    expect(answers).toEqual([
      { requester: BETO, result: { kind: "approved", code: "ABCDEF", room: null, by: "Caio" } },
    ]);
    expect(ana.last()).toEqual({ type: "join-request-closed", id: "00000000000000aa" });
    // Responder de novo não faz nada.
    send(ana, { type: "join-answer", id: "00000000000000aa", accept: false });
    expect(answers).toHaveLength(1);
  });

  it("recusar avisa quem pediu", () => {
    const ana = join("Ana");
    room.requestJoin({ id: BETO, name: "Beto", avatar: null }, "00000000000000aa");
    send(ana, { type: "join-answer", id: "00000000000000aa", accept: false });
    expect(answers[0]).toEqual({ requester: BETO, result: { kind: "denied", by: "Ana" } });
  });

  it("pedido vence em 60 s, e sala vazia não recebe pedido", () => {
    expect(room.requestJoin({ id: BETO, name: "Beto", avatar: null }, "00000000000000aa")).toBe(false);
    const ana = join("Ana");
    room.requestJoin({ id: BETO, name: "Beto", avatar: null }, "00000000000000aa");
    now += 60_000;
    send(ana, { type: "join-answer", id: "00000000000000aa", accept: true });
    expect(answers).toEqual([]);
    expect(ana.sent).toContainEqual({ type: "join-request-closed", id: "00000000000000aa" });
  });

  it("quem chega vê os pedidos ainda abertos", () => {
    join("Ana");
    room.requestJoin({ id: BETO, name: "Beto", avatar: null }, "00000000000000aa");
    const caio = join("Caio");
    expect(caio.last()).toMatchObject({ type: "join-request", id: "00000000000000aa" });
  });
});
