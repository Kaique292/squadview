import { describe, expect, it } from "vitest";
import {
  cleanIcon,
  INVITE_TTL_MS,
  MAX_ICON_LENGTH,
  MAX_INVITES,
  MAX_SERVER_MEMBERS,
  MAX_SERVER_ROOMS,
  secureRandom,
  ServerState,
} from "./server-state";
import type { Profile } from "./session";

const DONO = { id: "111111111111111111", name: "Dona", avatar: null };
const ADM = { id: "222222222222222222", name: "Adm", avatar: null };
const ZE = { id: "333333333333333333", name: "Zé", avatar: "a".repeat(32) };

function fresh(now = { t: 1_000 }) {
  const store = new Map<string, unknown>();
  let n = 0;
  const state = new ServerState(
    {
      get: (k) => structuredClone(store.get(k)) as never,
      put: (k, v) => store.set(k, structuredClone(v)),
      delete: (k) => store.delete(k),
    },
    () => now.t,
    {
      roomId: () => (++n).toString(16).padStart(8, "0"),
      roomCode: () => "ABCDEF",
      secret: () => `Segredo${String(++n).padStart(5, "0")}`,
    },
  );
  state.create("00000000000000aa", "Turma", DONO);
  return { state, store, now };
}

/** Convida e entra (devolve o resultado da entrada). */
function join(state: ServerState, who: Profile) {
  const invite = state.act(DONO, { kind: "invite" });
  if (!invite.ok || !invite.invite) throw new Error("sem convite");
  return state.act(who, { kind: "join", secret: invite.invite });
}

