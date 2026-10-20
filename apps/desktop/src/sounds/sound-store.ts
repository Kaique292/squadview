import { create } from "zustand";
import { useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { useVoiceStore } from "../voice/voice-store";
import { type Cue, playCue } from "./sounds";

/** Os sons que dá para desligar um por um (como em Configurações › Notificações › Sons do Discord). */
export const SOUND_OPTIONS: { cues: Cue[]; label: string }[] = [
  { cues: ["join", "leave"], label: "Entrar e sair da sala" },
  { cues: ["user-join", "user-leave"], label: "Alguém entra ou sai da sala" },
  { cues: ["mute", "unmute"], label: "Mudo" },
  { cues: ["deafen", "undeafen"], label: "Surdo" },
  { cues: ["stream-start", "stream-stop"], label: "Começar e encerrar transmissão" },
  { cues: ["disconnect"], label: "Conexão perdida" },
];

export interface SoundPrefs {
  /** Volume dos avisos, em % (0 desliga todos). */
  volume: number;
  /** Sons desligados. */
  off: Cue[];
}

const KEY = "squadview.sons";
export const DEFAULT_SOUND_VOLUME = 60;
const DEFAULTS: SoundPrefs = { volume: DEFAULT_SOUND_VOLUME, off: [] };

function saved(): SoundPrefs {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<SoundPrefs>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

interface SoundState extends SoundPrefs {
  setVolume: (volume: number) => void;
  setEnabled: (cues: Cue[], on: boolean) => void;
}

export const useSoundStore = create<SoundState>((set, get) => {
  function save(changes: Partial<SoundPrefs>) {
    set(changes);
    const { volume, off } = get();
    try {
      localStorage.setItem(KEY, JSON.stringify({ volume, off }));
    } catch {
      // Sem armazenamento: vale só até fechar o app.
    }
  }
  return {
    ...saved(),
    setVolume(volume) {
      save({ volume: Math.min(100, Math.max(0, Math.round(volume))) });
    },
    setEnabled(cues, on) {
      const rest = get().off.filter((c) => !cues.includes(c));
      save({ off: on ? rest : [...rest, ...cues] });
    },
  };
});

/** Toca o som se ele estiver ligado, no volume escolhido. */
export function sound(cue: Cue, play: (cue: Cue, volume: number) => void = playCue) {
  const { volume, off } = useSoundStore.getState();
  if (volume > 0 && !off.includes(cue)) play(cue, volume / 100);
}

/**
 * Liga os sons ao que acontece no app: sala, voz e transmissão. Só reage a mudanças (abrir o app
 * com o mudo salvo, por exemplo, não toca nada). Chamar uma vez, ao abrir o app.
 */
export function listenToSoundEvents(play: (cue: Cue, volume: number) => void = playCue) {
  const cue = (c: Cue) => sound(c, play);

  const offRoom = useRoomStore.subscribe((s, prev) => {
    // Nós: entramos, saímos ou trocamos de sala.
    if (s.code !== prev.code) {
      if (s.code) cue("join");
      else if (prev.reconnecting) cue("disconnect");
      else cue("leave");
      return;
    }
    // Os outros (o primeiro da lista somos nós). A primeira lista depois de entrar não conta,
    // nem a troca de lista ao reconectar.
    if (!s.code || s.members === prev.members || prev.members.length === 0) return;
    if (s.reconnecting || prev.reconnecting) return;
    const before = new Map(prev.members.slice(1).map((m) => [m.id, m]));
    const now = new Map(s.members.slice(1).map((m) => [m.id, m]));
    const joined = [...now.keys()].some((id) => !before.has(id));
    const left = [...before.keys()].some((id) => !now.has(id));
    if (joined) cue("user-join");
    else if (left) cue("user-leave");
    // Alguém começou ou parou de transmitir.
    for (const [id, m] of now) {
      const was = before.get(id);
      if (!was || was.live === m.live) continue;
      cue(m.live ? "stream-start" : "stream-stop");
      break;
    }
  });

  const offVoice = useVoiceStore.subscribe((s, prev) => {
    const { muted, deafened } = s.prefs;
    // Surdo manda: tirar o surdo também tira o mudo, e soa um som só.
    if (deafened !== prev.prefs.deafened) cue(deafened ? "deafen" : "undeafen");
    else if (muted !== prev.prefs.muted) cue(muted ? "mute" : "unmute");
  });

  const offStream = useStreamStore.subscribe((s, prev) => {
    if (s.status === "live" && prev.status !== "live") cue("stream-start");
    else if (prev.status === "live" && s.status !== "live") cue("stream-stop");
  });

  return () => {
    offRoom();
    offVoice();
    offStream();
  };
}
