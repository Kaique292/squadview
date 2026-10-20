import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { useFriendsStore } from "../friends/friends-store";
import { DEFAULT_QUALITY, type StreamStats, useStreamStore } from "../stream/stream-store";
import { useToastStore } from "../ui/toast-store";
import { listenToRoomEvents, type Member, useMe, useRoomStore } from "./room-store";

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
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false };
const STATS = { fps: 60, latencyMs: 40, bitrateKbps: 8000, rttMs: 0, audioActive: true } as StreamStats;

let offs: (() => void)[] = [];

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function toasts() {
  return useToastStore.getState().toasts.map((t) => t.message);
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useAccountStore.setState({ profile: null });
  useFriendsStore.setState({ status: "signed-out", open: false, friends: [], outgoing: [] });
  useRoomStore.setState({
    code: "B8RJ2Q",
    roomName: null,
    reconnecting: false,
    invite: null,
    members: [],
    joining: false,
    joinError: null,
    pendingWatch: null,
    watching: null,
    watchStats: null,
    watchListening: true,
  });
  useToastStore.setState({ toasts: [] });
  useStreamStore.setState({
    status: "idle",
    stats: null,
    notice: null,
    fullscreen: false,
    listening: false,
    quality: DEFAULT_QUALITY,
  });
  offs.forEach((off) => off());
  offs = [await listenToRoomEvents()];
});

describe("tela inicial", () => {
  it("fora de sala, convida a assistir um amigo que está ao vivo", async () => {
    useRoomStore.setState({ code: null, members: [ME, ANA] });
    render(<App />);
    expect(screen.getByRole("heading", { name: "Ana está ao vivo" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Assistir Ana" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
    expect(screen.getByRole("button", { name: "Conectando com Ana…" })).toBeDisabled();
  });

  it("com vários amigos ao vivo, conta quantos", () => {
    useRoomStore.setState({ code: null, members: [ME, ANA, { ...BETO, live: true }] });
    render(<App />);
    expect(screen.getByRole("heading", { name: "2 amigos ao vivo" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Assistir Beto" })).toBeEnabled();
  });

  it("fechar o convite por link descarta o convite", async () => {
    useRoomStore.setState({ code: null });
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "take_pending_invite" ? "F5B6S9" : undefined));
    render(<App />);
    // Convite clicado com o app já aberto.
    await act(async () => handlers.get("invite")!({ payload: "F5B6S9" }));
    const dialog = await screen.findByRole("dialog", { name: "Entrar numa sala" });
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(useRoomStore.getState().invite).toBeNull());
  });
});

describe("canal de voz", () => {
  it("o ícone da sala na barra volta dos amigos para o canal", () => {
    useFriendsStore.setState({ open: true });
    render(<App />);
    emit("room-members", [ME]);
    expect(screen.queryByRole("region", { name: "Canal de voz" })).not.toBeInTheDocument();
    fireEvent.click(
      within(screen.getByRole("navigation", { name: "Salas" })).getByRole("button", { name: /Sala B8RJ2Q/ }),
    );
    expect(screen.getByRole("region", { name: "Canal de voz" })).toBeInTheDocument();
  });

  it("convida pelo botão da barra de controles, copiando o link", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<App />);
    emit("room-members", [ME]);
    const controles = screen.getByRole("navigation", { name: "Controles da voz" });
    fireEvent.click(within(controles).getByRole("button", { name: "Convidar pessoas" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(expect.stringContaining("sala=B8RJ2Q")));
    await waitFor(() => expect(toasts()).toContain("Link copiado. Cole no Discord para a galera entrar."));
  });

  it("sem área de transferência, mostra o link para copiar à mão", async () => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("negado")) } });
    render(<App />);
    emit("room-members", [ME]);
    const controles = screen.getByRole("navigation", { name: "Controles da voz" });
    fireEvent.click(within(controles).getByRole("button", { name: "Convidar pessoas" }));
    await waitFor(() => expect(toasts()[0]).toMatch(/O link é https:\/\/.*sala=B8RJ2Q/));
  });

  it("os controles aparecem com o foco do teclado e somem quando ele sai", () => {
    render(<App />);
    emit("room-members", [ME]);
    const canal = screen.getByRole("region", { name: "Canal de voz" });
    const grade = within(canal).getByRole("list", { name: "Quem está na voz" });
    fireEvent.focus(grade);
    expect(canal).toHaveAttribute("data-chrome");
    // Foco indo para outra parte do próprio canal não esconde.
    fireEvent.blur(grade, { relatedTarget: canal });
    expect(canal).toHaveAttribute("data-chrome");
    fireEvent.blur(grade, { relatedTarget: document.body });
    expect(canal).not.toHaveAttribute("data-chrome");
  });

  it("assiste pelo quadro de quem está ao vivo e mostra que está conectando", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const grade = screen.getByRole("list", { name: "Quem está na voz" });
    fireEvent.click(within(grade).getByRole("button", { name: "Assistir a transmissão" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
    expect(within(grade).getByRole("button", { name: "Conectando…" })).toBeDisabled();
  });
});