describe("servidor", () => {
  it("ícone: só data URL de imagem até o limite", () => {
    expect(cleanIcon(null)).toBeNull();
    expect(cleanIcon("data:image/png;base64,iVBORw0KGgo=")).toBe("data:image/png;base64,iVBORw0KGgo=");
    expect(cleanIcon("data:image/svg+xml;base64,PHN2Zz4=")).toBeUndefined();
    expect(cleanIcon(`data:image/png;base64,${"A".repeat(MAX_ICON_LENGTH)}`)).toBeUndefined();
    expect(cleanIcon(42)).toBeUndefined();
  });

  it("sorteia ids, códigos e segredos no formato certo", () => {
    expect(secureRandom.roomId()).toMatch(/^[0-9a-f]{8}$/);
    expect(secureRandom.roomCode()).toHaveLength(6);
    expect(secureRandom.secret()).toMatch(/^[A-Za-z0-9]{12}$/);
  });

  it("lista o dono primeiro, depois administradores e membros por nome, e mantém perfis em dia", () => {
    const { state } = fresh();
    join(state, ZE);
    join(state, ADM);
    state.act(DONO, { kind: "role", member: ADM.id, role: "admin" });
    expect(state.snapshot()!.members.map((m) => m.name)).toEqual(["Dona", "Adm", "Zé"]);
    expect(state.refreshProfile({ ...ZE, name: "José" })).toBe(true);
    expect(state.refreshProfile({ ...ZE, name: "José" })).toBe(false);
    expect(state.refreshProfile({ id: "999999999999999999", name: "Outro", avatar: null })).toBe(false);
    expect(state.snapshot()!.members.find((m) => m.id === ZE.id)?.name).toBe("José");
  });

  it("entrar de novo pelo convite não duplica ninguém", () => {
    const { state } = fresh();
    join(state, ZE);
    expect(join(state, ZE)).toMatchObject({ ok: true });
    expect(state.memberIds()).toHaveLength(2);
  });

  it("convite vence em 7 dias e some sozinho", () => {
    const { state, store, now } = fresh();
    const invite = state.act(ZE, { kind: "invite" });
    expect(invite).toMatchObject({ ok: false, error: "not-member" });
    const ok = state.act(DONO, { kind: "invite" });
    if (!ok.ok) throw new Error();
    now.t += INVITE_TTL_MS;
    expect(state.act(ZE, { kind: "join", secret: ok.invite! })).toEqual({ ok: false, error: "invite-invalid" });
    expect(store.get("invites")).toEqual({});
  });

  it(`no máximo ${MAX_INVITES} convites ativos e ${MAX_SERVER_MEMBERS} membros`, () => {
    const { state } = fresh();
    for (let i = 0; i < MAX_INVITES; i++) state.act(DONO, { kind: "invite" });
    expect(state.act(DONO, { kind: "invite" })).toEqual({ ok: false, error: "too-many" });

    const members: Record<string, unknown> = {};
    for (let i = 0; i < MAX_SERVER_MEMBERS; i++) {
      const id = `5${String(i).padStart(17, "0")}`;
      members[id] = { id, name: `P${i}`, avatar: null, role: "member" };
    }
    // Servidor lotado (montado direto no armazenamento): o convite não adianta.
    const { state: lotado, store } = fresh();
    const convite = lotado.act(DONO, { kind: "invite" });
    store.set("members", { ...members, [DONO.id]: { ...DONO, role: "owner" } });
    if (!convite.ok) throw new Error();
    expect(lotado.act(ZE, { kind: "join", secret: convite.invite! })).toEqual({ ok: false, error: "server-full" });
  });

  it(`no máximo ${MAX_SERVER_ROOMS} salas; renomear ou apagar sala que não existe avisa`, () => {
    const { state } = fresh();
    for (let i = 1; i < MAX_SERVER_ROOMS; i++) state.act(DONO, { kind: "room-create", name: `Sala ${i}` });
    expect(state.act(DONO, { kind: "room-create", name: "Mais uma" })).toEqual({ ok: false, error: "too-many" });
    expect(state.act(DONO, { kind: "room-rename", room: "ffffffff", name: "X" })).toEqual({
      ok: false,
      error: "unknown-room",
    });
    expect(state.act(DONO, { kind: "room-delete", room: "ffffffff" })).toEqual({ ok: false, error: "unknown-room" });
  });

  it("membro comum não mexe em nada além de convidar e sair", () => {
    const { state } = fresh();
    join(state, ZE);
    for (const op of [
      { kind: "update", name: "X" },
      { kind: "delete" },
      { kind: "room-create", name: "X" },
      { kind: "room-rename", room: "00000001", name: "X" },
      { kind: "room-delete", room: "00000001" },
      { kind: "kick", member: DONO.id },
      { kind: "role", member: ZE.id, role: "admin" },
    ] as const) {
      expect(state.act(ZE, op), op.kind).toEqual({ ok: false, error: "not-allowed" });
    }
    expect(state.act(ZE, { kind: "invite" })).toMatchObject({ ok: true });
    expect(state.act(ZE, { kind: "leave" })).toMatchObject({ ok: true, removed: [ZE.id] });
  });

  it("administrador tira membros, só o dono tira administradores, ninguém tira o dono", () => {
    const { state } = fresh();
    join(state, ADM);
    join(state, ZE);
    state.act(DONO, { kind: "role", member: ADM.id, role: "admin" });
    expect(state.act(ADM, { kind: "kick", member: ADM.id })).toEqual({ ok: false, error: "not-allowed" });
    expect(state.act(ADM, { kind: "kick", member: "999999999999999999" })).toEqual({
      ok: false,
      error: "unknown-member",
    });
    expect(state.act(ADM, { kind: "kick", member: ZE.id })).toMatchObject({ ok: true, removed: [ZE.id] });
    expect(state.act(DONO, { kind: "role", member: DONO.id, role: "member" })).toEqual({
      ok: false,
      error: "not-allowed",
    });
    expect(state.act(DONO, { kind: "role", member: ZE.id, role: "admin" })).toEqual({
      ok: false,
      error: "unknown-member",
    });
    expect(state.act(DONO, { kind: "kick", member: ADM.id })).toMatchObject({ ok: true, removed: [ADM.id] });
  });

  it("a última sala não pode ser apagada", () => {
    const { state } = fresh();
    const [geral] = state.snapshot()!.rooms;
    expect(state.act(DONO, { kind: "room-delete", room: geral.id })).toEqual({ ok: false, error: "not-allowed" });
  });

  it("trocar o ícone e tirar o ícone", () => {
    const { state } = fresh();
    const icon = "data:image/webp;base64,UklGRg==";
    expect(state.act(DONO, { kind: "update", icon })).toMatchObject({ ok: true, snapshot: { icon, name: "Turma" } });
    expect(state.act(DONO, { kind: "update", icon: null })).toMatchObject({ snapshot: { icon: null } });
  });

  // Regressão (ADR 0025): quem entrou numa sala do servidor só com o código não aparecia para quem
  // via a sala de fora, porque só o registro de cada membro avisava o servidor.
  it("a lista que a sala manda vale por ela: membros e quem não é membro", () => {
    const { state } = fresh();
    state.setPresence(DONO.id, "ABCDEF", false);
    const geral = state.snapshot()!.rooms[0].id;
    expect(state.presence()[geral]).toEqual([{ id: DONO.id, live: false, muted: false, deafened: false }]);

    const visita = {
      id: "0123456789abcdef",
      name: "Lobo",
      discord: { id: "444444444444444444", avatar: null },
      live: false,
      muted: true,
      deafened: true,
    };
    const dona = {
      id: DONO.id,
      name: "Dona",
      discord: { id: DONO.id, avatar: null },
      live: true,
      muted: false,
      deafened: false,
    };
    expect(state.setOccupants("ABCDEF", [dona, visita])).toBe(true);
    expect(state.presence()[geral]).toEqual([
      { id: DONO.id, live: true, muted: false, deafened: false, name: "Dona", discord: dona.discord },
      { id: visita.id, live: false, muted: true, deafened: true, name: "Lobo", discord: visita.discord },
    ]);
    expect(state.setOccupants("ABCDEF", [dona, visita])).toBe(false);
    expect(state.setOccupants("ZZZZZZ", [visita])).toBe(false);

    // A sala esvaziou: lista vazia (a presença antiga da Dona não volta a valer).
    expect(state.setOccupants("ABCDEF", [])).toBe(true);
    expect(state.presence()[geral]).toEqual([]);
  });

  it("liga as salas ao servidor uma vez, e desliga tudo quando ele é apagado", () => {
    const { state, store } = fresh();
    expect(state.syncRoomBindings()).toEqual({ bind: ["ABCDEF"], unbind: [] });
    expect(state.syncRoomBindings()).toEqual({ bind: [], unbind: [] });
    state.setOccupants("ABCDEF", []);
    state.act(DONO, { kind: "delete" });
    expect(state.syncRoomBindings()).toEqual({ bind: [], unbind: ["ABCDEF"] });
    expect(store.size).toBe(0);
  });

  it("apagado, o servidor não responde mais a ninguém", () => {
    const { state, store } = fresh();
    join(state, ZE);
    expect(state.act(DONO, { kind: "delete" })).toEqual({ ok: true, snapshot: null, removed: [DONO.id, ZE.id] });
    expect(state.snapshot()).toBeNull();
    expect(store.size).toBe(0);
    expect(state.act(DONO, { kind: "invite" })).toEqual({ ok: false, error: "not-member" });
    expect(state.act(ZE, { kind: "join", secret: "x" })).toEqual({ ok: false, error: "not-member" });
  });
});
