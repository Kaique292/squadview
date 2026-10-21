/**
 * Fundos dos quadros da sala: um degradê escuro com um brilho subindo de baixo, cada pessoa com o
 * seu. Ninguém na sala repete o fundo de outra pessoa (até acabarem as cores), e a escolha só
 * depende de quem está na sala: todo mundo vê as mesmas cores para as mesmas pessoas.
 */

/** As cores do brilho (o resto do degradê sai delas). */
export const TILE_GLOWS = [
  "#6e4ff6", // roxo
  "#3ba0ff", // azul
  "#1fbf8f", // verde-água
  "#e0569f", // rosa
  "#f0a030", // laranja
  "#e8434a", // vermelho
  "#2fa84f", // verde
  "#84cc16", // verde-limão
  "#14b8c6", // ciano
  "#d9b23a", // dourado
] as const;

/** O degradê de uma cor: escuro em cima e nas bordas, o brilho subindo do meio de baixo. */
export function tileGradient(glow: string) {
  return [
    `radial-gradient(130% 100% at 50% 118%, color-mix(in srgb, ${glow} 88%, white) 0%,`,
    `color-mix(in srgb, ${glow} 55%, #0d0b24) 36%,`,
    `color-mix(in srgb, ${glow} 16%, #07070f) 80%)`,
  ].join(" ");
}

/** Número estável a partir do id (o mesmo em todo PC). */
function hash(id: string) {
  let h = 2166136261;
  for (const c of id) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}

/**
 * A cor de cada pessoa da sala. Em ordem de id (não na ordem da lista, que começa por "eu" e muda
 * de PC para PC), cada um tenta a cor do seu id e, se já foi usada, pega a próxima livre.
 */
export function tileGlows(ids: readonly string[]): Record<string, string> {
  const used = new Set<number>();
  const result: Record<string, string> = {};
  for (const id of [...ids].sort()) {
    let slot = hash(id) % TILE_GLOWS.length;
    // Mais gente que cores: repete, mas espalhado.
    if (used.size < TILE_GLOWS.length) while (used.has(slot)) slot = (slot + 1) % TILE_GLOWS.length;
    used.add(slot);
    result[id] = TILE_GLOWS[slot];
  }
  return result;
}
