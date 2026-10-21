import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { App } from "../App";
import { DEFAULT_QUALITY, type StreamStats, useStreamStore } from "../stream/stream-store";
import { useToastStore } from "../ui/toast-store";
import { listenToRoomEvents, MAX_WATCHING, type Member, savedName, useRoomStore, type Watch } from "./room-store";

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

/** Uma live que já estamos assistindo. */
function watchOf(member: Member, extra: Partial<Watch> = {}): Watch {
  return { member, stats: null, history: [], listening: true, volume: 100, ...extra };
}

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useRoomStore.setState({
    code: null,
    roomName: null,
    reconnecting: false,
    members: [],
    joining: false,
    joinError: null,
    pendingWatch: [],
    watching: {},
  });
  useToastStore.setState({ toasts: [] });
  useStreamStore.setState({
    status: "idle",
    stats: null,
    fullscreen: false,
    fullscreenOf: null,
    pip: null,
    quality: DEFAULT_QUALITY,
  });
  await listenToRoomEvents();
});

describe("criar e entrar em sala", () => {
  it("cria uma sala com o nome e mostra o código para mandar para a galera", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q" } : undefined));
    render(<App />);
    // O "+" da barra de salas agora adiciona servidores: a sala rápida é o botão da lateral.
    fireEvent.click(screen.getByRole("button", { name: "Criar sala" }));
    const dialog = await screen.findByRole("dialog");
    const submit = screen.getAllByRole("button", { name: "Criar sala" }).find((b) => dialog.contains(b))!;
    expect(submit).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText("Como a galera te chama"), { target: { value: " Kaique " } });
    fireEvent.click(submit);
    // Sem nome digitado, a sala ganha o nome padrão "Sala de <nome>".
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "room_join",
        expect.objectContaining({ code: null, name: " Kaique ", roomName: "Sala de Kaique" }),
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    emit("room-members", [ME]);
    expect(sidebarButton(/Sala B8RJ2Q/)).toBeInTheDocument();
    // Sem a dica "Mande o código da sala…" embaixo de quem está sozinho na sala.
    expect(screen.queryByText(/Mande o código da sala/)).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Sucesso" })).toHaveTextContent("Você está na sala");
    expect(savedName()).toBe("Kaique");
  });

  it("entra com o código que o amigo mandou", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q" } : undefined));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Entrar com código" }));
    await screen.findByRole("dialog");
    fireEvent.change(screen.getByPlaceholderText("Como a galera te chama"), { target: { value: "Beto" } });
    const code = screen.getByPlaceholderText("B8RJ2Q");
    fireEvent.change(code, { target: { value: "b8r" } });
    expect(screen.getByRole("button", { name: "Entrar" })).toBeDisabled();
    fireEvent.change(code, { target: { value: "b8rj2q" } });
    expect(code).toHaveValue("B8RJ2Q");
    fireEvent.click(screen.getByRole("button", { name: "Entrar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "room_join",
        expect.objectContaining({ code: "B8RJ2Q", name: "Beto", roomName: null }),
      ),
    );
  });

  it("mostra o erro da sala e continua com o modal aberto", async () => {
    invoke.mockRejectedValue("A sala está cheia (máximo de 8 pessoas).");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Entrar com código" }));
    await screen.findByRole("dialog");
    fireEvent.change(screen.getByPlaceholderText("Como a galera te chama"), { target: { value: "Beto" } });
    fireEvent.change(screen.getByPlaceholderText("B8RJ2Q"), { target: { value: "B8RJ2Q" } });
    fireEvent.click(screen.getByRole("button", { name: "Entrar" }));
    expect(await screen.findByText(/sala está cheia/)).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("troca entre criar e entrar no próprio modal", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Entrar com código" }));
    await screen.findByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "Criar uma sala nova" }));
    expect(screen.getByRole("heading", { name: "Criar uma sala" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Tenho um código" }));
    expect(screen.getByRole("heading", { name: "Entrar numa sala" })).toBeInTheDocument();
  });

  it("usa uma mensagem genérica quando o erro não é texto", async () => {
    invoke.mockRejectedValue(new Error("x"));
    expect(await useRoomStore.getState().join(null, "Ana")).toBe(false);
    expect(useRoomStore.getState().joinError).toBe("Não deu para entrar na sala.");
  });
});

