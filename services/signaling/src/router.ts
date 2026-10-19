import { isValidRoomCode } from "./room-code";

export type Route = { kind: "create-room" } | { kind: "room"; code: string } | { kind: "not-found" };

/** Decide o que fazer com uma requisição, sem depender do runtime da Cloudflare. */
export function route(method: string, pathname: string): Route {
  if (method === "POST" && pathname === "/rooms") return { kind: "create-room" };

  const match = pathname.match(/^\/rooms\/([A-Z0-9]+)$/);
  if (match && isValidRoomCode(match[1])) return { kind: "room", code: match[1] };

  return { kind: "not-found" };
}
