import { Effect, getCurrentWindow } from "@tauri-apps/api/window";
import { create } from "zustand";
import { inTauri } from "../ui/tauri-env";

const KEY = "squadview.vidro";
/** Intensidade padrão do vidro, em %. */
export const DEFAULT_GLASS = 40;

const clamp = (v: number) => Math.min(100, Math.max(0, Math.round(v)));

function saved(): number {
  try {
    const raw = localStorage.getItem(KEY);
    const v = Number(raw);
    return raw === null || !Number.isFinite(v) ? DEFAULT_GLASS : clamp(v);
  } catch {
    return DEFAULT_GLASS;
  }
}

interface AppearanceState {
  /** Intensidade do vidro, de 0 (sólido) a 100. */
  glass: number;
  setGlass: (value: number) => void;
}

export const useAppearanceStore = create<AppearanceState>((set) => ({
  glass: saved(),
  setGlass(value) {
    const glass = clamp(value);
    set({ glass });
    try {
      localStorage.setItem(KEY, String(glass));
    } catch {
      // Sem armazenamento: vale só até fechar o app.
    }
    applyGlass(glass);
  },
}));

/** Se o Acrylic está ligado na janela (null = ainda não sabemos). */
let acrylic: boolean | null = null;

/**
 * Aplica a intensidade: as camadas do app ficam translúcidas (CSS) e, acima de 0, o Windows
 * desfoca o que está atrás da janela (Acrylic, feito pelo DWM, fora do processo do app).
 */
export function applyGlass(glass: number) {
  document.documentElement.style.setProperty("--glass", String(clamp(glass) / 100));
  const want = glass > 0;
  document.documentElement.classList.toggle("glass", want);
  if (want === acrylic || !inTauri()) return;
  acrylic = want;
  const win = getCurrentWindow();
  (want ? win.setEffects({ effects: [Effect.Acrylic] }) : win.clearEffects()).catch(() => {
    acrylic = null;
  });
}
