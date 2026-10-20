import { describe, expect, it, vi } from "vitest";
import { bearer } from "./router";
import { cleanAvatar, exchangeDiscordToken, SESSION_TTL_MS, signSession, verifySession } from "./session";

const SECRET = "segredo-de-teste";
const ID = "123456789012345678";

describe("sessão do SquadView", () => {
  it("assina e confere o id do Discord", async () => {
    const session = await signSession(SECRET, ID, 1000);
    expect(await verifySession(SECRET, session, 2000)).toBe(ID);
  });

  it("recusa sessão vencida, adulterada, de outro segredo ou malformada", async () => {
    const session = await signSession(SECRET, ID, 1000);
    expect(await verifySession(SECRET, session, 1000 + SESSION_TTL_MS)).toBeNull();
    expect(await verifySession("outro", session, 2000)).toBeNull();
    const [payload, sig] = session.split(".");
    const forged = btoa(JSON.stringify({ id: "876543210987654321", exp: 9e15 })).replace(/=+$/, "");
    expect(await verifySession(SECRET, `${forged}.${sig}`, 2000)).toBeNull();
    expect(await verifySession(SECRET, `${payload}.${sig}.x`, 2000)).toBeNull();
    expect(await verifySession(SECRET, 42, 2000)).toBeNull();
    expect(await verifySession("", session, 2000)).toBeNull();
  });

  it("lê a sessão só do cabeçalho Authorization", () => {
    expect(bearer("Bearer abc.def")).toBe("abc.def");
    expect(bearer("abc.def")).toBeNull();
    expect(bearer("Bearer a b")).toBeNull();
    expect(bearer(null)).toBeNull();
  });

  it("só aceita hash de foto do Discord", () => {
    expect(cleanAvatar("a_" + "f".repeat(32))).toBe("a_" + "f".repeat(32));
    expect(cleanAvatar("https://exemplo.com/x.png")).toBeNull();
  });
});

describe("troca do token do Discord", () => {
  it("lê o perfil no Discord e devolve uma sessão", async () => {
    const fetcher = vi.fn(async () =>
      Response.json({ id: ID, username: "kaique", global_name: "K.A", avatar: "b".repeat(32) }),
    );
    const result = await exchangeDiscordToken(SECRET, "token-ok", fetcher as unknown as typeof fetch, 1000);
    expect(result?.profile).toEqual({ id: ID, name: "K.A", avatar: "b".repeat(32) });
    expect(await verifySession(SECRET, result?.session, 2000)).toBe(ID);
    expect(fetcher).toHaveBeenCalledWith("https://discord.com/api/v10/users/@me", {
      headers: { Authorization: "Bearer token-ok" },
    });
  });

  it("recusa quando o Discord recusa o token ou o token é estranho", async () => {
    const denied = vi.fn(async () => new Response("no", { status: 401 }));
    expect(await exchangeDiscordToken(SECRET, "token", denied as unknown as typeof fetch)).toBeNull();
    expect(await exchangeDiscordToken(SECRET, "tem espaço", denied as unknown as typeof fetch)).toBeNull();
    expect(await exchangeDiscordToken(SECRET, undefined, denied as unknown as typeof fetch)).toBeNull();
    expect(denied).toHaveBeenCalledTimes(1);
  });
});
