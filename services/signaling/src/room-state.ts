import {
  byteLength,
  type ClientMessage,
  type ErrorCode,
  MAX_MESSAGE_BYTES,
  type Member,
  parseClientMessage,
  PROTOCOL_VERSION,
  RATE_LIMIT_MESSAGES,
  RATE_LIMIT_WINDOW_MS,
  type Resume,
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
  /** Id do Discord conferido pela sessão (ADR 0011), se o app mandou uma válida. */
  verified?: string | null;
}

/** Perfil de um membro conferido (para os recentes e os pedidos de entrada). */
export interface VerifiedProfile {
  id: string;
  name: string;
  avatar: string | null;
}

/** Pedido de entrada esperando resposta. */
interface PendingRequest {
  from: VerifiedProfile;
  at: number;
}

/** Quanto tempo um pedido de entrada espera resposta. */
export const JOIN_REQUEST_TTL_MS = 60_000;

/**
 * Quanto tempo a vaga de quem caiu espera ele voltar (ADR 0022). Cobre o app perceber a queda
 * (até 45 s sem resposta) e as primeiras tentativas de voltar; depois disso, ele sai da sala.
 */
export const RESUME_GRACE_MS = 60_000;

/**
 * A vaga de um membro: o que ele precisa para voltar com o mesmo id depois de uma queda.
 * `until` é `null` enquanto ele está conectado, e o fim do prazo depois que caiu.
 */
export interface Seat {
  token: string;
  member: Member;
  room: string | null;
  verified: string | null;
  until: number | null;
}

/** O que a sala faz fora dela: falar com o registro de cada pessoa e guardar pedidos. */
export interface RoomEffects {
  /** Código desta sala. */
  code(): string | null;
  /** Dois membros conferidos estiveram juntos na sala. */
  met(a: VerifiedProfile, b: VerifiedProfile): void;
  /** Resposta a um pedido de entrada, para o registro de quem pediu. */
  joinAnswered(
    requester: string,
    result: { kind: "approved"; code: string; room: string | null; by: string } | { kind: "denied"; by: string },
  ): void;
  getRequests(): Record<string, PendingRequest>;
  putRequests(requests: Record<string, PendingRequest>): void;
  /** Vagas dos membros, por id (ficam guardadas mesmo se a sala reiniciar). */
  getSeats(): Record<string, Seat>;
  putSeats(seats: Record<string, Seat>): void;
  /** Pede um `expire` no instante `at` (no Durable Object, um alarme). */
  wakeAt(at: number): void;
}

