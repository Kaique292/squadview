import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { listenToFriendsEvents, useFriendsStore } from "../friends/friends-store";
import { listenToRoomEvents, useRoomStore, useServerInviteStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import {
  clearServers,
  parseServerInvite,
  type Server,
  serverInitials,
  serverInviteUrl,
  serverOfRoom,
  useServersStore,
} from "./servers-store";

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

const ME = "999999999999999999";
const BETO = "222222222222222222";
const INVITE = "0123456789abcdef.AbCdEf234567";

function server(partial: Partial<Server> = {}): Server {
  return {
    id: "0000000000000001",
    name: "Turma do bairro",
    icon: null,
    rooms: [
      { id: "00000001", name: "Geral", code: "GERAL2" },
      { id: "00000002", name: "Ranked", code: "RANKD3" },
    ],
    members: [
      { id: ME, name: "Kaique", avatar: null, role: "owner" },
      { id: BETO, name: "Beto", avatar: null, role: "member" },
    ],
    ...partial,
  };
}

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

/** O botão do nome do servidor no topo da lateral (abre o menu dele). */
function serverMenu() {
  return screen.getAllByRole("button", { name: /Turma do bairro/ }).find((b) => b.hasAttribute("aria-haspopup"))!;
}

/** O que foi pedido ao registro pela conexão dos amigos. */
function actions() {
  return invoke.mock.calls.filter(([cmd]) => cmd === "friends_action").map(([, args]) => args.action);
}

const clipboard = vi.fn(async () => {});
let offs: (() => void)[] = [];

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "friends_status" ? "online" : cmd === "room_join" ? { code: "GERAL2" } : undefined),
  );
  handlers.clear();
  localStorage.clear();
  clipboard.mockClear();
  Object.assign(navigator, { clipboard: { writeText: clipboard } });
  useToastStore.setState({ toasts: [] });
  useAccountStore.setState({ profile: { id: ME, username: "kaique", globalName: "Kaique", avatar: null } });
  useRoomStore.setState({ code: null, roomName: null, members: [], watching: null });
  useFriendsStore.setState({ status: "signed-out", open: false });
  useServersStore.setState({ servers: [], selected: null, invites: {} });
  useServerInviteStore.setState({ invite: null });
  offs = [await listenToFriendsEvents(), await listenToRoomEvents()];
});

afterEach(() => offs.forEach((off) => off()));

describe("servidores: regras da interface", () => {
  it("lê o convite de um link ou do texto colado", () => {
    expect(serverInviteUrl(INVITE)).toBe(`https://kaique292.github.io/squadview/convite/?servidor=${INVITE}`);
    expect(parseServerInvite(serverInviteUrl(INVITE))).toBe(INVITE);
    // Convites antigos (squadview://) continuam valendo ao colar.
    expect(parseServerInvite(`squadview://servidor/${INVITE}`)).toBe(INVITE);
    expect(parseServerInvite(`  ${INVITE} `)).toBe(INVITE);
    expect(parseServerInvite("squadview://join/B8RJ2Q")).toBeNull();
  });

  it("iniciais do nome quando não há ícone", () => {
    expect(serverInitials("Turma do Bairro")).toBe("TdB");
    expect(serverInitials("squad")).toBe("SQ");
  });

  it("acha o servidor de uma sala pelo código", () => {
    expect(serverOfRoom([server()], "RANKD3")?.name).toBe("Turma do bairro");
    expect(serverOfRoom([server()], "OUTRA4")).toBeNull();
    expect(serverOfRoom([server()], null)).toBeNull();
  });
});

