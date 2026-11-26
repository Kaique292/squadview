import { describe, expect, it, vi } from "vitest";
import {
  generateTurnCredentials,
  hmacSha1Base64,
  meteredCredentials,
  parseIceServers,
  sharedSecretCredentials,
  TURN_RENEW_BEFORE_MS,
  TURN_TTL_S,
  TurnCache,
  turnIssuer,
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

describe("coturn próprio com segredo compartilhado (TURN REST API)", () => {
  const SECRET = "segredo-do-coturn";
  const URLS = "turn:187.53.103.68:3478?transport=udp, turn:187.53.103.68:3478?transport=tcp";
  const COTURN = { TURN_URLS: URLS, TURN_SHARED_SECRET: SECRET };
  /** 2023-11-14 22:13:20 UTC; com as 24 h, vence em 1700086400. */
  const NOW = 1_700_000_000_000;
  const noFetch = vi.fn() as unknown as typeof fetch;

  it("o HMAC-SHA1 em base64 bate com o calculado à parte", async () => {
    // Vetores calculados com o Python (`hmac` + `hashlib.sha1` + `base64`) e conferidos com o
    // `openssl dgst -sha1 -hmac`.
    expect(await hmacSha1Base64(SECRET, "1700086400:123456789012345678")).toBe("/izOcy2uJirc48HkBZypzBM8aNg=");
    expect(await hmacSha1Base64("outro", "1700000000:a1b2c3d4e5f60718")).toBe("7JhNOkAHfXaeDlNId0ETMWE/N5c=");
  });

  it("o usuário é o vencimento em unix e quem pediu, e a senha é o HMAC dele", async () => {
    const turn = await sharedSecretCredentials(
      ["turn:187.53.103.68:3478?transport=udp", "stun:187.53.103.68:3478", "turn:187.53.103.68:53"],
      SECRET,
      "123456789012345678",
      NOW,
    );
    expect(turn).toEqual({
      // Só as URLs TURN, sem a porta 53 (como nos outros provedores).
      urls: ["turn:187.53.103.68:3478?transport=udp"],
      username: `${NOW / 1000 + TURN_TTL_S}:123456789012345678`,
      credential: "/izOcy2uJirc48HkBZypzBM8aNg=",
    });
  });

  it("sem URL TURN ou sem segredo, não há credencial", async () => {
    expect(await sharedSecretCredentials(["stun:a:3478"], SECRET, "x", NOW)).toBeNull();
    expect(await sharedSecretCredentials(["turn:a:3478"], "", "x", NOW)).toBeNull();
  });

  it("com o segredo, ele vale antes do usuário e senha fixos, sem chamar ninguém", async () => {
    const env = { ...COTURN, TURN_USERNAME: "fixo", TURN_CREDENTIAL: "senha-fixa" };
    const provider = turnProvider(env, noFetch, () => NOW)!;
    expect(provider.perUser).toBe(true);
    const turn = await provider("123456789012345678");
    expect(turn?.username).toBe("1700086400:123456789012345678");
    expect(turn?.credential).toBe("/izOcy2uJirc48HkBZypzBM8aNg=");
    expect(turn?.urls).toEqual(["turn:187.53.103.68:3478?transport=udp", "turn:187.53.103.68:3478?transport=tcp"]);
    expect(noFetch).not.toHaveBeenCalled();
    // Sem o segredo, o par fixo continua valendo como antes.
    const fixo = turnProvider({ TURN_URLS: URLS, TURN_USERNAME: "fixo", TURN_CREDENTIAL: "senha-fixa" }, noFetch);
    expect(fixo?.perUser).toBeUndefined();
    expect((await fixo!())?.username).toBe("fixo");
    // Segredo sem as URLs não forma provedor.
    expect(turnProvider({ TURN_SHARED_SECRET: SECRET }, noFetch)).toBeNull();
  });

  it("Cloudflare e Metered, se configurados, continuam antes do coturn", () => {
    const cloudflare = turnProvider({ ...COTURN, TURN_KEY_ID: "a", TURN_KEY_API_TOKEN: "b" }, noFetch);
    expect(cloudflare?.perUser).toBeUndefined();
    const metered = turnProvider({ ...COTURN, METERED_DOMAIN: "a.metered.live", METERED_API_KEY: "k" }, noFetch);
    expect(metered?.perUser).toBeUndefined();
  });

  it("cada membro recebe a sua credencial, e uma nova a cada entrada", async () => {
    let now = NOW;
    const issue = turnIssuer(COTURN, noFetch, () => now);
    const ana = await issue("111111111111111111");
    const beto = await issue("2222222222222222");
    expect(ana?.username).toBe("1700086400:111111111111111111");
    expect(beto?.username).toBe("1700086400:2222222222222222");
    expect(ana?.credential).not.toBe(beto?.credential);
    // Entrando de novo mais tarde, o vencimento acompanha (a mesma validade de 24 h de hoje).
    now += 60 * 60 * 1000;
    expect((await issue("111111111111111111"))?.username).toBe("1700090000:111111111111111111");
  });

  it("os provedores de fora seguem com uma credencial por sala, guardada", async () => {
    const fetcher = vi.fn(async () => Response.json(RESPONSE, { status: 201 }));
    const issue = turnIssuer({ TURN_KEY_ID: "a", TURN_KEY_API_TOKEN: "b" }, fetcher as unknown as typeof fetch);
    expect((await issue("ana"))?.username).toBe("u1");
    expect((await issue("beto"))?.username).toBe("u1");
    expect(fetcher).toHaveBeenCalledTimes(1);
    // Sem provedor, ninguém recebe nada.
    expect(await turnIssuer({})("ana")).toBeNull();
  });
});