describe("na sala", () => {
  beforeEach(() => {
    useRoomStore.setState({ code: "B8RJ2Q" });
  });

  it("lista os membros com quem está ao vivo e convida a assistir", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    const list = screen.getByRole("list", { name: "Membros da sala" });
    expect(list).toHaveTextContent("Kaique (você)");
    expect(list).toHaveTextContent("AnaAo vivo");
    // Na grade da voz, quem está ao vivo tem o selo e o botão de assistir.
    const grid = screen.getByRole("list", { name: "Quem está na voz" });
    expect(grid).toHaveTextContent("Ana");
    expect(within(grid).getAllByText("Ao vivo")).toHaveLength(1);
    expect(within(grid).getByRole("button", { name: "Assistir a transmissão" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Assistir Beto" })).not.toBeInTheDocument();
  });

  it("assiste um amigo: pede, conecta e a live dele vira um quadro da sala, em foco", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.click(screen.getAllByRole("button", { name: "Assistir Ana" })[0]);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
    expect(screen.getByRole("button", { name: "Conectando…" })).toBeInTheDocument();

    emit("watch-started", ANA);
    expect(screen.getByRole("status", { name: "Sucesso" })).toHaveTextContent("Conectado direto com Ana.");
    // Sem barra embaixo: a live é um quadro como o da nossa, e começa em foco (como no Discord).
    const foco = screen.getByRole("group", { name: "Em foco" });
    expect(within(foco).getByRole("button", { name: "Voltar para a grade (live de Ana em foco)" })).toBeInTheDocument();
    emit("watch-stats", { memberId: ANA.id, stats: { fps: 60, height: 1080, rttMs: 18 } as StreamStats });
    expect(within(foco).getByText("1080p 60fps")).toHaveAttribute("data-video-hole");
    // Medição de quem não assistimos (chegou atrasada) não mexe em nada.
    emit("watch-stats", { memberId: BETO.id, stats: { fps: 30 } as StreamStats });
    expect(useRoomStore.getState().watching[BETO.id]).toBeUndefined();

    fireEvent.contextMenu(within(foco).getByRole("button", { name: /live de Ana/ }));
    const menu = await screen.findByRole("menu", { name: "Opções da live de Ana" });
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: /Silenciar transmissão/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_watch_listening", { memberId: ANA.id, listening: false }),
    );
    expect(within(foco).getByLabelText("Som da transmissão desligado")).toBeInTheDocument();

    fireEvent.click(within(menu).getByRole("menuitem", { name: /Parar de assistir/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_stop_watching", { memberId: ANA.id }));
    expect(screen.queryByRole("button", { name: /live de Ana/ })).not.toBeInTheDocument();
  });

  it("assiste várias lives ao mesmo tempo, cada uma com o seu quadro", async () => {
    const BIA = { ...BETO, live: true };
    render(<App />);
    emit("room-members", [ME, ANA, BIA]);
    emit("watch-started", ANA);
    emit("watch-started", BIA);
    // A última que começou fica em foco; a outra vai para a faixa.
    expect(
      within(screen.getByRole("group", { name: "Em foco" })).getByRole("button", { name: /live de Beto/ }),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("list", { name: "Outros na voz" })).getByRole("button", { name: /live de Ana/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toHaveTextContent("Assistindo Ana e mais 1");
    // Na grade, o quadro de quem já assistimos oferece ver a live, que vem para o foco.
    fireEvent.click(screen.getByRole("button", { name: /Voltar para a grade \(live de Beto em foco\)/ }));
    fireEvent.click(screen.getAllByRole("button", { name: "Ver a transmissão" })[0]);
    expect(
      within(screen.getByRole("group", { name: "Em foco" })).getByRole("button", { name: /live de Ana/ }),
    ).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("room_watch", expect.anything());
  });

  it("assistir pelo botão direito de quem já assistimos também só traz a live para o foco", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("watch-started", ANA);
    fireEvent.click(screen.getByRole("button", { name: /Voltar para a grade \(live de Ana em foco\)/ }));
    fireEvent.contextMenu(screen.getByRole("button", { name: "Pôr Ana em foco" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Assistir a transmissão/ }));
    expect(
      within(screen.getByRole("group", { name: "Em foco" })).getByRole("button", { name: /live de Ana/ }),
    ).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("room_watch", expect.anything());
  });

  it("tem um limite de lives ao mesmo tempo", async () => {
    const watching = Object.fromEntries(
      Array.from({ length: MAX_WATCHING }, (_, i) => [`m${i}`, watchOf({ ...ANA, id: `m${i}` })]),
    );
    useRoomStore.setState({ watching });
    await useRoomStore.getState().watch(BETO);
    expect(invoke).not.toHaveBeenCalledWith("room_watch", expect.anything());
    expect(useToastStore.getState().toasts[0].message).toContain(`até ${MAX_WATCHING} lives`);
  });

  it("ajusta o volume de cada live até 200%, como no Discord, e lembra para a próxima", async () => {
    localStorage.removeItem("squadview.volume");
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("watch-started", ANA);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_watch_volume", { memberId: ANA.id, volume: 1 }));

    fireEvent.contextMenu(screen.getByRole("button", { name: /live de Ana/ }));
    const slider = await screen.findByRole("slider", { name: "Volume da transmissão" });
    expect(slider).toHaveAttribute("max", "200");
    fireEvent.change(slider, { target: { value: "150" } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_watch_volume", { memberId: ANA.id, volume: 1.5 }));
    expect(screen.getByText("150%")).toBeInTheDocument();
    expect(localStorage.getItem("squadview.volume")).toBe("150");
    // As setas do teclado mexem no volume, não no item destacado do menu.
    const key = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true });
    const stop = vi.spyOn(key, "stopPropagation");
    slider.dispatchEvent(key);
    expect(stop).toHaveBeenCalled();
    // Volume de uma live que já fechou não faz nada.
    await useRoomStore.getState().setWatchVolume(BETO.id, 50);
    expect(invoke).not.toHaveBeenCalledWith("set_watch_volume", expect.objectContaining({ memberId: BETO.id }));
  });

  it("mexer no volume com o som desligado liga o som", async () => {
    useRoomStore.setState({
      watching: { [ANA.id]: watchOf(ANA, { listening: false, volume: 0 }) },
      members: [ME, ANA],
    });
    render(<App />);
    fireEvent.contextMenu(screen.getByRole("button", { name: /live de Ana/ }));
    fireEvent.change(await screen.findByRole("slider", { name: "Volume da transmissão" }), { target: { value: "40" } });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_watch_listening", { memberId: ANA.id, listening: true }),
    );
  });

  it("quando a transmissão do amigo acaba, o quadro some e explica", () => {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA) }, members: [ME, ANA] });
    useStreamStore.setState({ pip: ANA.id, fullscreen: true, fullscreenOf: ANA.id });
    render(<App />);
    emit("watch-ended", { memberId: ANA.id, reason: "A transmissão terminou." });
    expect(screen.queryByRole("button", { name: /live de Ana/ })).not.toBeInTheDocument();
    // Estava solta e em tela cheia: a interface volta ao normal.
    expect(useStreamStore.getState()).toMatchObject({ pip: null, fullscreen: false });
    // Fim normal: notificação informativa (não é erro).
    expect(screen.getByRole("status", { name: "Aviso do sistema" })).toHaveTextContent("A transmissão terminou.");
  });

  it("explica quando não deu para conectar e ignora fins de outros membros", () => {
    useRoomStore.setState({ pendingWatch: [ANA.id], members: [ME, ANA] });
    render(<App />);
    emit("watch-ended", { memberId: BETO.id, reason: "outro" });
    expect(useRoomStore.getState().pendingWatch).toEqual([ANA.id]);
    emit("watch-ended", { memberId: ANA.id, reason: "não deu para conectar direto (CGNAT)" });
    expect(useRoomStore.getState().pendingWatch).toEqual([]);
    expect(screen.getByRole("alert", { name: "Erro" })).toHaveTextContent(
      "Não deu para assistirnão deu para conectar direto (CGNAT)",
    );
  });

  it("mostra erros e quedas da sala", () => {
    render(<App />);
    emit("room-error", "Perdemos a conexão com a sala. Entre de novo.");
    expect(screen.getByRole("alert", { name: "Erro" })).toHaveTextContent("Perdemos a conexão");
  });

  it("não pede para assistir de novo quem já está na tela, e mostra falha do pedido", async () => {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA) } });
    await useRoomStore.getState().watch(ANA);
    useRoomStore.setState({ watching: {}, pendingWatch: [ANA.id] });
    await useRoomStore.getState().watch(ANA);
    expect(invoke).not.toHaveBeenCalledWith("room_watch", expect.anything());
    useRoomStore.setState({ pendingWatch: [] });
    invoke.mockRejectedValue("Entre numa sala primeiro.");
    await useRoomStore.getState().watch(ANA);
    expect(useRoomStore.getState().pendingWatch).toEqual([]);
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "Entre numa sala primeiro." });
  });

  it("copia o link de convite da sala", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<App />);
    emit("room-members", [ME]);
    fireEvent.click(screen.getByRole("button", { name: "Copiar link de convite" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("https://kaique292.github.io/squadview/convite/?sala=B8RJ2Q"),
    );
    expect(await screen.findByRole("status", { name: "Sucesso" })).toHaveTextContent("Link copiado");
  });

  it("sai da sala", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.click(screen.getAllByRole("button", { name: "Sair da sala" })[0]);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_leave"));
    expect(screen.getByRole("button", { name: "Entrar com código" })).toBeInTheDocument();
  });

  it("vários amigos ao vivo aparecem juntos", () => {
    render(<App />);
    emit("room-members", [ME, ANA, { ...BETO, live: true }]);
    const grid = screen.getByRole("list", { name: "Quem está na voz" });
    expect(within(grid).getAllByText("Ao vivo")).toHaveLength(2);
  });

  it("avisa quando não consegue copiar o convite", async () => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("negado")) } });
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: "Convidar pessoas" })[0]);
    expect(await screen.findByRole("status", { name: "Aviso" })).toHaveTextContent(
      "O link é https://kaique292.github.io/squadview/convite/?sala=B8RJ2Q",
    );
  });

  it("mostra há quanto tempo estamos na sala, como o canal de voz", () => {
    vi.useFakeTimers();
    useRoomStore.setState({ joinedAt: Date.now() - (2 * 3600 + 15 * 60 + 6) * 1000 });
    render(<App />);
    expect(screen.getByLabelText("Tempo na sala")).toHaveTextContent("2:15:06");
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByLabelText("Tempo na sala")).toHaveTextContent("2:15:07");
    vi.useRealTimers();
  });

  it("recolhe e abre a lista de membros pela categoria", () => {
    render(<App />);
    emit("room-members", [ME, BETO]);
    const category = screen.getByRole("button", { name: /Na sala — 2/ });
    expect(category).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(category);
    expect(category).toHaveAttribute("aria-expanded", "false");
  });

  it("mostra a sala ativa na barra com a pílula e o ponto de alguém ao vivo", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const rail = screen.getByRole("navigation", { name: "Salas" });
    expect(rail.querySelector("[aria-current=page]")).toHaveTextContent("B8R");
    expect(screen.getByLabelText("Alguém ao vivo")).toBeInTheDocument();
  });

  it("o painel do usuário mostra o que estamos fazendo", () => {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA) }, members: [ME, ANA] });
    render(<App />);
    const panel = screen.getByRole("contentinfo");
    expect(panel).toHaveTextContent("Assistindo Ana");
    // O painel tem o microfone e o fone da voz, como no Discord.
    expect(within(panel).getByRole("button", { name: "Silenciar microfone" })).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Ensurdecer" })).toBeInTheDocument();
  });
});

