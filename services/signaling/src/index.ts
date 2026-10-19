import { DurableObject } from "cloudflare:workers";
import { generateRoomCode } from "./room-code";
import { type Attachment, type Connection, randomPeerId, RoomState } from "./room-state";
import { route } from "./router";

interface Env {
  ROOMS: DurableObjectNamespace<Room>;
}

/**
 * Sinalização do SquadView: só ajuda os PCs a se encontrarem (nenhum vídeo passa por aqui).
 * As regras ficam em módulos puros e testados (router, room-code, protocol, room-state);
 * este arquivo só liga esses módulos ao runtime da Cloudflare.
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

/** Uma sala. Usa a Hibernation API: a sala dorme entre mensagens e não gasta tempo de CPU parada. */
export class Room extends DurableObject<Env> {
  private readonly state = new RoomState(() => this.ctx.getWebSockets().map(connection), randomPeerId);

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("Use WebSocket", { status: 426 });
    }
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    this.state.open(connection(server));
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    this.state.message(connection(ws), message);
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    this.state.closed(connection(ws));
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    this.state.closed(connection(ws));
  }
}

/** Adapta o WebSocket da Cloudflare à interface que a lógica da sala usa. */
function connection(ws: WebSocket): Connection {
  return {
    key: ws,
    send: (message) => {
      try {
        ws.send(JSON.stringify(message));
      } catch {
        // A conexão já caiu; o webSocketClose cuida da saída.
      }
    },
    close: (code, reason) => {
      try {
        ws.close(code, reason);
      } catch {
        // Já fechada.
      }
    },
    getAttachment: () => ws.deserializeAttachment() as Attachment | null,
    setAttachment: (a) => ws.serializeAttachment(a),
  };
}
