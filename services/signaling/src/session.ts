/**
 * Sessão do SquadView (ver docs/adr/0011-amigos-e-presenca.md): prova que o app é mesmo da
 * pessoa com aquele id do Discord. É `{id, exp}` em base64url, um ponto e o HMAC-SHA256 disso.
 */

/** Validade de uma sessão; cada conexão ganha uma nova. */
export const SESSION_TTL_MS = 90 * 24 * 60 * 60 * 1000;
/** Tamanho máximo de uma sessão (texto). */
export const MAX_SESSION_LENGTH = 512;

const DISCORD_API = "https://discord.com/api/v10";
const DISCORD_ID_PATTERN = /^[0-9]{15,21}$/;
const DISCORD_AVATAR_PATTERN = /^(a_)?[0-9a-f]{32}$/;
/** Maior nome que guardamos (o Discord limita a 32). */
const MAX_NAME = 32;

/** Perfil público de alguém, como o SquadView mostra. */
export interface Profile {
  id: string;
  name: string;
  avatar: string | null;
}

const encoder = new TextEncoder();

function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): Uint8Array | null {
  if (!/^[A-Za-z0-9_-]*$/.test(text)) return null;
  try {
    const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/"));
    return Uint8Array.from(binary, (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}

async function key(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, [
    "sign",
    "verify",
  ]);
}

/** Cria uma sessão para `id`, válida até `now + SESSION_TTL_MS`. */
export async function signSession(secret: string, id: string, now = Date.now()): Promise<string> {
  const payload = toBase64Url(encoder.encode(JSON.stringify({ id, exp: now + SESSION_TTL_MS })));
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", await key(secret), encoder.encode(payload)));
  return `${payload}.${toBase64Url(sig)}`;
}

/** Confere a sessão e devolve o id do Discord, ou `null` se for falsa, malformada ou vencida. */
export async function verifySession(secret: string, session: unknown, now = Date.now()): Promise<string | null> {
  if (typeof session !== "string" || session.length > MAX_SESSION_LENGTH || !secret) return null;
  const [payload, sigText, ...rest] = session.split(".");
  if (!payload || !sigText || rest.length > 0) return null;
  const sig = fromBase64Url(sigText);
  if (!sig) return null;
  // `verify` compara em tempo constante.
  const ok = await crypto.subtle.verify("HMAC", await key(secret), sig, encoder.encode(payload));
  if (!ok) return null;
  const bytes = fromBase64Url(payload);
  if (!bytes) return null;
  try {
    const data = JSON.parse(new TextDecoder().decode(bytes)) as { id?: unknown; exp?: unknown };
    if (typeof data.id !== "string" || !DISCORD_ID_PATTERN.test(data.id)) return null;
    if (typeof data.exp !== "number" || data.exp <= now) return null;
    return data.id;
  } catch {
    return null;
  }
}

/** Nome para mostrar, limpo como os nomes da sala (sem controle, até 32 caracteres). */
export function cleanProfileName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  // eslint-disable-next-line no-control-regex
  const name = value.replace(/[\u0000-\u001f\u007f-\u009f]/g, "").trim();
  const chars = [...name];
  if (chars.length === 0) return null;
  return chars.slice(0, MAX_NAME).join("");
}

/** Hash da foto do Discord, ou `null` (foto padrão) se vier vazio ou fora do formato. */
export function cleanAvatar(value: unknown): string | null {
  return typeof value === "string" && DISCORD_AVATAR_PATTERN.test(value) ? value : null;
}

/**
 * Troca o token do Discord (de quem acabou de entrar) por uma sessão do SquadView. Lê o perfil
 * em `/users/@me` e não guarda o token. `null` se o Discord recusar o token.
 */
export async function exchangeDiscordToken(
  secret: string,
  accessToken: unknown,
  fetcher: typeof fetch = fetch,
  now = Date.now(),
): Promise<{ session: string; profile: Profile } | null> {
  if (typeof accessToken !== "string" || accessToken.length === 0 || accessToken.length > 256) return null;
  if (!/^[A-Za-z0-9._~-]+$/.test(accessToken)) return null;
  const response = await fetcher(`${DISCORD_API}/users/@me`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!response.ok) return null;
  const user = (await response.json()) as Record<string, unknown>;
  if (typeof user.id !== "string" || !DISCORD_ID_PATTERN.test(user.id)) return null;
  const name = cleanProfileName(user.global_name) ?? cleanProfileName(user.username) ?? "Sem nome";
  const profile: Profile = { id: user.id, name, avatar: cleanAvatar(user.avatar) };
  return { session: await signSession(secret, user.id, now), profile };
}