describe("nome da sala e apelido", () => {
  beforeEach(() => useRoomStore.setState({ code: "B8RJ2Q", joinedAt: Date.now() }));

  it("cria a sala com o nome escolhido e mostra o nome no lugar do código", async () => {
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q", roomName: "Galera do LoL" } : undefined),
    );
    useRoomStore.setState({ code: null, members: [] });
    render(<App />);
    // O "+" da barra de salas agora adiciona servidores: a sala rápida é o botão da lateral.
    fireEvent.click(screen.getByRole("button", { name: "Criar sala" }));
    await screen.findByRole("dialog");
    fireEvent.change(screen.getByPlaceholderText("Como a galera te chama"), { target: { value: "Kaique" } });
    fireEvent.change(screen.getByPlaceholderText("Sala de Kaique"), { target: { value: "Galera do LoL" } });
    fireEvent.click(screen.getAllByRole("button", { name: "Criar sala" }).at(-1)!);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "room_join",
        expect.objectContaining({ code: null, name: "Kaique", roomName: "Galera do LoL" }),
      ),
    );
    emit("room-members", [ME]);
    await waitFor(() => expect(sidebarButton(/Galera do LoL/)).toBeInTheDocument());
  });

  it("renomeia a sala pelo menu e atualiza quando alguém renomeia", async () => {
    useRoomStore.setState({ roomName: "Sala 1" });
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.pointerDown(sidebarButton(/Sala 1/)!, { button: 0, ctrlKey: false });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Editar nome da sala/ }));
    const input = await screen.findByRole("textbox", { name: "Nome da sala" });
    fireEvent.change(input, { target: { value: "Squad" } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_rename", { name: "Squad" }));

    emit("room-name", "Squad");
    await waitFor(() => expect(sidebarButton(/Squad/)).toBeInTheDocument());
  });

  it("troca o apelido pelo botão direito no próprio nome", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    const list = screen.getByRole("list", { name: "Membros da sala" });
    fireEvent.contextMenu(within(list).getByText(ME.name));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Alterar apelido/ }));
    const input = await screen.findByRole("textbox", { name: "Apelido" });
    fireEvent.change(input, { target: { value: "K.A" } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_set_name", { name: "K.A" }));
  });

  it("no amigo ao vivo, o botão direito oferece assistir", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.contextMenu(within(screen.getByRole("list", { name: "Membros da sala" })).getByText(ANA.name));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Assistir a transmissão/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
  });
});

