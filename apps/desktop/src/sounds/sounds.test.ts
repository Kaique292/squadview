import { type Member, useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { listenToSoundEvents, useSoundStore } from "./sound-store";
import { CUES, type Cue, playCue } from "./sounds";

const member = (id: string, live = false): Member => ({ id, name: id, live });

function setup() {
  const played: Cue[] = [];
  const volumes: number[] = [];
  const off = listenToSoundEvents((cue, volume) => {
    played.push(cue);
    volumes.push(volume);
  });
  return { played, volumes, off };
}

beforeEach(() => {
  localStorage.clear();
  useSoundStore.setState({ volume: 60, off: [] });
  useRoomStore.setState({ code: null, members: [], reconnecting: false });
  useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS } });
  useStreamStore.setState({ status: "idle" });
});

describe("sons de aviso", () => {
  it("toca ao entrar e ao sair da sala", () => {
    const { played, off } = setup();
    useRoomStore.setState({ code: "ABC123" });
    useRoomStore.setState({ code: null });
    expect(played).toEqual(["join", "leave"]);
    off();
  });

  it("conexão perdida toca o som de desconexão, não o de sair", () => {
    const { played, off } = setup();
    useRoomStore.setState({ code: "ABC123" });
    useRoomStore.setState({ reconnecting: true });
    useRoomStore.setState({ code: null, reconnecting: false });
    expect(played).toEqual(["join", "disconnect"]);
    off();
  });

  it("a lista da sala ao entrar não toca nada; quem chega e sai depois, sim", () => {
    const { played, off } = setup();
    useRoomStore.setState({ code: "ABC123" });
    useRoomStore.setState({ members: [member("eu"), member("ana"), member("bia")] });
    expect(played).toEqual(["join"]);
    useRoomStore.setState({ members: [member("eu"), member("ana"), member("bia"), member("caio")] });
    useRoomStore.setState({ members: [member("eu"), member("bia"), member("caio")] });
    expect(played).toEqual(["join", "user-join", "user-leave"]);
    off();
  });

  it("nossa própria troca na lista (primeiro membro) não toca o som de alguém entrar", () => {
    const { played, off } = setup();
    useRoomStore.setState({ code: "ABC123", members: [member("eu"), member("ana")] });
    played.length = 0;
    useRoomStore.setState({ members: [member("eu2"), member("ana")] });
    expect(played).toEqual([]);
    off();
  });

  it("alguém da sala começa e para de transmitir", () => {
    const { played, off } = setup();
    useRoomStore.setState({ code: "ABC123", members: [member("eu"), member("ana")] });
    played.length = 0;
    useRoomStore.setState({ members: [member("eu"), member("ana", true)] });
    useRoomStore.setState({ members: [member("eu"), member("ana", false)] });
    expect(played).toEqual(["stream-start", "stream-stop"]);
    off();
  });

  it("tirar o mudo com o surdo ligado tira os dois e toca um som só", async () => {
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, muted: true, deafened: true } });
    const { played, off } = setup();
    await useVoiceStore.getState().setMuted(false);
    expect(played).toEqual(["undeafen"]);
    off();
  });

  it("toca na ordem: mudo, tirar mudo, surdo", async () => {
    const { played, off } = setup();
    const voice = useVoiceStore.getState();
    await voice.setMuted(true);
    await voice.setMuted(false);
    await voice.setDeafened(true);
    expect(played).toEqual(["mute", "unmute", "deafen"]);
    off();
  });

  it("começar e encerrar a nossa transmissão", () => {
    const { played, off } = setup();
    useStreamStore.setState({ status: "starting" });
    useStreamStore.setState({ status: "live" });
    useStreamStore.setState({ status: "idle" });
    expect(played).toEqual(["stream-start", "stream-stop"]);
    off();
  });

  it("respeita o volume e os sons desligados", () => {
    const { played, volumes, off } = setup();
    useSoundStore.getState().setEnabled(["join", "leave"], false);
    useRoomStore.setState({ code: "ABC123" });
    expect(played).toEqual([]);
    useSoundStore.getState().setVolume(30);
    useStreamStore.setState({ status: "live" });
    expect(played).toEqual(["stream-start"]);
    expect(volumes).toEqual([0.3]);
    useSoundStore.getState().setVolume(0);
    useStreamStore.setState({ status: "idle" });
    expect(played).toEqual(["stream-start"]);
    off();
  });

  it("guarda as escolhas neste PC", () => {
    useSoundStore.getState().setVolume(25);
    useSoundStore.getState().setEnabled(["mute", "unmute"], false);
    expect(JSON.parse(localStorage.getItem("squadview.sons")!)).toEqual({ volume: 25, off: ["mute", "unmute"] });
  });

  it("todo som tem notas, e tocar sem Web Audio não quebra", () => {
    for (const notes of Object.values(CUES)) expect(notes.length).toBeGreaterThan(0);
    expect(() => playCue("join", 1)).not.toThrow();
  });
});
