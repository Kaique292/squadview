import { describe, expect, it, vi } from "vitest";
import {
  generateTurnCredentials,
  meteredCredentials,
  parseIceServers,
  TURN_RENEW_BEFORE_MS,
  TURN_TTL_S,
  TurnCache,
  turnProvider,
} from "./turn";

const RESPONSE = {
  iceServers: [
    { urls: ["stun:stun.cloudflare.com:3478", "stun:stun.cloudflare.com:53"] },
    {
      urls: [
        "turn:turn.cloudflare.com:3478?transport=udp",
        "turn:turn.cloudflare.com:53?transport=udp",
        "turn:turn.cloudflare.com:443?transport=tcp",
        "turns:turn.cloudflare.com:5349?transport=tcp",
      ],
      username: "u1",
      credential: "segredo",
    },
  ],
};

describe("credenciais do repassador TURN", () => {
  it("fica com a entrada TURN que tem credencial e tira a porta 53", () => {
    expect(parseIceServers(RESPONSE)).toEqual({
      urls: [
        "turn:turn.cloudflare.com:3478?transport=udp",
        "turn:turn.cloudflare.com:443?transport=tcp",
        "turns:turn.cloudflare.com:5349?transport=tcp",
      ],
      username: "u1",
      credential: "segredo",
    });
  });

  it("aceita iceServers como objeto (formato antigo) e recusa o que não serve", () => {
    expect(parseIceServers({ iceServers: RESPONSE.iceServers[1] })?.username).toBe("u1");
    expect(parseIceServers({ iceServers: [RESPONSE.iceServers[0]] })).toBeNull();
    expect(parseIceServers(null)).toBeNull();
    expect(parseIceServers({ iceServers: [{ urls: "turn:x:3478", username: 1, credential: "c" }] })).toBeNull();
  });

  it("chama a API da Cloudflare com a chave e o prazo", async () => {
    const fetcher = vi.fn(async () => Response.json(RESPONSE, { status: 201 }));
    const turn = await generateTurnCredentials("chave", "token", fetcher as unknown as typeof fetch);
    expect(turn?.credential).toBe("segredo");
    const [url, init] = fetcher.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://rtc.live.cloudflare.com/v1/turn/keys/chave/credentials/generate-ice-servers");
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer token");
    expect(JSON.parse(init.body as string)).toEqual({ ttl: TURN_TTL_S });
  });

  it("chave errada ou fora do ar: segue sem repassador", async () => {
    const recusa = vi.fn(async () => new Response("não", { status: 401 }));
    expect(await generateTurnCredentials("a", "b", recusa as unknown as typeof fetch)).toBeNull();
    const caiu = vi.fn(async () => {
      throw new Error("rede");
    });
    expect(await generateTurnCredentials("a", "b", caiu as unknown as typeof fetch)).toBeNull();
  });

  it("guarda as credenciais e só pede novas perto de vencer, com um pedido para quem entra junto", async () => {
    let now = 0;
    let n = 0;
    const generate = vi.fn(async () => ({ urls: ["turn:x:3478"], username: `u${++n}`, credential: "c" }));
    const cache = new TurnCache(generate, () => now);
    const [a, b] = await Promise.all([cache.get(), cache.get()]);
    expect([a?.username, b?.username]).toEqual(["u1", "u1"]);
    expect(generate).toHaveBeenCalledTimes(1);
    now = TURN_TTL_S * 1000 - TURN_RENEW_BEFORE_MS - 1;
    expect((await cache.get())?.username).toBe("u1");
    now += 2;
    expect((await cache.get())?.username).toBe("u2");
  });

  it("Metered: lista direto, uma URL por entrada, junta as da mesma credencial", async () => {
    const lista = [
      { urls: "stun:x.relay.metered.ca:80" },
      { urls: "turn:x.relay.metered.ca:80", username: "m", credential: "s" },
      { urls: "turn:x.relay.metered.ca:80?transport=tcp", username: "m", credential: "s" },
      { urls: "turn:x.relay.metered.ca:443", username: "m", credential: "s" },
    ];
    const fetcher = vi.fn(async () => Response.json(lista));
    const turn = await meteredCredentials("squad.metered.live", "k&1", fetcher as unknown as typeof fetch);
    expect(turn).toEqual({
      urls: ["turn:x.relay.metered.ca:80", "turn:x.relay.metered.ca:80?transport=tcp", "turn:x.relay.metered.ca:443"],
      username: "m",
      credential: "s",
    });
    expect(fetcher.mock.calls[0]).toEqual(["https://squad.metered.live/api/v1/turn/credentials?apiKey=k%261"]);
    // Domínio que não é do Metered: a chave não sai daqui.
    expect(await meteredCredentials("evil.com/x?", "k", fetcher as unknown as typeof fetch)).toBeNull();
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("o provedor vem dos segredos configurados (e nenhum: só conexão direta)", async () => {
    const fetcher = vi.fn(async () => Response.json(RESPONSE, { status: 201 }));
    const f = fetcher as unknown as typeof fetch;
    expect(turnProvider({}, f)).toBeNull();
    expect(turnProvider({ TURN_KEY_ID: "a" }, f)).toBeNull();
    expect(await turnProvider({ TURN_KEY_ID: "a", TURN_KEY_API_TOKEN: "b" }, f)!()).toMatchObject({ username: "u1" });
    const fixo = turnProvider({ TURN_URLS: "turn:a:3478, turn:b:3478", TURN_USERNAME: "x", TURN_CREDENTIAL: "y" }, f);
    expect(await fixo!()).toEqual({ urls: ["turn:a:3478", "turn:b:3478"], username: "x", credential: "y" });
    expect(turnProvider({ METERED_DOMAIN: "a.metered.live", METERED_API_KEY: "k" }, f)).not.toBeNull();
  });
});
