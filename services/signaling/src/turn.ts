/**
 * Repassador TURN (ADR 0014): só para quem não conecta direto (CGNAT). O Worker consegue
 * credenciais e manda para quem entra na sala; o vídeo passa pelo repassador só quando o caminho
 * direto não fecha.
 *
 * O provedor é plugável e escolhido pelos segredos configurados (o primeiro que estiver completo):
 * - Cloudflare Realtime: `TURN_KEY_ID` e `TURN_KEY_API_TOKEN` (pago por uso, 1.000 GB/mês grátis);
 * - Metered "Open Relay": `METERED_DOMAIN` (ex.: `squadview.metered.live`) e `METERED_API_KEY`
 *   (plano grátis de 20 GB/mês, só com cadastro);
 * - coturn próprio com credencial temporária ("TURN REST API", `use-auth-secret`): `TURN_URLS`
 *   (separadas por vírgula) e `TURN_SHARED_SECRET`, o mesmo `static-auth-secret` do coturn. Cada
 *   membro recebe a sua credencial, que vence em `TURN_TTL_S`;
 * - fixo (um coturn com usuário e senha permanentes): `TURN_URLS`, `TURN_USERNAME` e
 *   `TURN_CREDENTIAL`. Todo membro de sala recebe a mesma senha, que nunca vence: prefira o segredo.
 * Sem nenhum, a sala funciona só com conexão direta.
 */

export interface TurnCredentials {
  urls: string[];
  username: string;
  credential: string;
}

/** Quanto as credenciais duram: mais que uma sessão longa de jogo. */
export const TURN_TTL_S = 24 * 60 * 60;
/** Gera credenciais novas quando faltar menos que isso para vencerem. */
export const TURN_RENEW_BEFORE_MS = 2 * 60 * 60 * 1000;

const API = "https://rtc.live.cloudflare.com/v1/turn/keys";

/** Os segredos que escolhem o provedor (todos opcionais). */
export interface TurnEnv {
  TURN_KEY_ID?: string;
  TURN_KEY_API_TOKEN?: string;
  METERED_DOMAIN?: string;
  METERED_API_KEY?: string;
  TURN_URLS?: string;
  TURN_SHARED_SECRET?: string;
  TURN_USERNAME?: string;
  TURN_CREDENTIAL?: string;
}

/**
 * Busca credenciais no provedor configurado; `null` se não deu (a sala segue só com o direto).
 * `user` só importa para o provedor `perUser` (o coturn com segredo), que gera uma por pessoa.
 */
export type TurnProvider = ((user?: string) => Promise<TurnCredentials | null>) & { perUser?: boolean };

/** O provedor configurado, ou `null` sem nenhum. */
export function turnProvider(
  env: TurnEnv,
  fetcher: typeof fetch = fetch,
  now: () => number = Date.now,
): TurnProvider | null {
  if (env.TURN_KEY_ID && env.TURN_KEY_API_TOKEN) {
    const [id, token] = [env.TURN_KEY_ID, env.TURN_KEY_API_TOKEN];
    return () => generateTurnCredentials(id, token, fetcher);
  }
  if (env.METERED_DOMAIN && env.METERED_API_KEY) {
    const [domain, key] = [env.METERED_DOMAIN, env.METERED_API_KEY];
    return () => meteredCredentials(domain, key, fetcher);
  }
  if (env.TURN_URLS && env.TURN_SHARED_SECRET) {
    const [urls, secret] = [splitUrls(env.TURN_URLS), env.TURN_SHARED_SECRET];
    const provider: TurnProvider = (user = "") => sharedSecretCredentials(urls, secret, user, now());
    provider.perUser = true;
    return provider;
  }
  if (env.TURN_URLS && env.TURN_USERNAME && env.TURN_CREDENTIAL) {
    const fixed = parseIceServers({
      iceServers: [
        {
          urls: splitUrls(env.TURN_URLS),
          username: env.TURN_USERNAME,
          credential: env.TURN_CREDENTIAL,
        },
      ],
    });
    return async () => fixed;
  }
  return null;
}

function splitUrls(urls: string): string[] {
  return urls
    .split(",")
    .map((u) => u.trim())
    .filter(Boolean);
}

/**
 * Quem entrou na sala recebe as credenciais: por pessoa no provedor `perUser`, e as da sala (no
 * {@link TurnCache}, uma chamada por dia) nos outros. Sem provedor, sempre `null`.
 */
export function turnIssuer(
  env: TurnEnv,
  fetcher: typeof fetch = fetch,
  now: () => number = Date.now,
): (user: string) => Promise<TurnCredentials | null> {
  const provider = turnProvider(env, fetcher, now);
  if (!provider) return async () => null;
  if (provider.perUser) return (user) => provider(user);
  const cache = new TurnCache(() => provider(), now);
  return () => cache.get();
}