describe("menu da sala na lateral", () => {
  function openRoomMenu() {
    const button = screen
      .getAllByRole("button", { name: /Sala B8RJ2Q/ })
      .find((b) => !b.closest('nav[aria-label="Salas"]'))!;
    fireEvent.pointerDown(button, { button: 0, ctrlKey: false });
  }

  it("copia o código da sala", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<App />);
    emit("room-members", [ME]);
    openRoomMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: /Copiar código da sala/ }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("B8RJ2Q"));
    await waitFor(() => expect(toasts()).toContain("Código B8RJ2Q copiado."));
  });

  it("sem área de transferência, mostra o código para copiar à mão", async () => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("negado")) } });
    render(<App />);
    emit("room-members", [ME]);
    openRoomMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: /Copiar código da sala/ }));
    await waitFor(() => expect(toasts()).toContain("Não deu para copiar. O código é B8RJ2Q."));
  });

  it("convida e sai da sala pelo menu", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<App />);
    emit("room-members", [ME]);
    openRoomMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: /Convidar pessoas/ }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(expect.stringContaining("sala=B8RJ2Q")));
    openRoomMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: /Sair da sala/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_leave"));
  });

  it("explica quando o Rust recusa o nome novo da sala", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "room_rename" ? Promise.reject("Nome grande demais.") : Promise.resolve(undefined),
    );
    await useRoomStore.getState().renameRoom("x".repeat(100));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "Nome grande demais." });
  });
});

describe("menu de um membro", () => {
  it("oferece adicionar como amigo quem entrou com o Discord", async () => {
    useFriendsStore.setState({ status: "online" });
    render(<App />);
    emit("room-members", [ME, { ...BETO, discord: { id: "222222222222222222", avatar: null } }]);
    fireEvent.contextMenu(within(screen.getByRole("list", { name: "Membros da sala" })).getByText("Beto"));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Adicionar amigo/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", {
        action: { type: "friend-request", id: "222222222222222222" },
      }),
    );
  });

  it("não oferece adicionar quem já é amigo ou já tem pedido", async () => {
    useFriendsStore.setState({
      status: "online",
      outgoing: [{ id: "222222222222222222", name: "Beto", avatar: null }],
    });
    render(<App />);
    emit("room-members", [ME, { ...BETO, discord: { id: "222222222222222222", avatar: null } }]);
    fireEvent.contextMenu(within(screen.getByRole("list", { name: "Membros da sala" })).getByText("Beto"));
    await screen.findByRole("slider", { name: "Volume de Beto" });
    expect(screen.queryByRole("menuitem", { name: /Adicionar amigo/ })).not.toBeInTheDocument();
  });

  it("as setas do teclado mexem no volume, não no menu", async () => {
    render(<App />);
    emit("room-members", [ME, BETO]);
    fireEvent.contextMenu(within(screen.getByRole("list", { name: "Membros da sala" })).getByText("Beto"));
    const volume = await screen.findByRole("slider", { name: "Volume de Beto" });
    volume.focus();
    fireEvent.keyDown(volume, { key: "ArrowDown" });
    expect(volume).toHaveFocus();
  });

  it("explica quando o Rust recusa o apelido novo", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "room_set_name" ? Promise.reject("Apelido inválido.") : Promise.resolve(undefined),
    );
    await useRoomStore.getState().setNickname("   ");
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "Apelido inválido." });
  });
});

describe("assistindo um amigo", () => {
  it("dois cliques no vídeo e o botão alternam a tela cheia, e mostra o ping", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("watch-started", ANA);
    emit("watch-stats", { ...STATS, rttMs: 23 });
    expect(screen.getByText(/ping 23 ms/)).toBeInTheDocument();
    fireEvent.doubleClick(screen.getByTestId("area-do-video"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: true }));
    fireEvent.click(screen.getByRole("button", { name: "Sair da tela cheia" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: false }));
  });

  it("quem assistimos muda de estado na sala e a tela acompanha", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("watch-started", ANA);
    emit("room-members", [ME, { ...ANA, name: "Ana Paula" }]);
    expect(useRoomStore.getState().watching?.name).toBe("Ana Paula");
    expect(screen.getByRole("region", { name: "Assistindo Ana Paula" })).toBeInTheDocument();
  });

  it("se o som do vídeo não puder ser desligado, o botão volta como estava", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "set_watch_listening" ? Promise.reject(new Error("falhou")) : Promise.resolve(undefined),
    );
    await useRoomStore.getState().setWatchListening(false);
    expect(useRoomStore.getState().watchListening).toBe(true);
  });
});

describe("transmitindo", () => {
  it("dois cliques no vídeo põem em tela cheia e o fone liga o som da prévia", async () => {
    useStreamStore.setState({ status: "live", stats: STATS });
    render(<App />);
    fireEvent.doubleClick(screen.getByTestId("area-do-video"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_fullscreen", { fullscreen: true }));
    fireEvent.click(screen.getByRole("button", { name: "Ouvir o som da prévia" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_preview_listening", { listening: true }));
    expect(screen.getByRole("button", { name: "Parar de ouvir a prévia" })).toBeInTheDocument();
  });
});

describe("eu na sala", () => {
  function MeProbe() {
    const me = useMe();
    return <p>{me ? `Eu: ${me.name}` : "Fora da sala"}</p>;
  }

  it("o primeiro da lista de membros sou eu", () => {
    render(<MeProbe />);
    expect(screen.getByText("Fora da sala")).toBeInTheDocument();
    emit("room-members", [ME, ANA]);
    expect(screen.getByText("Eu: Kaique")).toBeInTheDocument();
  });

  it("desvincular o Discord sem nome conhecido não manda perfil vazio para a sala", async () => {
    useAccountStore.setState({
      profile: { id: "123456789012345678", username: "kaique", globalName: null, avatar: null },
    });
    // Sem apelido salvo nem membros na lista: não há nome para mandar.
    localStorage.clear();
    invoke.mockClear();
    act(() => useAccountStore.setState({ profile: null }));
    await act(async () => {});
    expect(invoke).not.toHaveBeenCalledWith("room_set_profile", expect.anything());
  });
});
