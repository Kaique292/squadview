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

export interface Member {
  id: PeerId;
  name: string;
  live: boolean;
}

export type ClientMessage =
  | { type: "join"; name: string; protocol: number }
  | { type: "status"; live: boolean }
  | { type: "signal"; to: PeerId; payload: Record<string, unknown> }
  | { type: "leave" };

export type ErrorCode =
  "room-full" | "invalid-message" | "version-mismatch" | "not-joined" | "unknown-peer" | "rate-limited";

export type ServerMessage =
  | { type: "welcome"; you: PeerId; members: Member[] }
  | { type: "member-joined"; member: Member }
  | { type: "member-updated"; member: Member }
  | { type: "member-left"; id: PeerId }
  | { type: "signal"; from: PeerId; payload: Record<string, unknown> }
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
      return { type: "join", name, protocol: data.protocol };
    }
    case "status":
      return typeof data.live === "boolean" ? { type: "status", live: data.live } : null;
    case "signal": {
      if (typeof data.to !== "string" || !PEER_ID_PATTERN.test(data.to) || !isPlainObject(data.payload)) return null;
      if (byteLength(JSON.stringify(data.payload)) > MAX_SIGNAL_PAYLOAD_BYTES) return null;
      return { type: "signal", to: data.to, payload: data.payload };
    }
    case "leave":
      return { type: "leave" };
    default:
      return null;
  }
}
