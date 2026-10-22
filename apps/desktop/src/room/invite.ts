/** Página de convite (GitHub Pages): abre o app pelo `squadview://` ou oferece o download. */
export const INVITE_PAGE = "https://kaique292.github.io/squadview/convite/";

/** Link para mandar à galera: funciona para quem tem e para quem não tem o app. */
export function inviteUrl(code: string): string {
  return `${INVITE_PAGE}?sala=${encodeURIComponent(code)}`;
}

/** Link de convite de um servidor (`<id>.<segredo>`): a mesma página abre `squadview://servidor/...` ou o download. */
export function serverInviteLink(invite: string): string {
  return `${INVITE_PAGE}?servidor=${encodeURIComponent(invite)}`;
}

/** Letras dos códigos de sala, fáceis de ditar (sem 0/O, 1/I/L). Igual ao `room-code.ts` do Worker. */
const ROOM_CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const ROOM_CODE_LENGTH = 6;

/**
 * O que a pessoa digitou ou colou no campo do código: o link de convite inteiro vira o código dele;
 * o resto fica em maiúsculas, sem espaços nem hífens.
 */
export function codeFromInput(text: string): string {
  const fromLink = /[?&]sala=([^&#\s]+)/i.exec(text);
  const raw = fromLink ? decodeURIComponent(fromLink[1]) : text;
  return raw.toUpperCase().replace(/[\s-]/g, "");
}

/** O código tem 6 caracteres e só letras e números que um código de sala usa. */
export function isRoomCode(code: string): boolean {
  return code.length === ROOM_CODE_LENGTH && [...code].every((c) => ROOM_CODE_ALPHABET.includes(c));
}
