import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { type Member, useRoomStore } from "../room/room-store";
import { toast } from "../ui/toast-store";
import type { PadDevice } from "./controllers";

/** Espelha `sv_session::ArcadeSeatInfo`: um jogador no fliperama que abrimos. */
export interface ArcadeSeat {
  memberId: string;
  /** O P do jogador (a vaga real do controle no Windows + 1). */
  slot: number;
  connected: boolean;
  /** Com o controle total de teclado e mouse (ADR 0024). */
  kbm: boolean;
  /** O Windows deu vaga no XInput ao controle virtual dele (sem vaga, o jogo não o vê). */
  xinput?: boolean;
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
  /** A conexão do controle passa pelo repassador (mais atraso). */
  relayed?: boolean;
}

interface ArcadeStore {
  /** O painel do fliperama (do host) aberto. */
  hostDialog: boolean;
  /** Jogadores do fliperama que abrimos (vazio: ninguém, ou fechado). */
  seats: ArcadeSeat[];
  playing: Playing | null;
  /** Os controles do nosso PC enquanto jogamos (`null`: ainda conferindo). */
  controllers: PadDevice[] | null;
  /** A janela do jogo do nosso fliperama está em primeiro plano (`null`: não sabemos). */
  gameFocused: boolean | null;
  setHostDialog: (open: boolean) => void;
  /** `reserveP1`: sem controle no PC, um controle virtual parado guarda o P1 para nós. */
  open: (slots: number, game?: string | null, reserveP1?: boolean) => Promise<boolean>;
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
  controllers: null,
  gameFocused: null,
  setHostDialog: (open) => set({ hostDialog: open }),

  async open(slots, game = null, reserveP1 = true) {
    try {
      await invoke("arcade_open", { slots, game, reserveP1 });
      focusWarned = false;
      toast.success("Fliperama aberto: a galera já pode entrar como jogador.", "Fliperama");
      return true;
    } catch (e) {
      toast.error(typeof e === "string" ? e : "Não deu para abrir o fliperama.", "Fliperama");
      return false;
    }
  },
  async close() {
    await invoke("arcade_close").catch(() => {});
    set({ seats: [], gameFocused: null });
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
    set({ playing: { hostId: host.id, slot: null, kbm: false }, controllers: null });
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
    set({ playing: null, controllers: null });
    await invoke("arcade_leave").catch(() => {});
  },
}));

/** Já avisamos, neste fliperama, que o jogo saiu do primeiro plano (o aviso não se repete). */
let focusWarned = false;

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
          if (seat.xinput === false) {
            toast.warning(
              `O Windows não deu vaga ao controle de ${nameOf(seat.memberId)}, e o jogo não vai enxergá-lo. Desligue um controle do seu PC (o Windows aceita 4) e peça para ele entrar de novo.`,
              "Fliperama",
            );
          }
        }
      }
    }),
    listen<PadDevice[]>("arcade-controllers", (e) => {
      if (useArcadeStore.getState().playing) useArcadeStore.setState({ controllers: e.payload });
    }),
    listen<boolean>("arcade-focus", (e) => {
      useArcadeStore.setState({ gameFocused: e.payload });
      // Muitos jogos ignoram os controles fora do primeiro plano: o amigo acha que o controle dele
      // não funciona. Avisa uma vez por fliperama, e só com alguém jogando.
      const playing = useArcadeStore.getState().seats.some((s) => s.connected);
      if (!e.payload && playing && !focusWarned) {
        focusWarned = true;
        toast.warning(
          "O jogo saiu do primeiro plano. Muitos jogos ignoram os controles dos amigos assim: volte para a janela do jogo.",
          "Fliperama",
        );
      }
    }),
    listen<{ hostId: string; slot: number; relayed?: boolean }>("arcade-seat", (e) => {
      const playing = useArcadeStore.getState().playing;
      if (playing?.hostId !== e.payload.hostId) return;
      useArcadeStore.setState({ playing: { ...playing, slot: e.payload.slot, relayed: e.payload.relayed ?? false } });
      toast.success(
        `Você é o P${e.payload.slot} no fliperama de ${nameOf(e.payload.hostId)}. Clique no jogo para jogar.`,
        "Fliperama",
      );
    }),
    listen<{ hostId: string; reason: string }>("arcade-left", (e) => {
      const playing = useArcadeStore.getState().playing;
      if (playing?.hostId === e.payload.hostId) useArcadeStore.setState({ playing: null, controllers: null });
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
