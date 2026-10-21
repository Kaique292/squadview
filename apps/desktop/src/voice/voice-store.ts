import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { toast } from "../ui/toast-store";

/** Espelha `sv_media::AudioDevice`. */
export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

/** Escolhas de voz que ficam salvas neste PC (espelha `voice::VoicePrefs` no Rust). */
export interface VoicePrefs {
  inputDevice: string | null;
  outputDevice: string | null;
  /** Volume do microfone, em % (0 a 200). */
  inputVolume: number;
  /** Volume da saída, em % (0 a 200). */
  outputVolume: number;
  /** O "Isolamento de voz" do Discord. */
  noiseSuppression: boolean;
  /** Controle automático de ganho: a voz chega no mesmo volume para todos. */
  autoGain: boolean;
  muted: boolean;
  deafened: boolean;
}

/** Volume e silenciar de um amigo, só para nós. Guardado pelo nome (o id muda a cada sala). */
export interface PeerPrefs {
  volume: number;
  muted: boolean;
}

export const MAX_VOICE_VOLUME = 200;
const PREFS_KEY = "squadview.voz";
const PEERS_KEY = "squadview.voz.pessoas";

export const DEFAULT_PREFS: VoicePrefs = {
  inputDevice: null,
  outputDevice: null,
  inputVolume: 100,
  outputVolume: 100,
  noiseSuppression: true,
  autoGain: true,
  muted: false,
  deafened: false,
};

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? { ...fallback, ...(JSON.parse(raw) as T) } : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Sem armazenamento: vale só nesta sessão.
  }
}

/** O que vai para o Rust: volumes em 0 a 2. */
export function toRust(p: VoicePrefs) {
  return { ...p, inputVolume: p.inputVolume / 100, outputVolume: p.outputVolume / 100 };
}

interface VoiceState {
  prefs: VoicePrefs;
  peers: Record<string, PeerPrefs>;
  /** Quem está falando agora (`"me"` = nós). */
  speaking: Record<string, boolean>;
  /** Com quem a voz está ligada (ids). */
  connected: string[];
  /** Com quem a voz passa por outro membro (ADR 0020): id → id de quem repassa. */
  bridges: Record<string, string>;
  /** Nível do microfone (0 a 1). */
  level: number;
  devices: { inputs: AudioDevice[]; outputs: AudioDevice[] };
  setMuted: (muted: boolean) => Promise<void>;
  setDeafened: (deafened: boolean) => Promise<void>;
  setPrefs: (changes: Partial<VoicePrefs>) => Promise<void>;
  peerPrefs: (name: string) => PeerPrefs;
  setPeerVolume: (id: string, name: string, volume: number) => Promise<void>;
  setPeerMuted: (id: string, name: string, muted: boolean) => Promise<void>;
  loadDevices: () => Promise<void>;
}

export const useVoiceStore = create<VoiceState>((set, get) => ({
  prefs: load(PREFS_KEY, DEFAULT_PREFS),
  peers: load<Record<string, PeerPrefs>>(PEERS_KEY, {}),
  speaking: {},
  connected: [],
  bridges: {},
  level: 0,
  devices: { inputs: [], outputs: [] },

  async setMuted(muted) {
    // Tirar o mudo com o surdo ligado tira os dois, como no Discord.
    const deafened = muted ? get().prefs.deafened : false;
    const prefs = { ...get().prefs, muted, deafened };
    set({ prefs });
    save(PREFS_KEY, prefs);
    await invoke("voice_set_muted", { muted }).catch(() => {});
    if (!deafened) await invoke("voice_set_deafened", { deafened: false }).catch(() => {});
  },

  async setDeafened(deafened) {
    const prefs = { ...get().prefs, deafened };
    set({ prefs });
    save(PREFS_KEY, prefs);
    await invoke("voice_set_deafened", { deafened }).catch(() => {});
  },

  async setPrefs(changes) {
    const prefs = { ...get().prefs, ...changes };
    set({ prefs });
    save(PREFS_KEY, prefs);
    await invoke("voice_set_settings", { prefs: toRust(prefs) }).catch(() => {});
  },

  peerPrefs(name) {
    return get().peers[name] ?? { volume: 100, muted: false };
  },

  async setPeerVolume(id, name, volume) {
    const v = Math.min(MAX_VOICE_VOLUME, Math.max(0, Math.round(volume)));
    const peers = { ...get().peers, [name]: { ...get().peerPrefs(name), volume: v } };
    set({ peers });
    save(PEERS_KEY, peers);
    await invoke("voice_set_peer_volume", { memberId: id, volume: v / 100 }).catch(() => {});
  },

  async setPeerMuted(id, name, muted) {
    const peers = { ...get().peers, [name]: { ...get().peerPrefs(name), muted } };
    set({ peers });
    save(PEERS_KEY, peers);
    await invoke("voice_set_peer_muted", { memberId: id, muted }).catch(() => {});
  },

  async loadDevices() {
    try {
      const devices = await invoke<VoiceState["devices"] | null>("voice_devices");
      // Resposta fora do esperado não pode derrubar os botões: fica a lista vazia.
      if (devices && Array.isArray(devices.inputs) && Array.isArray(devices.outputs)) set({ devices });
    } catch {
      // Fora do app (navegador, testes): sem lista.
    }
  },
}));

/**
 * Ouve os eventos da voz. `nameOf` diz o nome de um membro pelo id (para reaplicar o volume e o
 * silenciar salvos de cada amigo quando a voz com ele liga).
 */
export async function listenToVoiceEvents(nameOf: (id: string) => string | undefined) {
  const offs = await Promise.all([
    listen<{ memberId: string | null; speaking: boolean }>("voice-speaking", (e) => {
      const key = e.payload.memberId ?? "me";
      useVoiceStore.setState((s) => ({ speaking: { ...s.speaking, [key]: e.payload.speaking } }));
    }),
    listen<number>("voice-level", (e) => useVoiceStore.setState({ level: e.payload })),
    listen<string[]>("voice-peers", (e) => {
      useVoiceStore.setState({ connected: e.payload });
      // A cada mudança, reaplica o salvo de todos (não só de quem acabou de chegar): uma voz que cai
      // e volta entre dois avisos não parece "nova", e o silenciar dela se perdia.
      for (const id of e.payload) {
        const name = nameOf(id);
        if (!name) continue;
        const saved = useVoiceStore.getState().peers[name];
        if (saved && saved.volume !== 100)
          void invoke("voice_set_peer_volume", { memberId: id, volume: saved.volume / 100 });
        if (saved?.muted) void invoke("voice_set_peer_muted", { memberId: id, muted: true });
      }
    }),
    listen<Record<string, string>>("voice-bridges", (e) => useVoiceStore.setState({ bridges: e.payload })),
    listen<string>("voice-error", (e) => toast.warning(e.payload, "Voz")),
    listen<{ memberId: string; reason: string }>("voice-failed", (e) => {
      const name = nameOf(e.payload.memberId) ?? "um amigo";
      toast.warning(`Sem voz com ${name}: ${e.payload.reason}`, "Voz");
    }),
  ]);
  return () => offs.forEach((off) => off());
}