describe("queda da conexão com a sala", () => {
  beforeEach(() => useRoomStore.setState({ code: "B8RJ2Q", joinedAt: Date.now() }));

  it("avisa que está reconectando e quando volta", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("room-connection", "reconnecting");
    expect(await screen.findByText("Reconectando…")).toBeInTheDocument();
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "warning", title: "Reconectando" });
    emit("room-connection", "connected");
    expect(screen.queryByText("Reconectando…")).not.toBeInTheDocument();
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "success", message: "De volta à sala." });
  });

  it("se não der para voltar, sai da sala e explica", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    emit("room-connection", "lost");
    expect(useRoomStore.getState().code).toBeNull();
    expect(screen.getByRole("button", { name: "Entrar com código" })).toBeInTheDocument();
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", title: "Conexão perdida" });
  });
});

describe("convite por link", () => {
  it("abre o modal de entrar com o código preenchido, sem entrar sozinho", async () => {
    useRoomStore.setState({ code: null, members: [] });
    render(<App />);
    act(() => useRoomStore.getState().setInvite("F5B6S9"));
    expect(await screen.findByRole("dialog", { name: "Entrar numa sala" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("B8RJ2Q")).toHaveValue("F5B6S9");
    expect(invoke).not.toHaveBeenCalledWith("room_join", expect.anything());
  });

  it("convite da sala em que já estamos só avisa", () => {
    useRoomStore.setState({ code: "B8RJ2Q", invite: null });
    act(() => useRoomStore.getState().setInvite("B8RJ2Q"));
    expect(useRoomStore.getState().invite).toBeNull();
    expect(useToastStore.getState().toasts[0].message).toBe("Você já está na sala B8RJ2Q.");
  });
});

/** Botão da lateral (o mesmo nome aparece no ícone da sala, na barra de salas). */
function sidebarButton(name: RegExp) {
  return screen.getAllByRole("button", { name }).find((b) => !b.closest('nav[aria-label="Salas"]'));
}
