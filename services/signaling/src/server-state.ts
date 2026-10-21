import { generateRoomCode } from "./room-code";
import type { Profile } from "./session";

/**
 * Um servidor (ver docs/adr/0016-servidores-e-salas-fixas.md): nome, ícone, membros com papel,
 * salas com código fixo e convites. Sem depender do runtime da Cloudflare: o Durable Object
 * `Server` liga isto ao armazenamento e avisa o registro de cada membro.
 */

export const MAX_SERVER_MEMBERS = 100;
export const MAX_SERVER_ROOMS = 20;
export const MAX_SERVERS_PER_USER = 20;
export const MAX_INVITES = 20;
export const INVITE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
/** Ícone em data URL (96 px; o app comprime para caber, dentro do limite de 16 KB por mensagem). */
export const MAX_ICON_LENGTH = 12 * 1024;

export type Role = "owner" | "admin" | "member";

export interface ServerRoom {
  id: string;
  name: string;
  /** Código de sala comum: entrar na sala é entrar nele. */
  code: string;
}

export interface ServerMember extends Profile {
  role: Role;
}

export interface ServerSnapshot {
  id: string;
  name: string;
  icon: string | null;
  rooms: ServerRoom[];
  members: ServerMember[];
}

/** O que alguém pode pedir ao servidor (já conferido quem é pelo registro dele). */
export type ServerOp =
  | { kind: "update"; name?: string; icon?: string | null }
  | { kind: "delete" }
  | { kind: "leave" }
  | { kind: "room-create"; name: string }
  | { kind: "room-rename"; room: string; name: string }
  | { kind: "room-delete"; room: string }
  | { kind: "invite" }
  | { kind: "join"; secret: string }
  | { kind: "kick"; member: string }
  | { kind: "role"; member: string; role: "admin" | "member" };

export type ServerError =
  | "not-member"
  | "not-allowed"
  | "unknown-room"
  | "unknown-member"
  | "invite-invalid"
  | "server-full"
  | "too-many"
  | "owner-cannot-leave";

/**
 * Resultado de um pedido: o servidor como ficou (para avisar os membros), quem saiu dele (para
 * tirar da lista dessas pessoas) e, num convite, o segredo criado.
 */
export type ServerResult =
  { ok: true; snapshot: ServerSnapshot | null; removed: string[]; invite?: string } | { ok: false; error: ServerError };

export interface ServerStore {
  get<T>(key: string): T | undefined;
  put(key: string, value: unknown): void;
  delete(key: string): void;
}

interface Info {
  id: string;
  name: string;
  icon: string | null;
}

interface Invite {
  by: string;
  expires: number;
}

export interface ServerRandom {
  /** Id de sala (8 hex). */
  roomId(): string;
  roomCode(): string;
  /** Segredo de convite (12 letras e números). */
  secret(): string;
}

const ALNUM = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";

function randomString(length: number, alphabet: string): string {
  const bytes = crypto.getRandomValues(new Uint32Array(length));
  return [...bytes].map((n) => alphabet[n % alphabet.length]).join("");
}

export const secureRandom: ServerRandom = {
  roomId: () => randomString(8, "0123456789abcdef"),
  roomCode: () => generateRoomCode(),
  secret: () => randomString(12, ALNUM),
};

export const SERVER_ID = /^[0-9a-f]{16}$/;
export const ROOM_ID = /^[0-9a-f]{8}$/;
export const INVITE_SECRET = /^[A-Za-z0-9]{12}$/;
const ICON = /^data:image\/(webp|png|jpeg);base64,[A-Za-z0-9+/]+={0,2}$/;

/** Ícone aceito: data URL de imagem até o limite, ou `null` (sem ícone). `undefined` se inválido. */
export function cleanIcon(value: unknown): string | null | undefined {
  if (value === null) return null;
  if (typeof value !== "string" || value.length > MAX_ICON_LENGTH || !ICON.test(value)) return undefined;
  return value;
}

export class ServerState {
  constructor(
    private readonly store: ServerStore,
    private readonly now: () => number = Date.now,
    private readonly random: ServerRandom = secureRandom,
  ) {}

  private info(): Info | undefined {
    return this.store.get<Info>("info");
  }
  private members(): Record<string, ServerMember> {
    return this.store.get<Record<string, ServerMember>>("members") ?? {};
  }
  private rooms(): ServerRoom[] {
    return this.store.get<ServerRoom[]>("rooms") ?? [];
  }
  private invites(): Record<string, Invite> {
    return this.store.get<Record<string, Invite>>("invites") ?? {};
  }

  /** Cria o servidor com o dono e uma sala "Geral" (vazio, ninguém teria onde entrar). */
  create(id: string, name: string, owner: Profile): ServerSnapshot {
    this.store.put("info", { id, name, icon: null } satisfies Info);
    this.store.put("members", { [owner.id]: { ...owner, role: "owner" } });
    this.store.put("rooms", [{ id: this.random.roomId(), name: "Geral", code: this.random.roomCode() }]);
    this.store.put("invites", {});
    return this.snapshot()!;
  }

  /** Como o servidor está (`null` se não existe ou foi apagado). Dono primeiro, depois por nome. */
  snapshot(): ServerSnapshot | null {
    const info = this.info();
    if (!info) return null;
    const order: Record<Role, number> = { owner: 0, admin: 1, member: 2 };
    const members = Object.values(this.members()).sort(
      (a, b) => order[a.role] - order[b.role] || a.name.localeCompare(b.name, "pt-BR"),
    );
    return { ...info, rooms: this.rooms(), members };
  }

  /** Ids de todos os membros (para avisar quando algo muda). */
  memberIds(): string[] {
    return Object.keys(this.members());
  }