/** Sem registro de pessoas (testes antigos e Worker sem amigos). */
const NO_EFFECTS: RoomEffects = {
  code: () => null,
  met: () => {},
  joinAnswered: () => {},
  getRequests: () => ({}),
  putRequests: () => {},
  getSeats: () => ({}),
  putSeats: () => {},
  wakeAt: () => {},
};

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
  /** Entrou por um código de sala que não existe (`room-not-found`). */
  notFound: 4404,
  rateLimited: 4429,
  /** O mesmo membro voltou por outra conexão (a antiga ficou pendurada). */
  replaced: 4410,
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
    private readonly effects: RoomEffects = NO_EFFECTS,
    private readonly newToken: () => string = randomResumeToken,
  ) {}

  /** Uma conexão nova foi aceita. */
  open(conn: Connection): void {
    conn.setAttachment({ member: null, rateWindowStart: this.now(), rateCount: 0 });
  }

  /**
   * Chegou uma mensagem de `conn`. `verified` é o id do Discord da sessão que veio no `join`,
   * já conferida por quem chama (a conferência é assíncrona e esta lógica não).
   */
  message(conn: Connection, raw: string | ArrayBuffer, verified: string | null = null): void {
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
      return this.join(conn, { ...attachment, verified }, msg);
    }
    if (me === null) {
      conn.send({ type: "error", code: "not-joined" });
      return;
    }

    switch (msg.type) {
      case "status": {
        const member = {
          ...me,
          live: msg.live,
          muted: msg.muted,
          deafened: msg.deafened,
          watching: msg.watching,
          watchingAll: msg.watchingAll,
          effect: msg.effect,
          arcade: msg.arcade,
        };
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
        let discord = msg.discord === undefined ? me.discord : msg.discord;
        // Conferido pela sessão: a conta não pode virar a de outra pessoa.
        if (discord && attachment.verified && discord.id !== attachment.verified) discord = me.discord;
        const member = { ...me, name: msg.name, discord };
        conn.setAttachment({ ...attachment, member });
        this.broadcast({ type: "member-updated", member }, conn);
        return;
      }
      case "signal": {
        const target = this.joined().find(([, m]) => m.id === msg.to && m.id !== me.id);
        if (!target) {
          // Quem caiu e ainda pode voltar não é desconhecido: o sinal só se perde (o app tenta de novo).
          if (!this.ghosts().some((s) => s.member.id === msg.to)) conn.send({ type: "error", code: "unknown-peer" });
          return;
        }
        target[0].send({ type: "signal", from: me.id, payload: msg.payload });
        return;
      }
      case "verify": {
        // Sessão que não confere (ou de outra conta já conferida): ignora.
        if (!verified || (attachment.verified && attachment.verified !== verified)) return;
        const avatar = me.discord?.id === verified ? me.discord.avatar : null;
        const member = { ...me, discord: { id: verified, avatar }, verified: true };
        conn.setAttachment({ ...attachment, member, verified });
        this.broadcast({ type: "member-updated", member }, conn);
        this.meetOthers(conn, { id: verified, name: me.name, avatar });
        return;
      }
      case "join-answer": {
        const requests = this.liveRequests();
        const request = requests[msg.id];
        if (!request) return;
        delete requests[msg.id];
        this.effects.putRequests(requests);
        const code = this.effects.code();
        this.effects.joinAnswered(
          request.from.id,
          msg.accept && code
            ? { kind: "approved", code, room: attachment.room ?? null, by: me.name }
            : { kind: "denied", by: me.name },
        );
        for (const [c] of this.joined()) c.send({ type: "join-request-closed", id: msg.id });
        return;
      }
      case "leave":
        this.leave(conn);
        conn.close(CLOSE.normal, "leave");
        return;
    }
  }

  /**
   * A conexão caiu ou foi fechada sem `leave`. O membro continua na sala para os outros até o fim
   * do prazo (ADR 0022): se voltar com o token, nada muda para ninguém; se não, sai no `expire`.
   */
  closed(conn: Connection): void {
    const attachment = conn.getAttachment();
    const me = attachment?.member;
    if (!attachment || !me) return;
    conn.setAttachment({ ...attachment, member: null });
    const seats = this.effects.getSeats();
    const seat = seats[me.id];
    if (!seat) {
      this.broadcast({ type: "member-left", id: me.id }, conn);
      return;
    }
    seats[me.id] = {
      ...seat,
      member: me,
      room: attachment.room ?? null,
      verified: attachment.verified ?? null,
      until: this.now() + RESUME_GRACE_MS,
    };
    this.effects.putSeats(seats);
    this.scheduleExpiry(seats);
  }

  /**
   * Fim do prazo: quem caiu e não voltou sai da sala. Vagas sem conexão e sem prazo (a sala
   * reiniciou e as conexões sumiram sem aviso) ganham o prazo a partir de agora.
   */
  expire(): void {
    const seats = this.effects.getSeats();
    const connected = new Set(this.joined().map(([, m]) => m.id));
    const now = this.now();
    const left: string[] = [];
    let changed = false;
    for (const [id, seat] of Object.entries(seats)) {
      if (connected.has(id)) continue;
      if (seat.until === null) {
        seats[id] = { ...seat, until: now + RESUME_GRACE_MS };
        changed = true;
      } else if (seat.until <= now) {
        delete seats[id];
        left.push(id);
        changed = true;
      }
    }
    if (changed) this.effects.putSeats(seats);
    for (const id of left) {
      for (const [c] of this.joined()) c.send({ type: "member-left", id });
    }
    // Sempre reagenda: um alarme que dispara um pouco antes do prazo não pode deixar a vaga
    // (o "clone" de quem caiu) para sempre na sala.
    this.scheduleExpiry(seats);
  }

  /** Sai da sala na hora (com `leave` ou por erro): sem prazo para voltar. */
  private leave(conn: Connection): void {
    const attachment = conn.getAttachment();
    const me = attachment?.member;
    if (!attachment || !me) return;
    conn.setAttachment({ ...attachment, member: null });
    const seats = this.effects.getSeats();
    if (seats[me.id]) {
      delete seats[me.id];
      this.effects.putSeats(seats);
    }
    this.broadcast({ type: "member-left", id: me.id }, conn);
  }

  /** Acorda a sala no fim do prazo mais próximo. */
  private scheduleExpiry(seats: Record<string, Seat>): void {
    const next = Math.min(...Object.values(seats).map((s) => s.until ?? Infinity));
    if (Number.isFinite(next)) this.effects.wakeAt(next);
  }

  /** Quem caiu e ainda está no prazo para voltar (continua na sala para os outros). */
  private ghosts(): Seat[] {
    const connected = new Set(this.joined().map(([, m]) => m.id));
    return Object.entries(this.effects.getSeats())
      .filter(([id]) => !connected.has(id))
      .map(([, seat]) => seat);
  }

  private join(conn: Connection, attachment: Attachment, msg: Extract<ClientMessage, { type: "join" }>): void {
    const { name, protocol, room } = msg;
    // Com sessão conferida, a conta do Discord é a da sessão (a foto só vale se for da mesma conta).
    const verified = attachment.verified ?? null;
    const discord = verified
      ? { id: verified, avatar: msg.discord?.id === verified ? msg.discord.avatar : null }
      : msg.discord;
    if (protocol !== PROTOCOL_VERSION) return this.fail(conn, "version-mismatch", CLOSE.versionMismatch);

    // Volta pelo token; sem ele (app reaberto), a mesma conta conferida retoma a vaga que caiu.
    const back = this.resumable(msg.resume, verified) ?? this.ghostOf(verified);
    if (back) {
      // A conexão antiga pode estar pendurada (a queda ainda não chegou aqui): esta a substitui.
      for (const [c, m] of this.joined()) {
        if (m.id !== back.member.id || c.key === conn.key) continue;
        const a = c.getAttachment();
        if (a) c.setAttachment({ ...a, member: null });
        c.close(CLOSE.replaced, "replaced");
      }
    }
    // Vagas que ficaram de uma sala reiniciada ganham prazo; as vencidas saem.
    this.expire();

    // Sem limite de pessoas (ADR 0012): a sala aceita quem chegar.
    const others = this.joined().filter(([c]) => c.key !== conn.key);
    const ghosts = this.ghosts().filter((s) => s.member.id !== back?.member.id);

    // Entrando por um código, a sala precisa existir: sem ninguém nela (nem quem caiu e ainda pode
    // voltar), o código não é de sala nenhuma. Antes, qualquer código criava uma sala vazia.
    if (msg.create === false && !back && others.length === 0 && ghosts.length === 0) {
      return this.fail(conn, "room-not-found", CLOSE.notFound);
    }

    let id = back?.member.id;
    if (!id) {
      const taken = new Set([...others.map(([, m]) => m.id), ...Object.keys(this.effects.getSeats())]);
      id = this.newPeerId();
      while (taken.has(id)) id = this.newPeerId();
    }

    // Quem cria a sala (entra nela vazia) escolhe o nome; quem chega depois recebe o nome atual.
    const current =
      [...others.map(([c]) => c.getAttachment()?.room), ...ghosts.map((s) => s.room)].find((r) => r) ?? null;
    const roomName = current ?? (others.length === 0 ? room : null);
    // Quem volta continua como estava (ao vivo, mudo...) até o próximo `status` dele.
    const member: Member = back
      ? { ...back.member, name, discord, verified: !!verified }
      : { id, name, live: false, muted: false, deafened: false, discord, verified: !!verified };
    const token = back?.token ?? this.newToken();
    const seats = this.effects.getSeats();
    // Só guarda a vaga de quem sabe voltar para ela; a de um app antigo some na queda, como antes.
    if (msg.resumable) seats[id] = { token, member, room: roomName, verified, until: null };
    else delete seats[id];
    this.effects.putSeats(seats);

    conn.setAttachment({ ...attachment, member, room: roomName });
    conn.send({
      type: "welcome",
      you: id,
      members: [...others.map(([, m]) => m), ...ghosts.map((s) => s.member)],
      room: roomName,
      resume: token,
    });
    // Para os outros, quem voltou nunca saiu: só uma atualização (sem som de entrada).
    this.broadcast(back ? { type: "member-updated", member } : { type: "member-joined", member }, conn);

    if (verified) this.meetOthers(conn, { id: verified, name, avatar: discord?.avatar ?? null });
    // Quem chega vê os pedidos de entrada ainda abertos.
    for (const [requestId, r] of Object.entries(this.liveRequests())) {
      conn.send({ type: "join-request", id: requestId, from: r.from });
    }
  }

  /** A vaga que `resume` pede, se o token confere, ainda está no prazo e é da mesma conta. */
  private resumable(resume: Resume | null, verified: string | null): { member: Member; token: string } | null {
    const seat = resume ? this.effects.getSeats()[resume.id] : undefined;
    if (!seat || seat.token !== resume?.token) return null;
    if (seat.until !== null && seat.until <= this.now()) return null;
    if (seat.verified && seat.verified !== verified) return null;
    return { member: seat.member, token: seat.token };
  }

  /** A vaga de quem caiu com a conta conferida `verified`, se ainda está no prazo. */
  private ghostOf(verified: string | null): { member: Member; token: string } | null {
    if (!verified) return null;
    const now = this.now();
    const seat = this.ghosts().find((s) => s.verified === verified && (s.until === null || s.until > now));
    return seat ? { member: seat.member, token: seat.token } : null;
  }

  /** Recentes: quem é conferido "conhece" os outros conferidos da sala. */
  private meetOthers(conn: Connection, me: VerifiedProfile): void {
    const seen = new Set([me.id]);
    for (const [c, m] of this.joined()) {
      if (c.key === conn.key) continue;
      const other = c.getAttachment()?.verified;
      if (!other || seen.has(other)) continue;
      seen.add(other);
      this.effects.met(me, { id: other, name: m.name, avatar: m.discord?.id === other ? m.discord.avatar : null });
    }
  }

  /**
   * Um amigo de alguém da sala quer entrar: o pedido aparece para todos os membros, e o
   * primeiro que responder decide. `false` se a sala está vazia.
   */
  requestJoin(from: VerifiedProfile, requestId: string): boolean {
    const members = this.joined();
    if (members.length === 0) return false;
    const requests = this.liveRequests();
    // Um pedido por pessoa: pedir de novo troca o antigo.
    for (const [rid, r] of Object.entries(requests)) {
      if (r.from.id === from.id) {
        delete requests[rid];
        for (const [c] of members) c.send({ type: "join-request-closed", id: rid });
      }
    }
    requests[requestId] = { from, at: this.now() };
    this.effects.putRequests(requests);
    for (const [c] of members) c.send({ type: "join-request", id: requestId, from });
    return true;
  }

  /** Pedidos ainda no prazo (os vencidos somem de todos). */
  private liveRequests(): Record<string, PendingRequest> {
    const requests = this.effects.getRequests();
    let changed = false;
    for (const [rid, r] of Object.entries(requests)) {
      if (this.now() - r.at >= JOIN_REQUEST_TTL_MS) {
        delete requests[rid];
        changed = true;
        for (const [c] of this.joined()) c.send({ type: "join-request-closed", id: rid });
      }
    }
    if (changed) this.effects.putRequests(requests);
    return requests;
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
    // Quem é fechado por erro também sai da sala para os outros, sem prazo para voltar.
    this.leave(conn);
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

/** Token de retomada: 32 caracteres hex aleatórios (só quem recebeu o `welcome` conhece). */
export function randomResumeToken(): string {
  return randomPeerId() + randomPeerId();
}
