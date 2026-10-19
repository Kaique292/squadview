import { describe, expect, it } from "vitest";
import { route } from "./router";

describe("router", () => {
  it("cria sala com POST /rooms", () => {
    expect(route("POST", "/rooms")).toEqual({ kind: "create-room" });
  });

  it("encaminha para a sala quando o código é válido", () => {
    expect(route("GET", "/rooms/ABCDEF")).toEqual({ kind: "room", code: "ABCDEF" });
  });

  it("devolve not-found para código inválido ou rota desconhecida", () => {
    expect(route("GET", "/rooms/AB0DEF")).toEqual({ kind: "not-found" });
    expect(route("GET", "/")).toEqual({ kind: "not-found" });
    expect(route("GET", "/rooms")).toEqual({ kind: "not-found" });
  });
});
