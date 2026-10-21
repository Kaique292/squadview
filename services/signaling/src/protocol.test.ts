import { describe, expect, it } from "vitest";
import { cleanName, MAX_SIGNAL_PAYLOAD_BYTES, parseClientMessage } from "./protocol";

const PEER = "0123456789abcdef";

describe("nome de exibição", () => {
  it("tira espaços das pontas e aceita acentos e emoji", () => {
    expect(cleanName("  Kaique  ")).toBe("Kaique");
    expect(cleanName("João 🎮")).toBe("João 🎮");
  });

  it("recusa vazio, longo demais, caracteres de controle e não-texto", () => {
    expect(cleanName("   ")).toBeNull();
    expect(cleanName("a".repeat(33))).toBeNull();
    expect(cleanName("a".repeat(32))).toBe("a".repeat(32));
    expect(cleanName("Ana\nBeto")).toBeNull();
    expect(cleanName(42)).toBeNull();
  });
});

describe("mensagens do cliente", () => {
  it("lê a conta do Discord no join e no rename", () => {
    const discord = { id: "123456789012345678", avatar: "a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" };
    expect(parseClientMessage(JSON.stringify({ type: "join", name: "Ana", protocol: 1, discord }))).toMatchObject({
      discord,
    });
    // Sem foto: avatar nulo (foto padrão do Discord).
    expect(
      parseClientMessage(JSON.stringify({ type: "join", name: "Ana", protocol: 1, discord: { id: discord.id } })),
    ).toMatchObject({ discord: { id: discord.id, avatar: null } });
    expect(parseClientMessage(JSON.stringify({ type: "rename", name: "Ana", discord }))).toEqual({
      type: "rename",
      name: "Ana",
      discord,
    });
    expect(parseClientMessage(JSON.stringify({ type: "rename", name: "Ana", discord: null }))).toEqual({
      type: "rename",
      name: "Ana",
      discord: null,
    });
  });

  it("lê o pedido de conferir a sessão já na sala", () => {
    expect(parseClientMessage(JSON.stringify({ type: "verify", session: "abc.def" }))).toEqual({
      type: "verify",
      session: "abc.def",
    });
    expect(parseClientMessage(JSON.stringify({ type: "verify", session: "" }))).toBeNull();
    expect(parseClientMessage(JSON.stringify({ type: "verify", session: "x".repeat(513) }))).toBeNull();
  });

  it("recusa conta do Discord fora do formato (nada de URL)", () => {
    for (const discord of [
      { id: "abc", avatar: null },
      { id: "123456789012345678", avatar: "https://exemplo.com/foto.png" },
      { id: "123456789012345678", avatar: "../../x" },
      "123456789012345678",
    ]) {
      expect(parseClientMessage(JSON.stringify({ type: "join", name: "Ana", protocol: 1, discord }))).toBeNull();
      expect(parseClientMessage(JSON.stringify({ type: "rename", name: "Ana", discord }))).toBeNull();
    }
  });

  it("lê join, status, signal e leave válidos", () => {
    expect(parseClientMessage('{"type":"join","name":" Ana ","protocol":1}')).toEqual({
      type: "join",
      name: "Ana",
      protocol: 1,
      room: null,
      discord: null,
      session: null,
    });
    expect(parseClientMessage('{"type":"join","name":"Ana","protocol":1,"room":" Galera do LoL "}')).toEqual({
      type: "join",
      name: "Ana",
      protocol: 1,
      room: "Galera do LoL",
      discord: null,
      session: null,
    });
    expect(parseClientMessage('{"type":"room","name":"Squad"}')).toEqual({ type: "room", name: "Squad" });
    expect(parseClientMessage('{"type":"rename","name":"K.A"}')).toEqual({ type: "rename", name: "K.A" });
    // Apps antigos só mandam live: mudo e surdo ficam falsos.
    expect(parseClientMessage('{"type":"status","live":true}')).toEqual({
      type: "status",
      live: true,
      muted: false,
      deafened: false,
      watching: null,
      effect: null,
    });
    expect(parseClientMessage('{"type":"status","live":false,"muted":true,"deafened":true}')).toEqual({
      type: "status",
      live: false,
      muted: true,
      deafened: true,
      watching: null,
      effect: null,
    });
    // Quem estamos assistindo (para quem transmite ver quem assiste).
    expect(parseClientMessage(`{"type":"status","live":false,"watching":"${PEER}"}`)).toMatchObject({
      type: "status",
      watching: PEER,
    });
    expect(parseClientMessage(`{"type":"signal","to":"${PEER}","payload":{"candidates":[]}}`)).toEqual({
      type: "signal",
      to: PEER,
      payload: { candidates: [] },
    });
    expect(parseClientMessage('{"type":"leave"}')).toEqual({ type: "leave" });
  });

  it("recusa JSON quebrado, tipos desconhecidos e campos errados", () => {
    expect(parseClientMessage("{")).toBeNull();
    expect(parseClientMessage("[]")).toBeNull();
    expect(parseClientMessage("null")).toBeNull();
    expect(parseClientMessage('{"type":"hack"}')).toBeNull();
    expect(parseClientMessage('{"type":"join","name":"Ana"}')).toBeNull();
    expect(parseClientMessage('{"type":"join","name":"Ana","protocol":1.5}')).toBeNull();
    expect(parseClientMessage('{"type":"join","name":"","protocol":1}')).toBeNull();
    expect(parseClientMessage('{"type":"status","live":"sim"}')).toBeNull();
    expect(parseClientMessage('{"type":"status","live":true,"muted":"sim"}')).toBeNull();
    expect(parseClientMessage('{"type":"status","live":true,"watching":"não é um id"}')).toBeNull();
    expect(parseClientMessage('{"type":"status","live":true,"watching":7}')).toBeNull();
    expect(parseClientMessage('{"type":"join","name":"Ana","protocol":1,"room":""}')).toBeNull();
    expect(parseClientMessage('{"type":"room","name":"   "}')).toBeNull();
    expect(parseClientMessage('{"type":"rename"}')).toBeNull();
  });

  it("lê o efeito de fala do status e troca o inválido ou desconhecido pelo anel padrão", () => {
    const status = (effect: unknown) => parseClientMessage(JSON.stringify({ type: "status", live: false, effect }));
    expect(status({ kind: "waves", color: "#FF8800" })).toMatchObject({ effect: { kind: "waves", color: "#ff8800" } });
    expect(status(null)).toMatchObject({ effect: null });
    // Um efeito de um app mais novo não derruba ninguém: vira o anel padrão.
    expect(status({ kind: "fogo", color: "#ff8800" })).toMatchObject({ effect: null });
    expect(status({ kind: "dots", color: "vermelho" })).toMatchObject({ effect: null });
    expect(status({ kind: "dots", color: "#ff8800;background:url(x)" })).toMatchObject({ effect: null });
    expect(status("dots")).toMatchObject({ effect: null });
  });

  it("recusa signal com destino inválido ou payload que não é objeto", () => {
    expect(parseClientMessage('{"type":"signal","to":"curto","payload":{}}')).toBeNull();
    expect(parseClientMessage(`{"type":"signal","to":"${PEER}","payload":[1]}`)).toBeNull();
    expect(parseClientMessage(`{"type":"signal","to":"${PEER}","payload":"x"}`)).toBeNull();
  });

  it("recusa signal com payload maior que o limite", () => {
    const big = JSON.stringify({ type: "signal", to: PEER, payload: { x: "a".repeat(MAX_SIGNAL_PAYLOAD_BYTES) } });
    expect(parseClientMessage(big)).toBeNull();
  });
});