describe("servidores na barra de salas", () => {
  it("os servidores do registro aparecem na barra e abrem as salas na lateral", async () => {
    render(<App />);
    emit("friends-event", { type: "servers", servers: [server()] });
    const rail = screen.getByRole("navigation", { name: "Salas" });
    fireEvent.click(within(rail).getByRole("button", { name: "Turma do bairro" }));
    const salas = screen.getByRole("list", { name: "Salas do servidor" });
    expect(
      within(salas)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Geral", "Ranked"]);
    // Sem estar numa sala, o meio mostra o servidor com as salas para entrar.
    expect(screen.getByRole("region", { name: "Turma do bairro" })).toBeInTheDocument();

    fireEvent.click(within(salas).getByRole("button", { name: "Ranked" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "room_join",
        expect.objectContaining({ code: "RANKD3", roomName: "Ranked", name: "Kaique" }),
      ),
    );
  });

  it("dentro da sala do servidor, ela não aparece de novo como sala avulsa", () => {
    useRoomStore.setState({ code: "GERAL2", roomName: "Geral", members: [{ id: "a", name: "Kaique", live: false }] });
    useServersStore.setState({ servers: [server()], selected: "0000000000000001" });
    render(<App />);
    const rail = screen.getByRole("navigation", { name: "Salas" });
    expect(within(rail).queryByRole("button", { name: "Geral" })).not.toBeInTheDocument();
    const geral = within(screen.getByRole("list", { name: "Salas do servidor" })).getByRole("button", {
      name: /Geral/,
    });
    expect(geral).toHaveAttribute("aria-current", "location");
    expect(screen.getByRole("list", { name: "Membros da sala" })).toBeInTheDocument();
  });

  it("criar um servidor: abre na hora, e o dono cria salas pelo menu", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Adicionar um servidor" }));
    const dialog = await screen.findByRole("dialog", { name: "Criar um servidor" });
    const nome = within(dialog).getByRole("textbox", { name: "Nome do servidor" });
    expect(nome).toHaveValue("Servidor de Kaique");
    fireEvent.change(nome, { target: { value: "Turma do bairro" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Criar" }));
    await waitFor(() => expect(actions()).toContainEqual({ type: "server-create", name: "Turma do bairro" }));

    emit("friends-event", { type: "server", server: server({ rooms: [server().rooms[0]] }) });
    expect(screen.getByRole("list", { name: "Salas do servidor" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Criar sala" }));
    const criar = await screen.findByRole("dialog", { name: "Criar sala" });
    fireEvent.change(within(criar).getByRole("textbox"), { target: { value: "Jogos" } });
    fireEvent.click(within(criar).getByRole("button", { name: "Salvar" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: "0000000000000001",
        op: { kind: "room-create", name: "Jogos" },
      }),
    );
  });

  it("entrar com o link de convite (aberto pelo link do sistema também)", async () => {
    render(<App />);
    act(() => useServerInviteStore.setState({ invite: INVITE }));
    const dialog = await screen.findByRole("dialog", { name: "Entrar num servidor" });
    expect(within(dialog).getByRole("textbox", { name: "Link de convite" })).toHaveValue(INVITE);
    fireEvent.click(within(dialog).getByRole("button", { name: "Entrar" }));
    await waitFor(() => expect(actions()).toContainEqual({ type: "server-join", invite: INVITE }));
    expect(useServerInviteStore.getState().invite).toBeNull();
  });

  it("convite que não é de servidor avisa e não envia", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Adicionar um servidor" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Tenho um convite" }));
    const campo = within(dialog).getByRole("textbox", { name: "Link de convite" });
    fireEvent.change(campo, { target: { value: "squadview://join/B8RJ2Q" } });
    expect(within(dialog).getByText(/não parece um convite de servidor/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Entrar" }));
    expect(actions().some((a) => a.type === "server-join")).toBe(false);
  });

  it("sem o Discord, explica e oferece entrar; a sala rápida continua", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "friends_status" ? "signed-out" : undefined));
    useFriendsStore.setState({ status: "signed-out" });
    useAccountStore.setState({ profile: null });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Adicionar um servidor" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/usam sua conta do Discord/)).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Criar" })).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: /Só uma sala rápida/ }));
    expect(await screen.findByRole("dialog", { name: "Criar uma sala" })).toBeInTheDocument();
  });

  it("convidar copia o link e avisa", async () => {
    useServersStore.setState({ servers: [server()], selected: "0000000000000001" });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Copiar link de convite do servidor" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({ type: "server-action", server: "0000000000000001", op: { kind: "invite" } }),
    );
    emit("friends-event", { type: "server-invite", server: "0000000000000001", invite: INVITE });
    await waitFor(() => expect(clipboard).toHaveBeenCalledWith(serverInviteUrl(INVITE)));
    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]).toMatchObject({ message: expect.stringContaining("copiado") }),
    );
  });

  it("membro comum não vê as configurações nem cria salas, e pode sair", async () => {
    const comoMembro = server({
      members: [
        { id: BETO, name: "Beto", avatar: null, role: "owner" },
        { id: ME, name: "Kaique", avatar: null, role: "member" },
      ],
    });
    useServersStore.setState({ servers: [comoMembro], selected: comoMembro.id });
    render(<App />);
    expect(screen.queryByRole("button", { name: "Criar sala" })).not.toBeInTheDocument();
    fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
    const menu = await screen.findByRole("menu");
    expect(within(menu).queryByText("Configurações do servidor")).not.toBeInTheDocument();
    fireEvent.click(within(menu).getByText("Sair do servidor"));
    await waitFor(() =>
      expect(actions()).toContainEqual({ type: "server-action", server: comoMembro.id, op: { kind: "leave" } }),
    );
  });

  it("dono muda papéis e tira gente nas configurações", async () => {
    useServersStore.setState({ servers: [server()], selected: "0000000000000001" });
    render(<App />);
    fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
    fireEvent.click(within(await screen.findByRole("menu")).getByText("Configurações do servidor"));
    const dialog = await screen.findByRole("dialog", { name: "Configurações de Turma do bairro" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Membros" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Tornar administrador" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tirar Beto do servidor" }));
    await waitFor(() =>
      expect(actions()).toEqual(
        expect.arrayContaining([
          { type: "server-action", server: "0000000000000001", op: { kind: "role", member: BETO, role: "admin" } },
          { type: "server-action", server: "0000000000000001", op: { kind: "kick", member: BETO } },
        ]),
      ),
    );
    // A última sala não se apaga; com duas, dá.
    fireEvent.click(within(dialog).getByRole("tab", { name: "Salas" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Apagar Ranked" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: "0000000000000001",
        op: { kind: "room-delete", room: "00000002" },
      }),
    );
  });

  it("apagar o servidor só pede confirmação (sem digitar o nome); cancelar não apaga", async () => {
    useServersStore.setState({ servers: [server()], selected: "0000000000000001" });
    render(<App />);
    fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
    fireEvent.click(within(await screen.findByRole("menu")).getByText("Apagar servidor"));
    let dialog = await screen.findByRole("dialog", { name: "Apagar Turma do bairro?" });
    expect(within(dialog).queryByRole("textbox")).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(actions().some((a) => a.op?.kind === "delete")).toBe(false);

    // O botão direito no ícone do servidor abre o mesmo menu.
    const rail = screen.getByRole("navigation", { name: "Salas" });
    fireEvent.contextMenu(within(rail).getByRole("button", { name: "Turma do bairro" }));
    const menu = await screen.findByRole("menu", { name: "Opções de Turma do bairro" });
    expect(within(menu).getByText("Convidar pessoas")).toBeInTheDocument();
    expect(within(menu).getByText("Configurações do servidor")).toBeInTheDocument();
    expect(within(menu).getByText("Criar sala")).toBeInTheDocument();
    fireEvent.click(within(menu).getByText("Apagar servidor"));
    dialog = await screen.findByRole("dialog", { name: "Apagar Turma do bairro?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apagar servidor" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({ type: "server-action", server: "0000000000000001", op: { kind: "delete" } }),
    );
  });

  it("mostra quem está em cada sala do servidor, mesmo com a gente fora dela (ADR 0018)", () => {
    render(<App />);
    emit("friends-event", { type: "servers", servers: [server()] });
    act(() => useServersStore.getState().select("0000000000000001"));
    const salas = screen.getByRole("list", { name: "Salas do servidor" });
    expect(within(salas).queryByRole("list", { name: "Quem está na sala" })).toBeNull();

    emit("friends-event", {
      type: "server-presence",
      server: "0000000000000001",
      presence: { "00000001": [], "00000002": [{ id: BETO, live: true }] },
    });
    const ranked = within(salas).getByRole("button", { name: "Ranked" }).closest("li")!;
    const quem = within(ranked).getByRole("list", { name: "Quem está na sala" });
    expect(within(quem).getByText("Beto")).toBeInTheDocument();
    expect(within(quem).getByText("Ao vivo")).toHaveClass("uppercase");
    const geral = within(salas).getByRole("button", { name: "Geral" }).closest("li")!;
    expect(within(geral).queryByRole("list", { name: "Quem está na sala" })).toBeNull();

    // Quem não é (mais) membro não aparece; sair da conta limpa tudo.
    emit("friends-event", {
      type: "server-presence",
      server: "0000000000000001",
      presence: { "00000002": [{ id: "123456789012345678", live: false }] },
    });
    expect(within(ranked).queryByText("Beto")).toBeNull();
    act(() => clearServers());
    expect(useServersStore.getState().presence).toEqual({});
  });

  it("o ícone do servidor mostra o alto-falante com gente em call, e some quando esvazia", () => {
    render(<App />);
    emit("friends-event", { type: "servers", servers: [server()] });
    const rail = screen.getByRole("navigation", { name: "Salas" });
    expect(within(rail).queryByLabelText("Gente em call")).toBeNull();
    emit("friends-event", {
      type: "server-presence",
      server: "0000000000000001",
      presence: { "00000001": [{ id: BETO, live: false }], "00000002": [] },
    });
    expect(within(rail).getByLabelText("Gente em call")).toBeInTheDocument();
    emit("friends-event", { type: "server-presence", server: "0000000000000001", presence: { "00000001": [] } });
    expect(within(rail).queryByLabelText("Gente em call")).toBeNull();
  });

  it("quem acabou de entrar no servidor aparece na sala assim que o retrato com ele chega", () => {
    const NOVO = "444444444444444444";
    render(<App />);
    emit("friends-event", { type: "servers", servers: [server()] });
    act(() => useServersStore.getState().select("0000000000000001"));
    // A presença dele chegou antes do retrato do servidor com ele na lista.
    emit("friends-event", {
      type: "server-presence",
      server: "0000000000000001",
      presence: { "00000001": [{ id: NOVO, live: false }] },
    });
    expect(screen.queryByText("Dani")).toBeNull();
    emit("friends-event", {
      type: "server",
      server: server({ members: [...server().members, { id: NOVO, name: "Dani", avatar: null, role: "member" }] }),
    });
    expect(screen.getByText("Dani")).toBeInTheDocument();
  });

  it("saiu ou foi tirado: o servidor some da barra com aviso; erros do servidor viram texto", async () => {
    render(<App />);
    emit("friends-event", { type: "servers", servers: [server()] });
    act(() => useServersStore.getState().select("0000000000000001"));
    emit("friends-event", { type: "server-removed", id: "0000000000000001" });
    expect(useServersStore.getState()).toMatchObject({ servers: [], selected: null });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "Você não está mais em Turma do bairro." });

    await act(() => useServersStore.getState().create("Mais um"));
    emit("friends-event", { type: "error", code: "too-many" });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ title: "Servidor", message: /20 servidores/ });
    // Um "too-many" que não veio de servidor continua sendo dos amigos.
    emit("friends-event", { type: "error", code: "too-many" });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ title: "Amigos" });
    emit("friends-event", { type: "error", code: "not-allowed" });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ message: /dono ou os administradores/ });
  });

  it("saindo da conta, os servidores somem da tela", () => {
    useServersStore.setState({ servers: [server()], selected: "0000000000000001" });
    emit("friends-status", "signed-out");
    expect(useServersStore.getState()).toMatchObject({ servers: [], selected: null });
  });
});
