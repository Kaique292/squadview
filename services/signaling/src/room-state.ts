import {
  byteLength,
  type ClientMessage,
  type ErrorCode,
  MAX_MEMBERS,
  MAX_MESSAGE_BYTES,
  type Member,
  parseClientMessage,
  PROTOCOL_VERSION,
  RATE_LIMIT_MESSAGES,
  RATE_LIMIT_WINDOW_MS,
  type ServerMessage,
} from "./protocol";

/** O que guardamos em cada conexão (no Durable Object, vai no serializeAttachment do WebSocket). */
export interface Attachment {
  /** Preenchido depois do `join`. */
  member: Member | null;
  /**
   * Nome da sala, igual em todos os membros. Fica em cada conexão porque a sala hiberna: quando
   * o último sai, o nome vai junto (a sala é efêmera).
   */
  room?: string | null;
  rateWindowStart: number;
  rateCount: number;
}

/** Uma conexão de WebSocket, vista pela lógica da sala. */
export interface Connection {
  /**
   * Identidade da conexão (no Durable Object, o próprio WebSocket). O adaptador cria um objeto
   * novo a cada chamada, então duas conexões são a mesma quando têm a mesma `key`.
   */
  readonly key: unknown;
  send(message: ServerMessage): void;
  close(code: number, reason: string): void;
  getAttachment(): Attachment | null;
  setAttachment(attachment: Attachment): void;
}

/** Códigos de fechamento (4000–4999 são livres para a aplicação). */
export const CLOSE = {
  normal: 1000,
  invalid: 4400,
  versionMismatch: 4426,
  roomFull: 4409,
  rateLimited: 4429,
} as const;

/**
 * Regras da sala, sem depender do runtime da Cloudflare: quem entra, quem sai,
 * quem está ao vivo e o repasse de mensagens `signal` entre dois membros.
 */
export class RoomState {
  constructor(
    private readonly connections: () => Connection[],
    private readonly newPeerId: () => string,
    private readonly now: () => number = Date.now,
  ) {}

  /** Uma conexão nova foi aceita. */
  open(conn: Connection): void {
    conn.setAttachment({ member: null, rateWindowStart: this.now(), rateCount: 0 });
  }

  /** Chegou uma mensagem de `conn`. */
  message(conn: Connection, raw: string | ArrayBuffer): void {
    const attachment = this.allow(conn);
    if (attachment === null) return;

    if (typeof raw !== "string" || byteLength(raw) > MAX_MESSAGE_BYTES) {
      return this.fail(conn, "invalid-message", CLOSE.invalid);
    }
    const msg = parseClientMessage(raw);
    if (msg === null) return this.fail(conn, "invalid-message", CLOSE.invalid);

    const me = attachment.member;
    if (msg.type === "join") {
      if (me !== null) return this.fail(conn, "invalid-message", CLOSE.invalid);
      return this.join(conn, attachment, msg);
    }
    if (me === null) {
      conn.send({ type: "error", code: "not-joined" });
      return;
    }

    switch (msg.type) {
      case "status": {
        const member = { ...me, live: msg.live, muted: msg.muted, deafened: msg.deafened };
        conn.setAttachment({ ...attachment, member });
        this.broadcast({ type: "member-updated", member }, conn);
        return;
      }
      case "room": {
        // Qualquer membro pode renomear a sala (salas de amigos, sem dono): todos ficam sabendo.
        for (const [c] of this.joined()) {
          const a = c.key === conn.key ? attachment : c.getAttachment();
          if (a) c.setAttachment({ ...a, room: msg.name });
          c.send({ type: "room-updated", name: msg.name });
        }
        return;
      }
      case "rename": {
        // Trocar o apelido ou vincular/desvincular o Discord (`discord` ausente: continua a mesma conta).
        const member = { ...me, name: msg.name, discord: msg.discord === undefined ? me.discord : msg.discord };
        conn.setAttachment({ ...attachment, member });
        this.broadcast({ type: "member-updated", member }, conn);
        return;
      }
      case "signal": {
        const target = this.joined().find(([, m]) => m.id === msg.to && m.id !== me.id);
        if (!target) {
          conn.send({ type: "error", code: "unknown-peer" });
          return;
        }
        target[0].send({ type: "signal", from: me.id, payload: msg.payload });
        return;
      }
      case "leave":
        conn.setAttachment({ ...attachment, member: null });
        this.broadcast({ type: "member-left", id: me.id }, conn);
        conn.close(CLOSE.normal, "leave");
        return;
    }
  }

  /** A conexão caiu ou foi fechada pelo cliente. */
  closed(conn: Connection): void {
    const attachment = conn.getAttachment();
    const me = attachment?.member;
    if (!attachment || !me) return;
    conn.setAttachment({ ...attachment, member: null });
    this.broadcast({ type: "member-left", id: me.id }, conn);
  }

  private join(conn: Connection, attachment: Attachment, msg: Extract<ClientMessage, { type: "join" }>): void {
    const { name, protocol, room, discord } = msg;
    if (protocol !== PROTOCOL_VERSION) return this.fail(conn, "version-mismatch", CLOSE.versionMismatch);
    const others = this.joined().filter(([c]) => c.key !== conn.key);
    if (others.length >= MAX_MEMBERS) return this.fail(conn, "room-full", CLOSE.roomFull);

    const taken = new Set(others.map(([, m]) => m.id));
    let id = this.newPeerId();
    while (taken.has(id)) id = this.newPeerId();

    // Quem cria a sala (entra nela vazia) escolhe o nome; quem chega depois recebe o nome atual.
    const current = others.map(([c]) => c.getAttachment()?.room).find((r) => r) ?? null;
    const roomName = others.length === 0 ? room : current;
    const member: Member = { id, name, live: false, muted: false, deafened: false, discord };
    conn.setAttachment({ ...attachment, member, room: roomName });
    conn.send({ type: "welcome", you: id, members: others.map(([, m]) => m), room: roomName });
    this.broadcast({ type: "member-joined", member }, conn);
  }

  /**
   * Conta a mensagem na janela de taxa e devolve o estado já atualizado da conexão,
   * ou `null` se ela passou do limite (e foi fechada).
   */
  private allow(conn: Connection): Attachment | null {
    const now = this.now();
    const attachment = conn.getAttachment() ?? { member: null, rateWindowStart: now, rateCount: 0 };
    const fresh = now - attachment.rateWindowStart >= RATE_LIMIT_WINDOW_MS;
    const next: Attachment = {
      ...attachment,
      rateWindowStart: fresh ? now : attachment.rateWindowStart,
      rateCount: fresh ? 1 : attachment.rateCount + 1,
    };
    conn.setAttachment(next);
    if (next.rateCount > RATE_LIMIT_MESSAGES) {
      this.fail(conn, "rate-limited", CLOSE.rateLimited);
      return null;
    }
    return next;
  }

  private fail(conn: Connection, code: ErrorCode, closeCode: number): void {
    conn.send({ type: "error", code });
    // Quem é fechado por erro também sai da sala para os outros.
    this.closed(conn);
    conn.close(closeCode, code);
  }

  private joined(): [Connection, Member][] {
    const out: [Connection, Member][] = [];
    for (const c of this.connections()) {
      const m = c.getAttachment()?.member;
      if (m) out.push([c, m]);
    }
    return out;
  }

  private broadcast(message: ServerMessage, except: Connection): void {
    for (const [c] of this.joined()) {
      if (c.key !== except.key) c.send(message);
    }
  }
}

/** 16 caracteres hex aleatórios. */
export function randomPeerId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}
