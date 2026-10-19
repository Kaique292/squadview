import { DurableObject } from "cloudflare:workers";
import { generateRoomCode } from "./room-code";
import { route } from "./router";

interface Env {
  ROOMS: DurableObjectNamespace<Room>;
}

/**
 * Esqueleto da Fase 0. Na Fase 2 cada sala vira um ponto de encontro por WebSocket,
 * onde os membros trocam candidatos de endereço para o hole punching.
 * A lógica testável fica em router.ts e room-code.ts; aqui só fica a ligação com a Cloudflare.
 */
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const r = route(request.method, new URL(request.url).pathname);
    switch (r.kind) {
      case "create-room":
        return Response.json({ code: generateRoomCode() }, { status: 201 });
      case "room":
        return env.ROOMS.get(env.ROOMS.idFromName(r.code)).fetch(request);
      case "not-found":
        return new Response("Not found", { status: 404 });
    }
  },
};

export class Room extends DurableObject<Env> {
  async fetch(): Promise<Response> {
    return new Response("Sinalização chega na Fase 2.", { status: 501 });
  }
}
