/**
 * Códigos de sala: 6 caracteres fáceis de ditar na call
 * (sem 0/O, 1/I/L, que se confundem).
 */
export const ROOM_CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
export const ROOM_CODE_LENGTH = 6;

export function generateRoomCode(random: (max: number) => number = secureRandom): string {
  let code = "";
  for (let i = 0; i < ROOM_CODE_LENGTH; i++) code += ROOM_CODE_ALPHABET[random(ROOM_CODE_ALPHABET.length)];
  return code;
}

export function isValidRoomCode(code: string): boolean {
  return code.length === ROOM_CODE_LENGTH && [...code].every((c) => ROOM_CODE_ALPHABET.includes(c));
}

function secureRandom(max: number): number {
  return crypto.getRandomValues(new Uint32Array(1))[0] % max;
}
