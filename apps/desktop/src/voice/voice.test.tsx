import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { listenToRoomEvents, type Member, useRoomStore } from "../room/room-store";
import { DEFAULT_QUALITY, useStreamStore } from "../stream/stream-store";
import { useToastStore } from "../ui/toast-store";
import { gridColumns } from "./VoiceRoom";
import { DEFAULT_PREFS, listenToVoiceEvents, useVoiceStore } from "./voice-store";

const { invoke, listen, handlers } = vi.hoisted(() => {
  const handlers = new Map<string, (e: { payload: unknown }) => void>();
  return {
    handlers,
    invoke: vi.fn(),
    listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
      handlers.set(name, cb);
      return () => handlers.delete(name);
    }),
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const ME: Member = { id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: false };
const ANA: Member = { id: "bbbbbbbbbbbbbbbb", name: "Ana", live: true };
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false, muted: true };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: null, pendingWatch: null });
  useStreamStore.setState({ status: "idle", stats: null, fullscreen: false, quality: DEFAULT_QUALITY });
  useToastStore.setState({ toasts: [] });
  await listenToRoomEvents();
  await listenToVoiceEvents((id) => useRoomStore.getState().members.find((m) => m.id === id)?.name);
});

describe("chat de voz", () => {
  it("entra na sala já com a voz e as escolhas salvas", async () => {
    useRoomStore.setState({ code: null });
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, inputVolume: 150, muted: true } });
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q" } : undefined));
    await useRoomStore.getState().join(null, "Kaique");
    expect(invoke).toHaveBeenCalledWith(
      "room_join",
      expect.objectContaining({ voice: expect.objectContaining({ inputVolume: 1.5, muted: true }) }),
    );
  });

  it("mostra um quadro por pessoa, com o nome da sala, o ao vivo e quem está mudo", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    const canal = screen.getByRole("region", { name: "Canal de voz" });
    expect(within(canal).getByText("Galera")).toBeInTheDocument();
    const grade = within(canal).getByRole("list", { name: "Quem está na voz" });
    expect(within(grade).getAllByRole("listitem")).toHaveLength(3);
    expect(within(grade).getByText("Ao vivo")).toBeInTheDocument();
    expect(within(grade).getByLabelText("Microfone silenciado")).toBeInTheDocument();
  });

  it("acende a borda de quem está falando", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("voice-speaking", { memberId: ANA.id, speaking: true });
    const grade = screen.getByRole("list", { name: "Quem está na voz" });
    expect(grade.querySelectorAll("[data-speaking]")).toHaveLength(1);
    emit("voice-speaking", { memberId: null, speaking: true });
    expect(grade.querySelectorAll("[data-speaking]")).toHaveLength(2);
  });

  it("silencia o microfone e ensurdece pelos botões, e lembra", async () => {
    render(<App />);
    emit("room-members", [ME]);
    fireEvent.click(within(screen.getByRole("contentinfo")).getByRole("button", { name: "Silenciar microfone" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_set_muted", { muted: true }));
    fireEvent.click(within(screen.getByRole("contentinfo")).getByRole("button", { name: "Ensurdecer" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_set_deafened", { deafened: true }));
    expect(JSON.parse(localStorage.getItem("squadview.voz")!)).toMatchObject({ muted: true, deafened: true });
  });

  it("ligar o microfone com o fone desligado liga os dois, como no Discord", async () => {
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, muted: true, deafened: true } });
    await useVoiceStore.getState().setMuted(false);
    expect(useVoiceStore.getState().prefs).toMatchObject({ muted: false, deafened: false });
    expect(invoke).toHaveBeenCalledWith("voice_set_deafened", { deafened: false });
  });

  it("muda volume e silencia um amigo só para mim, pelo botão direito", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.contextMenu(within(screen.getByRole("list", { name: "Membros da sala" })).getByText("Ana"));
    const volume = await screen.findByRole("slider", { name: "Volume de Ana" });
    fireEvent.change(volume, { target: { value: "150" } });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("voice_set_peer_volume", { memberId: ANA.id, volume: 1.5 }),
    );
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Silenciar/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_set_peer_muted", { memberId: ANA.id, muted: true }));
    expect(useVoiceStore.getState().peers.Ana).toEqual({ volume: 150, muted: true });
  });

  it("reaplica o volume salvo quando a voz com o amigo liga", () => {
    useRoomStore.setState({ members: [ME, ANA] });
    useVoiceStore.setState({ peers: { Ana: { volume: 60, muted: true } } });
    emit("voice-peers", [ANA.id]);
    expect(invoke).toHaveBeenCalledWith("voice_set_peer_volume", { memberId: ANA.id, volume: 0.6 });
    expect(invoke).toHaveBeenCalledWith("voice_set_peer_muted", { memberId: ANA.id, muted: true });
  });

  it("explica quando não deu para ligar a voz com alguém", () => {
    useRoomStore.setState({ members: [ME, ANA] });
    emit("voice-failed", { memberId: ANA.id, reason: "sem conexão direta" });
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "warning",
      message: "Sem voz com Ana: sem conexão direta",
    });
  });

  it("o botão vermelho sai da sala", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const controles = screen.getByRole("navigation", { name: "Controles da voz" });
    fireEvent.click(within(controles).getByRole("button", { name: "Sair da sala" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_leave"));
  });

  it("a barra da chamada junta os botões em grupos, com o desligar à parte", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const controles = screen.getByRole("navigation", { name: "Controles da voz" });
    const audio = within(controles).getByRole("group", { name: "Áudio" });
    expect(within(audio).getByRole("button", { name: "Silenciar microfone" })).toBeInTheDocument();
    expect(within(audio).getByRole("button", { name: "Ensurdecer" })).toBeInTheDocument();
    const tela = within(controles).getByRole("group", { name: "Tela e convite" });
    expect(within(tela).getByRole("button", { name: "Transmitir minha tela" })).toBeInTheDocument();
    expect(within(tela).getByRole("button", { name: "Convidar pessoas" })).toBeInTheDocument();
    expect(within(audio).queryByRole("button", { name: "Sair da sala" })).not.toBeInTheDocument();
    expect(within(tela).queryByRole("button", { name: "Sair da sala" })).not.toBeInTheDocument();
  });

  it("ensurdecido aparece com o microfone e o fone cortados", () => {
    render(<App />);
    emit("room-members", [ME, { ...ANA, live: false, muted: true, deafened: true }]);
    const grade = screen.getByRole("list", { name: "Quem está na voz" });
    expect(within(grade).getByLabelText("Microfone silenciado")).toBeInTheDocument();
    expect(within(grade).getByLabelText("Ensurdecido")).toBeInTheDocument();
  });

  it("mostra a foto do Discord dos outros membros", () => {
    render(<App />);
    emit("room-members", [ME, { ...ANA, discord: { id: "123456789012345678", avatar: "a".repeat(32) } }]);
    const grade = screen.getByRole("list", { name: "Quem está na voz" });
    expect(grade.querySelector('img[src*="cdn.discordapp.com/avatars/123456789012345678/"]')).not.toBeNull();
  });

  it("vincular o Discord já na sala avisa a galera do nome e da foto", async () => {
    useRoomStore.setState({ members: [ME] });
    const profile = { id: "123456789012345678", username: "kaique", globalName: "K.A", avatar: "b".repeat(32) };
    act(() => useAccountStore.setState({ profile }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("room_set_profile", {
        name: "K.A",
        discord: { id: profile.id, avatar: profile.avatar },
      }),
    );
    act(() => useAccountStore.setState({ profile: null }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_set_profile", { name: "Kaique", discord: null }));
  });

  it("mostra o topo e os controles só com o mouse no canal, como no Discord", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const canal = screen.getByRole("region", { name: "Canal de voz" });
    expect(canal).not.toHaveAttribute("data-chrome");
    fireEvent.mouseEnter(canal);
    expect(canal).toHaveAttribute("data-chrome");
    fireEvent.mouseLeave(canal);
    expect(canal).not.toHaveAttribute("data-chrome");
  });

  it("organiza a grade como o Discord", () => {
    expect([1, 2, 3, 4, 5, 6, 7, 8].map(gridColumns)).toEqual([1, 2, 2, 2, 3, 3, 4, 4]);
  });

  it("mostra o nível do microfone e os avisos da voz", () => {
    emit("voice-level", 0.4);
    expect(useVoiceStore.getState().level).toBe(0.4);
    emit("voice-error", "Microfone ocupado por outro app.");
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "warning",
      message: "Microfone ocupado por outro app.",
    });
  });

  it("não reaplica nada para quem já estava na voz ou não tem escolha salva", () => {
    useRoomStore.setState({ members: [ME, ANA, BETO] });
    useVoiceStore.setState({ peers: { Ana: { volume: 100, muted: false } } });
    emit("voice-peers", [ANA.id]);
    emit("voice-peers", [ANA.id, BETO.id]);
    expect(invoke).not.toHaveBeenCalledWith("voice_set_peer_volume", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("voice_set_peer_muted", expect.anything());
  });

  it("sem nome conhecido, o aviso de falha fala de um amigo", () => {
    emit("voice-failed", { memberId: "dddddddddddddddd", reason: "sem rota" });
    expect(useToastStore.getState().toasts[0].message).toBe("Sem voz com um amigo: sem rota");
  });
});
