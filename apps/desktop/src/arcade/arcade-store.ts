import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { type Member, useRoomStore } from "../room/room-store";
import { toast } from "../ui/toast-store";

/** Espelha `sv_session::ArcadeSeatInfo`: um jogador no fliperama que abrimos. */
export interface ArcadeSeat {
  memberId: string;
  /** O P do jogador (a vaga real do controle no Windows + 1). */
  slot: number;
  connected: boolean;
  /** Com o controle total de teclado e mouse (ADR 0024). */
  kbm: boolean;
}

/** Espelha `arcade::DriverInfo`. */
export interface DriverInfo {
  status: "installed" | "missing" | "incompatible" | "unavailable";
  detail: string | null;
  version: string | null;
  removable: boolean;
}

/** Espelha `sv_input::KeyMap`: tecla (`KeyboardEvent.code`) → entrada do controle. */
export interface KeyMap {
  keys: Record<string, PadInput>;
}

export type PadInput =
  | "a"
  | "b"
  | "x"
  | "y"
  | "start"
  | "back"
  | "left-shoulder"
  | "right-shoulder"
  | "left-trigger"
  | "right-trigger"
  | "left-thumb"
  | "right-thumb"
  | "dpad-up"
  | "dpad-down"
  | "dpad-left"
  | "dpad-right"
  | "left-stick-up"
  | "left-stick-down"
  | "left-stick-left"
  | "left-stick-right"
  | "right-stick-up"
  | "right-stick-down"
  | "right-stick-left"
  | "right-stick-right";

/** Nossa participação como jogador no fliperama de alguém. */
export interface Playing {
  hostId: string;
  /** `null` enquanto espera a vaga (e a conexão). */
  slot: number | null;
  /** Quem hospeda ligou nosso controle total. */
  kbm: boolean;
}

interface ArcadeStore {
  /** O painel do fliperama (do host) aberto. */
  hostDialog: boolean;
  /** Jogadores do fliperama que abrimos (vazio: ninguém, ou fechado). */
  seats: ArcadeSeat[];
  playing: Playing | null;
  setHostDialog: (open: boolean) => void;
  open: (slots: number, game?: string | null) => Promise<boolean>;
  close: () => Promise<void>;
  kick: (memberId: string) => Promise<void>;
  /** Dá o controle total a `memberId` (ou corta, com `null`). */
  grantKbm: (memberId: string | null) => Promise<void>;
  /** Pede uma vaga no fliperama de `host` e assiste a live dele. */
  join: (host: Member) => Promise<void>;
  leave: () => Promise<void>;
}

export const useArcadeStore = create<ArcadeStore>()((set, get) => ({
  hostDialog: false,
  seats: [],
  playing: null,
  setHostDialog: (open) => set({ hostDialog: open }),

  async open(slots, game = null) {
    try {
      await invoke("arcade_open", { slots, game });
      toast.success("Fliperama aberto: a galera já pode entrar como jogador.", "Fliperama");
      return true;
    } catch (e) {
      toast.error(typeof e === "string" ? e : "Não deu para abrir o fliperama.", "Fliperama");
      return false;
    }
  },
  async close() {
    await invoke("arcade_close").catch(() => {});
    set({ seats: [] });
  },
  async kick(memberId) {
    await invoke("arcade_kick", { memberId }).catch(() => {});
  },
  async grantKbm(memberId) {
    try {
      await invoke("arcade_grant_kbm", { memberId });
    } catch (e) {
      toast.error(typeof e === "string" ? e : "Não deu para mudar o controle total.", "Fliperama");
    }
  },
  async join(host) {
    set({ playing: { hostId: host.id, slot: null, kbm: false } });
    try {
      await invoke("arcade_join", { hostId: host.id });
    } catch (e) {
      set({ playing: null });
      toast.error(typeof e === "string" ? e : "Não deu para entrar no fliperama.", "Fliperama");
      return;
    }
    // O jogo é a live de quem hospeda: assiste se ainda não assiste.
    const room = useRoomStore.getState();
    if (!room.watching[host.id] && !room.pendingWatch.includes(host.id)) await room.watch(host);
  },
  async leave() {
    if (!get().playing) return;
    set({ playing: null });
    await invoke("arcade_leave").catch(() => {});
  },
}));

/** Nome de um membro da sala (para os avisos). */
function nameOf(id: string): string {
  return useRoomStore.getState().members.find((m) => m.id === id)?.name ?? "Quem hospeda";
}

/** Liga os eventos do fliperama vindos do Rust ao store. Chamar uma vez, ao abrir o app. */
export async function listenToArcadeEvents() {
  const offs = await Promise.all([
    listen<ArcadeSeat[]>("arcade-players", (e) => {
      const before = useArcadeStore.getState().seats;
      useArcadeStore.setState({ seats: e.payload });
      for (const seat of e.payload) {
        if (!before.some((b) => b.memberId === seat.memberId)) {
          toast.success(`${nameOf(seat.memberId)} entrou no fliperama como P${seat.slot}.`, "Fliperama");
        }
      }
    }),
    listen<{ hostId: string; slot: number }>("arcade-seat", (e) => {
      const playing = useArcadeStore.getState().playing;
      if (playing?.hostId !== e.payload.hostId) return;
      useArcadeStore.setState({ playing: { ...playing, slot: e.payload.slot } });
      toast.success(
        `Você é o P${e.payload.slot} no fliperama de ${nameOf(e.payload.hostId)}. Clique no jogo para jogar.`,
        "Fliperama",
      );
    }),
    listen<{ hostId: string; reason: string }>("arcade-left", (e) => {
      const playing = useArcadeStore.getState().playing;
      if (playing?.hostId === e.payload.hostId) useArcadeStore.setState({ playing: null });
      if (e.payload.reason) toast.warning(e.payload.reason, "Fliperama");
    }),
    listen<{ hostId: string; granted: boolean }>("arcade-kbm", (e) => {
      const playing = useArcadeStore.getState().playing;
      if (playing?.hostId !== e.payload.hostId) return;
      useArcadeStore.setState({ playing: { ...playing, kbm: e.payload.granted } });
      toast.info(
        e.payload.granted
          ? `${nameOf(e.payload.hostId)} te deu o controle total: seu teclado e mouse vão direto para o jogo.`
          : "O controle total foi desligado: seu teclado e mouse voltam a ser o controle.",
        "Fliperama",
      );
    }),
  ]);
  return () => offs.forEach((off) => off());
}

/** Vagas livres no fliperama de um membro (0 se fechado). */
export function freeSlots(member: Member): number {
  const arcade = member.arcade;
  return arcade ? Math.max(0, arcade.slots - arcade.players.length) : 0;
}
