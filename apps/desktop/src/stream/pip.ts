/**
 * A conversa entre a janela principal e a janelinha do picture in picture (ADR 0021). A janelinha
 * é outra página do app (`index.html#pip=...`): ela só manda pedidos, e quem mexe no som da live é
 * a janela principal, que guarda o estado (o mesmo dos menus da live).
 */

/** Da janelinha para a principal: ligar ou desligar o som, mudar o volume. */
export const PIP_CONTROL_EVENT = "pip-control";
/** Da principal para a janelinha: como o som da live está agora. */
export const PIP_STATE_EVENT = "pip-state";

export interface PipState {
  memberId: string;
  listening: boolean;
  /** De 0 a 200 (%). */
  volume: number;
}

export type PipControl = { memberId: string } & Partial<Omit<PipState, "memberId">>;

/** O que a janelinha mostra, vindo do endereço dela (montado pelo Rust em `pip::page`). */
export interface PipPage extends PipState {
  name: string;
}

/** Lê o endereço da janelinha; `null` se não é ela. */
export function parsePipHash(hash: string): PipPage | null {
  if (!hash.startsWith("#pip=")) return null;
  const params = new URLSearchParams(hash.slice(1));
  const memberId = params.get("pip");
  if (!memberId) return null;
  const volume = Number(params.get("volume"));
  return {
    memberId,
    name: params.get("nome") || "Live",
    listening: params.get("som") !== "0",
    volume: Number.isFinite(volume) ? Math.min(200, Math.max(0, volume)) : 100,
  };
}
