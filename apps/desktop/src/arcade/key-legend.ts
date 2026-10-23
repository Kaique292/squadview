import { keyLabel } from "../settings/ArcadeSection";
import type { KeyMap, PadInput } from "./arcade-store";

/** Os botões que a legenda mostra, na ordem, com o nome do controle de Xbox. */
const BUTTONS: [PadInput, string][] = [
  ["a", "A"],
  ["b", "B"],
  ["x", "X"],
  ["y", "Y"],
  ["start", "Start"],
];

/** A tecla que melhor representa a entrada: letra antes de seta, Espaço ou teclado numérico. */
function keyFor(map: KeyMap, input: PadInput): string | null {
  const codes = Object.keys(map.keys).filter((code) => map.keys[code] === input);
  const letter = codes.find((code) => /^Key[A-Z]$/.test(code));
  const code = letter ?? codes[0];
  return code ? keyLabel(code) : null;
}

/**
 * Uma linha curta com as teclas que viram o controle ("WASD anda · J = A · K = B..."), para quem
 * joga no teclado saber o que apertar sem abrir as configurações.
 */
export function keyLegend(map: KeyMap): string {
  const stick = (["left-stick-up", "left-stick-left", "left-stick-down", "left-stick-right"] as PadInput[]).map(
    (input) => keyFor(map, input),
  );
  const parts: string[] = [];
  if (stick.every((k) => k !== null)) {
    const keys = stick as string[];
    parts.push(`${keys.every((k) => k.length === 1) ? keys.join("") : keys.join("/")} anda`);
  }
  for (const [input, name] of BUTTONS) {
    const key = keyFor(map, input);
    if (key) parts.push(`${key} = ${name}`);
  }
  parts.push("mouse = analógico direito");
  return parts.join(" · ");
}