/**
 * Credencial temporária do coturn ("TURN REST API", `use-auth-secret`): o usuário é
 * `<vence_em_unix>:<quem>` e a senha é `base64(HMAC-SHA1(segredo, usuário))`. O coturn confere a
 * senha com o mesmo segredo e recusa depois do vencimento, então ninguém guarda uma senha que
 * sirva para sempre, e o registro do coturn mostra quem usou o repassador.
 */
export async function sharedSecretCredentials(
  urls: string[],
  secret: string,
  user: string,
  now: number = Date.now(),
): Promise<TurnCredentials | null> {
  const turnUrls = parseIceServers({ iceServers: [{ urls, username: "-", credential: "-" }] })?.urls;
  if (!turnUrls?.length || !secret) return null;
  const expires = Math.floor(now / 1000) + TURN_TTL_S;
  const username = `${expires}:${user}`;
  return { urls: turnUrls, username, credential: await hmacSha1Base64(secret, username) };
}

/** `base64(HMAC-SHA1(key, message))`, pelo Web Crypto do Worker. */
export async function hmacSha1Base64(key: string, message: string): Promise<string> {
  const encoder = new TextEncoder();
  const cryptoKey = await crypto.subtle.importKey("raw", encoder.encode(key), { name: "HMAC", hash: "SHA-1" }, false, [
    "sign",
  ]);
  const signature = new Uint8Array(await crypto.subtle.sign("HMAC", cryptoKey, encoder.encode(message)));
  return btoa(String.fromCharCode(...signature));
}

/** Metered: `GET https://<app>.metered.live/api/v1/turn/credentials?apiKey=...` (lista de iceServers). */
export async function meteredCredentials(
  domain: string,
  apiKey: string,
  fetcher: typeof fetch = fetch,
): Promise<TurnCredentials | null> {
  // Só o domínio do app (sem esquema nem caminho), para a chave não ir para outro lugar.
  if (!/^[a-z0-9-]+\.metered\.live$/i.test(domain)) return null;
  try {
    const response = await fetcher(`https://${domain}/api/v1/turn/credentials?apiKey=${encodeURIComponent(apiKey)}`);
    if (!response.ok) return null;
    return parseIceServers(await response.json());
  } catch {
    return null;
  }
}

/**
 * Pede credenciais à Cloudflare. `null` se não deu (chave errada, fora do ar): a sala segue sem
 * repassador, só com conexão direta.
 */
export async function generateTurnCredentials(
  keyId: string,
  token: string,
  fetcher: typeof fetch = fetch,
): Promise<TurnCredentials | null> {
  try {
    const response = await fetcher(`${API}/${encodeURIComponent(keyId)}/credentials/generate-ice-servers`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ ttl: TURN_TTL_S }),
    });
    if (!response.ok) return null;
    return parseIceServers(await response.json());
  } catch {
    return null;
  }
}

/**
 * Lê a lista de servidores (`{iceServers: [...]}`, `{iceServers: {...}}` ou a lista direto, como o
 * Metered manda) e junta as URLs TURN da primeira credencial (cada provedor separa de um jeito).
 */
export function parseIceServers(body: unknown): TurnCredentials | null {
  const raw = Array.isArray(body) ? body : (body as { iceServers?: unknown } | null)?.iceServers;
  const entries = (Array.isArray(raw) ? raw : raw ? [raw] : []) as {
    urls?: unknown;
    username?: unknown;
    credential?: unknown;
  }[];
  const turnUrls = (e: (typeof entries)[number]) =>
    (Array.isArray(e.urls) ? e.urls : [e.urls]).filter(
      // A porta 53 costuma ser bloqueada e só atrasa as tentativas.
      (u): u is string => typeof u === "string" && /^turns?:/.test(u) && !/:53(\?|$)/.test(u),
    );
  const first = entries.find(
    (e) => turnUrls(e).length > 0 && typeof e.username === "string" && typeof e.credential === "string",
  );
  if (!first) return null;
  const username = first.username as string;
  const credential = first.credential as string;
  const urls = entries.filter((e) => e.username === username && e.credential === credential).flatMap(turnUrls);
  return { urls: [...new Set(urls)], username, credential };
}

/** Guarda as credenciais da sala e só pede novas perto de vencer (uma chamada por sala por dia). */
export class TurnCache {
  private cached: { credentials: TurnCredentials; expires: number } | null = null;
  private pending: Promise<TurnCredentials | null> | null = null;

  constructor(
    private readonly generate: () => Promise<TurnCredentials | null>,
    private readonly now: () => number = Date.now,
  ) {}

  async get(): Promise<TurnCredentials | null> {
    if (this.cached && this.cached.expires - this.now() > TURN_RENEW_BEFORE_MS) return this.cached.credentials;
    // Várias pessoas entrando juntas: um pedido só.
    this.pending ??= this.generate().finally(() => (this.pending = null));
    const credentials = await this.pending;
    if (credentials) this.cached = { credentials, expires: this.now() + TURN_TTL_S * 1000 };
    return credentials;
  }
}
