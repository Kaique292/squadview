import { describe, expect, it } from "vitest";
import { generateRoomCode, isValidRoomCode, ROOM_CODE_LENGTH } from "./room-code";

describe("códigos de sala", () => {
  it("gera códigos válidos e com o tamanho certo", () => {
    const code = generateRoomCode();
    expect(code).toHaveLength(ROOM_CODE_LENGTH);
    expect(isValidRoomCode(code)).toBe(true);
  });

  it("usa o gerador de aleatoriedade recebido", () => {
    expect(generateRoomCode(() => 0)).toBe("AAAAAA");
  });

  it("rejeita caracteres ambíguos e tamanho errado", () => {
    expect(isValidRoomCode("AB0DEF")).toBe(false);
    expect(isValidRoomCode("ABC")).toBe(false);
  });
});
