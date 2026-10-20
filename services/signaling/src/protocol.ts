/**
 * Contrato da sinalização (ver PROTOCOL.md). O app em Rust (sv-net) segue exatamente estes tipos.
 */

export const PROTOCOL_VERSION = 1;
export const MAX_MEMBERS = 8;
export const MAX_NAME_LENGTH = 32;
/** Tamanho máximo de uma mensagem bruta, em bytes. */
export const MAX_MESSAGE_BYTES = 16 * 1024;
/** Tamanho máximo do `payload` de um `signal`, serializado, em bytes. */
export const MAX_SIGNAL_PAYLOAD_BYTES = 8 * 1024;
/** Mensagens permitidas por conexão em cada janela. */
export const RATE_LIMIT_MESSAGES = 100;
export const RATE_LIMIT_WINDOW_MS = 10_000;

export type PeerId = string;

/**
 * Conta do Discord de um membro, só para mostrar a foto: o id e o hash da foto, nunca uma URL
 * (o app monta o endereço do CDN do Discord; assim ninguém faz os outros abrirem links quaisquer).
 * O servidor não confere se a conta é mesmo da pessoa, como já acontece com o nome.
 */
export interface DiscordUser {
  id: string;
  avatar: string | null;
}

export interface Member {
  id: PeerId;
  name: string;
  live: boolean;
  /** Microfone silenciado (chat de voz). */
  muted: boolean;
  /** Ensurdecido: não ouve ninguém. */
  deafened: boolean;
  /** Conta do Discord vinculada, ou `null`. */
  discord: DiscordUser | null;
  /** A conta foi conferida pela sessão do SquadView (só conferidos podem virar amigos). */
  verified: boolean;
  /** Quem este membro está assistindo (id), ou `null`. Ausente até o primeiro `status` dele. */
  watching?: PeerId | null;
}

export type ClientMessage =
  | {
      type: "join";
      name: string;
      protocol: number;
      room: string | null;
      discord: DiscordUser | null;
      /** Sessão do SquadView (ADR 0011): o servidor confere e o id do Discord passa a valer. */
      session: string | null;
    }
  | { type: "room"; name: string }
  /** `discord` ausente: continua a mesma conta; `null`: desvinculou. */
  | { type: "rename"; name: string; discord?: DiscordUser | null }
  /** `watching`: quem estamos assistindo (para quem transmite ver quantos e quem assistem). */
  | { type: "status"; live: boolean; muted: boolean; deafened: boolean; watching: PeerId | null }
  | { type: "signal"; to: PeerId; payload: Record<string, unknown> }
  /** Resposta a um pedido de entrada de um amigo de alguém da sala. */
  | { type: "join-answer"; id: string; accept: boolean }
  /** Entrou com o Discord já dentro da sala: confere a sessão sem sair e entrar. */
  | { type: "verify"; session: string }
  | { type: "leave" };

export type ErrorCode =
  "room-full" | "invalid-message" | "version-mismatch" | "not-joined" | "unknown-peer" | "rate-limited";

export type ServerMessage =
  | { type: "welcome"; you: PeerId; members: Member[]; room: string | null }
  | { type: "room-updated"; name: string }
  | { type: "member-joined"; member: Member }
  | { type: "member-updated"; member: Member }
  | { type: "member-left"; id: PeerId }
  | { type: "signal"; from: PeerId; payload: Record<string, unknown> }
  /** Um amigo de alguém da sala quer entrar; qualquer membro aceita ou recusa. */
  | { type: "join-request"; id: string; from: { id: string; name: string; avatar: string | null } }
  /** O pedido foi respondido (por alguém) ou venceu: some de todos. */
  | { type: "join-request-closed"; id: string }
  | { type: "error"; code: ErrorCode };

const encoder = new TextEncoder();

export function byteLength(text: string): number {
  return encoder.encode(text).byteLength;
}

/** Nome de exibição: sem espaços nas pontas, sem caracteres de controle, 1 a 32 caracteres. */
export function cleanName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return null;
  const name = value.trim();
  const length = [...name].length;
  return length >= 1 && length <= MAX_NAME_LENGTH ? name : null;
}

