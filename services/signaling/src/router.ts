import { isValidRoomCode } from "./room-code";

export type Route =
  | { kind: "create-room" }
  | { kind: "room"; code: string }
  | { kind: "auth-discord" }
  | { kind: "me" }
  | { kind: "not-found" };

/** Decide o que fazer com uma requisição, sem depender do runtime da Cloudflare. */
export function route(method: string, pathname: string): Route {
  if (method === "POST" && pathname === "/rooms") return { kind: "create-room" };
  // Amigos e presença (ADR 0011).
  if (method === "POST" && pathname === "/auth/discord") return { kind: "auth-discord" };
  if (method === "GET" && pathname === "/me") return { kind: "me" };

  const match = pathname.match(/^\/rooms\/([A-Z0-9]+)$/);
  if (match && isValidRoomCode(match[1])) return { kind: "room", code: match[1] };

  return { kind: "not-found" };
}

/** Lê a sessão do cabeçalho `Authorization: Bearer <sessão>` (nunca da URL, que vai para logs). */
export function bearer(header: string | null): string | null {
  const match = header?.match(/^Bearer ([A-Za-z0-9_.-]{1,512})$/);
  return match ? match[1] : null;
}