  /** Mantém o nome e a foto de um membro em dia (ele mudou no Discord). */
  refreshProfile(profile: Profile): boolean {
    const m = this.members()[profile.id];
    if (!m || (m.name === profile.name && m.avatar === profile.avatar)) return false;
    this.update<Record<string, ServerMember>>("members", {}, (all) => (all[profile.id] = { ...m, ...profile }));
    return true;
  }

  /** Um pedido de `actor` (já conferido pelo registro dele). */
  act(actor: Profile, op: ServerOp): ServerResult {
    if (!this.info()) return { ok: false, error: "not-member" };
    if (op.kind === "join") return this.join(actor, op.secret);

    const me = this.members()[actor.id];
    if (!me) return { ok: false, error: "not-member" };
    const admin = me.role === "owner" || me.role === "admin";

    switch (op.kind) {
      case "update": {
        if (!admin) return { ok: false, error: "not-allowed" };
        this.update<Info>("info", this.info()!, (info) => {
          if (op.name !== undefined) info.name = op.name;
          if (op.icon !== undefined) info.icon = op.icon;
        });
        return this.changed();
      }
      case "delete": {
        if (me.role !== "owner") return { ok: false, error: "not-allowed" };
        const removed = this.memberIds();
        for (const key of ["info", "members", "rooms", "invites"]) this.store.delete(key);
        return { ok: true, snapshot: null, removed };
      }
      case "leave": {
        // O dono não sai: apaga o servidor (ou, no futuro, passa para outra pessoa).
        if (me.role === "owner") return { ok: false, error: "owner-cannot-leave" };
        this.update<Record<string, ServerMember>>("members", {}, (all) => delete all[actor.id]);
        return this.changed([actor.id]);
      }
      case "room-create": {
        if (!admin) return { ok: false, error: "not-allowed" };
        if (this.rooms().length >= MAX_SERVER_ROOMS) return { ok: false, error: "too-many" };
        this.update<ServerRoom[]>("rooms", [], (rooms) =>
          rooms.push({ id: this.random.roomId(), name: op.name, code: this.random.roomCode() }),
        );
        return this.changed();
      }
      case "room-rename": {
        if (!admin) return { ok: false, error: "not-allowed" };
        if (!this.rooms().some((r) => r.id === op.room)) return { ok: false, error: "unknown-room" };
        this.update<ServerRoom[]>("rooms", [], (rooms) => {
          for (const r of rooms) if (r.id === op.room) r.name = op.name;
        });
        return this.changed();
      }
      case "room-delete": {
        if (!admin) return { ok: false, error: "not-allowed" };
        const rooms = this.rooms();
        if (!rooms.some((r) => r.id === op.room)) return { ok: false, error: "unknown-room" };
        // Sempre fica ao menos uma sala: sem nenhuma, o servidor não teria onde entrar.
        if (rooms.length === 1) return { ok: false, error: "not-allowed" };
        this.store.put(
          "rooms",
          rooms.filter((r) => r.id !== op.room),
        );
        return this.changed();
      }
      case "invite": {
        // Qualquer membro chama gente, como no Discord.
        const invites = this.liveInvites();
        if (Object.keys(invites).length >= MAX_INVITES) return { ok: false, error: "too-many" };
        const secret = this.random.secret();
        invites[secret] = { by: actor.id, expires: this.now() + INVITE_TTL_MS };
        this.store.put("invites", invites);
        return { ok: true, snapshot: this.snapshot(), removed: [], invite: secret };
      }
      case "kick": {
        const target = this.members()[op.member];
        if (!target) return { ok: false, error: "unknown-member" };
        // Administrador tira membros; só o dono tira administradores; ninguém tira o dono.
        const canKick = target.role === "member" ? admin : target.role === "admin" && me.role === "owner";
        if (!canKick || target.id === actor.id) return { ok: false, error: "not-allowed" };
        this.update<Record<string, ServerMember>>("members", {}, (all) => delete all[op.member]);
        return this.changed([op.member]);
      }
      case "role": {
        if (me.role !== "owner") return { ok: false, error: "not-allowed" };
        const target = this.members()[op.member];
        if (!target) return { ok: false, error: "unknown-member" };
        if (target.role === "owner") return { ok: false, error: "not-allowed" };
        this.update<Record<string, ServerMember>>("members", {}, (all) => (all[op.member].role = op.role));
        return this.changed();
      }
    }
  }

  private join(actor: Profile, secret: string): ServerResult {
    if (this.members()[actor.id]) return { ok: true, snapshot: this.snapshot(), removed: [] };
    const invite = this.liveInvites()[secret];
    if (!invite) return { ok: false, error: "invite-invalid" };
    if (this.memberIds().length >= MAX_SERVER_MEMBERS) return { ok: false, error: "server-full" };
    this.update<Record<string, ServerMember>>("members", {}, (all) => (all[actor.id] = { ...actor, role: "member" }));
    return this.changed();
  }

  /** Convites que ainda valem (os vencidos são apagados aqui). */
  private liveInvites(): Record<string, Invite> {
    const now = this.now();
    const all = this.invites();
    const live = Object.fromEntries(Object.entries(all).filter(([, i]) => i.expires > now));
    if (Object.keys(live).length !== Object.keys(all).length) this.store.put("invites", live);
    return live;
  }

  private changed(removed: string[] = []): ServerResult {
    return { ok: true, snapshot: this.snapshot(), removed };
  }

  private update<T>(key: string, fallback: T, change: (value: T) => void): void {
    const value = this.store.get<T>(key) ?? fallback;
    change(value);
    this.store.put(key, value);
  }
}