export const PEER_ID_PATTERN = /^[0-9a-f]{16}$/;
/** Id de um pedido de entrada (como o peerId). */
export const REQUEST_ID_PATTERN = PEER_ID_PATTERN;
/** Snowflake do Discord. */
const DISCORD_ID_PATTERN = /^[0-9]{15,21}$/;
/** Hash da foto do Discord (`a_` na frente quando é animada). */
const DISCORD_AVATAR_PATTERN = /^(a_)?[0-9a-f]{32}$/;

/**
 * Lê o campo `discord`: `undefined` se veio ausente, `null` se veio nulo, a conta se for válida.
 * Devolve `false` quando o campo existe mas não segue o formato.
 */
function parseDiscord(value: unknown): DiscordUser | null | undefined | false {
  if (value === undefined) return undefined;
  if (value === null) return null;
  if (!isPlainObject(value) || typeof value.id !== "string" || !DISCORD_ID_PATTERN.test(value.id)) return false;
  const avatar = value.avatar ?? null;
  if (avatar !== null && (typeof avatar !== "string" || !DISCORD_AVATAR_PATTERN.test(avatar))) return false;
  return { id: value.id, avatar };
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Lê uma mensagem do cliente. Devolve `null` se ela não seguir o contrato
 * (o tamanho bruto é conferido antes, por quem chama).
 */
export function parseClientMessage(raw: string): ClientMessage | null {
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isPlainObject(data)) return null;

  switch (data.type) {
    case "join": {
      const name = cleanName(data.name);
      if (name === null || typeof data.protocol !== "number" || !Number.isInteger(data.protocol)) return null;
      // Nome da sala: opcional (apps da versão 0.1.0 não mandam); se vier, precisa ser válido.
      const room = data.room === undefined || data.room === null ? null : cleanName(data.room);
      if (room === null && data.room !== undefined && data.room !== null) return null;
      // Conta do Discord: opcional (apps até a 0.2.0 não mandam).
      const discord = parseDiscord(data.discord);
      if (discord === false) return null;
      const session = data.session === undefined || data.session === null ? null : data.session;
      if (session !== null && (typeof session !== "string" || session.length > 512)) return null;
      return { type: "join", name, protocol: data.protocol, room, discord: discord ?? null, session };
    }
    case "room": {
      const name = cleanName(data.name);
      return name === null ? null : { type: "room", name };
    }
    case "rename": {
      const name = cleanName(data.name);
      const discord = parseDiscord(data.discord);
      if (name === null || discord === false) return null;
      return discord === undefined ? { type: "rename", name } : { type: "rename", name, discord };
    }
    case "status": {
      if (typeof data.live !== "boolean") return null;
      // muted e deafened são opcionais (o app 0.1.0 e o 0.2.0 só mandam live).
      const flag = (v: unknown) => (v === undefined ? false : typeof v === "boolean" ? v : null);
      const muted = flag(data.muted);
      const deafened = flag(data.deafened);
      if (muted === null || deafened === null) return null;
      // watching também é opcional (apps até o 0.4.0 não mandam).
      const watching = data.watching ?? null;
      if (watching !== null && (typeof watching !== "string" || !PEER_ID_PATTERN.test(watching))) return null;
      return { type: "status", live: data.live, muted, deafened, watching };
    }
    case "signal": {
      if (typeof data.to !== "string" || !PEER_ID_PATTERN.test(data.to) || !isPlainObject(data.payload)) return null;
      if (byteLength(JSON.stringify(data.payload)) > MAX_SIGNAL_PAYLOAD_BYTES) return null;
      return { type: "signal", to: data.to, payload: data.payload };
    }
    case "join-answer":
      if (typeof data.id !== "string" || !REQUEST_ID_PATTERN.test(data.id) || typeof data.accept !== "boolean")
        return null;
      return { type: "join-answer", id: data.id, accept: data.accept };
    case "verify":
      return typeof data.session === "string" && data.session.length > 0 && data.session.length <= 512
        ? { type: "verify", session: data.session }
        : null;
    case "leave":
      return { type: "leave" };
    default:
      return null;
  }
}
