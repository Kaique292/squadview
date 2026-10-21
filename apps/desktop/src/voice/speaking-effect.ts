import { invoke } from "@tauri-apps/api/core";
import { useMemo } from "react";
import { create } from "zustand";

/**
 * Efeito em volta da foto quando a pessoa fala (ADR 0015). Cada um escolhe o seu nas
 * configurações e a sala inteira passa a vê-lo; sem efeito, fica o anel verde padrão.
 */

export const SPEAKING_EFFECTS = ["dots", "waves", "equalizer", "rays"] as const;
export type SpeakingEffectKind = (typeof SPEAKING_EFFECTS)[number];

/** Espelha `sv_net::SpeakingEffect`. */
export interface SpeakingEffect {
  kind: SpeakingEffectKind;
  /** Cor no formato `#rrggbb`. */
  color: string;
}

export const EFFECT_LABELS: Record<SpeakingEffectKind, string> = {
  dots: "Esfera de pontos",
  waves: "Ondas de linhas",
  equalizer: "Equalizador",
  rays: "Raios",
};

/** Cores prontas para escolher (a última pode ser trocada por qualquer outra). */
export const EFFECT_COLORS = ["#f5e27a", "#3ef0b0", "#a8a8f0", "#7c3aed", "#ff5a4f", "#3ba0ff", "#ff6ac1"];

const COLOR = /^#[0-9a-f]{6}$/;
const KEY = "squadview.efeito-de-fala";

/**
 * O efeito, se for um que esta versão sabe desenhar e a cor estiver no formato; senão `null`
 * (o anel padrão). Vale para o salvo aqui e para o que chega de outros apps, de qualquer versão.
 */
export function cleanEffect(value: unknown): SpeakingEffect | null {
  if (typeof value !== "object" || value === null) return null;
  const { kind, color } = value as Record<string, unknown>;
  const known = SPEAKING_EFFECTS.find((k) => k === kind);
  if (!known || typeof color !== "string") return null;
  const hex = color.toLowerCase();
  return COLOR.test(hex) ? { kind: known, color: hex } : null;
}

function saved(): SpeakingEffect | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? cleanEffect(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}

interface SpeakingEffectState {
  /** O nosso efeito (`null`: o anel padrão). */
  effect: SpeakingEffect | null;
  /** Aplica o efeito: salva neste PC e avisa a sala, se estivermos numa. */
  setEffect: (effect: SpeakingEffect | null) => void;
}

export const useSpeakingEffectStore = create<SpeakingEffectState>((set) => ({
  effect: saved(),
  setEffect(value) {
    const effect = cleanEffect(value);
    set({ effect });
    try {
      if (effect) localStorage.setItem(KEY, JSON.stringify(effect));
      else localStorage.removeItem(KEY);
    } catch {
      // Sem armazenamento: vale só até fechar o app.
    }
    // Fora de uma sala o Rust não faz nada: o efeito vai junto ao entrar.
    void invoke("room_set_effect", { effect }).catch(() => {});
  },
}));

/**
 * O efeito de um membro da sala: o nosso vem do que escolhemos aqui (muda na hora, mesmo antes
 * de a sala confirmar); o dos outros, do que eles mandaram (o que não conhecemos vira o padrão).
 */
export function useMemberEffect(member: { effect?: unknown } | null | undefined, isMe: boolean) {
  const mine = useSpeakingEffectStore((s) => s.effect);
  const theirs = cleanEffect(member?.effect);
  // O mesmo objeto enquanto o efeito não muda: o desenho não é refeito a cada atualização da sala.
  const kind = theirs?.kind;
  const color = theirs?.color;
  const stable = useMemo(() => (kind && color ? { kind, color } : null), [kind, color]);
  return isMe ? mine : stable;
}
