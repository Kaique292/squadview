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
  it("lê join, status, signal e leave válidos", () => {
    expect(parseClientMessage('{"type":"join","name":" Ana ","protocol":1}')).toEqual({
      type: "join",
      name: "Ana",
      protocol: 1,
      room: null,
    });
    expect(parseClientMessage('{"type":"join","name":"Ana","protocol":1,"room":" Galera do LoL "}')).toEqual({
      type: "join",
      name: "Ana",
      protocol: 1,
      room: "Galera do LoL",
    });
    expect(parseClientMessage('{"type":"room","name":"Squad"}')).toEqual({ type: "room", name: "Squad" });
    expect(parseClientMessage('{"type":"rename","name":"K.A"}')).toEqual({ type: "rename", name: "K.A" });
    // Apps antigos só mandam live: mudo e surdo ficam falsos.
    expect(parseClientMessage('{"type":"status","live":true}')).toEqual({
      type: "status",
      live: true,
      muted: false,
      deafened: false,
    });
    expect(parseClientMessage('{"type":"status","live":false,"muted":true,"deafened":true}')).toEqual({
      type: "status",
      live: false,
      muted: true,
      deafened: true,
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
    expect(parseClientMessage('{"type":"join","name":"Ana","protocol":1,"room":""}')).toBeNull();
    expect(parseClientMessage('{"type":"room","name":"   "}')).toBeNull();
    expect(parseClientMessage('{"type":"rename"}')).toBeNull();
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
