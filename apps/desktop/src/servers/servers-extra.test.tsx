import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { listenToFriendsEvents, useFriendsStore } from "../friends/friends-store";
import { listenToRoomEvents, useRoomStore, useServerInviteStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { ServerIcon } from "./ServerIcon";
import { myRole, onServerMessage, type Server, serverErrorText, useServersStore } from "./servers-store";

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
const ADM = "222222222222222222";
const ZE = "333333333333333333";
const ID = "0000000000000001";

function server(partial: Partial<Server> = {}): Server {
  return {
    id: ID,
    name: "Turma",
    icon: null,
    rooms: [{ id: "00000001", name: "Geral", code: "GERAL2" }],
    members: [{ id: ME, name: "Kaique", avatar: null, role: "owner" }],
    ...partial,
  };
}

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function actions() {
  return invoke.mock.calls.filter(([cmd]) => cmd === "friends_action").map(([, args]) => args.action);
}

function serverMenu() {
  return screen.getAllByRole("button", { name: /Turma/ }).find((b) => b.hasAttribute("aria-haspopup"))!;
}

async function openSettings(tab: string) {
  fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
  fireEvent.click(within(await screen.findByRole("menu")).getByText("Configurações do servidor"));
  const dialog = await screen.findByRole("dialog", { name: "Configurações de Turma" });
  fireEvent.click(within(dialog).getByRole("tab", { name: tab }));
  // O conteúdo da aba troca com animação: espera a aba escolhida aparecer.
  await waitFor(() => expect(within(dialog).getByRole("tabpanel")).not.toHaveAttribute("data-leaving"));
  return dialog;
}

let offs: (() => void)[] = [];
/** Link que o sistema entregou ao abrir o app (só no teste do link). */
let pendingLink: string | null = null;

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(
      cmd === "friends_status"
        ? "online"
        : cmd === "room_join"
          ? { code: "GERAL2" }
          : cmd === "take_pending_invite"
            ? pendingLink
            : undefined,
    ),
  );
  handlers.clear();
  useToastStore.setState({ toasts: [] });
  useAccountStore.setState({ profile: { id: ME, username: "kaique", globalName: "Kaique", avatar: null } });
  useRoomStore.setState({ code: null, roomName: null, members: [], watching: null });
  useFriendsStore.setState({ status: "signed-out", open: false });
  useServersStore.setState({ servers: [], selected: null, invites: {} });
  useServerInviteStore.setState({ invite: null });
  offs = [await listenToFriendsEvents(), await listenToRoomEvents()];
});

afterEach(() => offs.forEach((off) => off()));

describe("servidores: mais casos", () => {
  it("o ícone aparece no lugar das iniciais", () => {
    const { container } = render(<ServerIcon name="Turma" icon="data:image/png;base64,iVBORw0KGgo=" size={40} />);
    expect(container.querySelector("img")).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
  });

  it("link de servidor pelo sistema abre o convite já preenchido", async () => {
    pendingLink = `servidor:${ID}.AbCdEf234567`;
    emit("invite", null);
    await waitFor(() => expect(useServerInviteStore.getState().invite).toBe(`${ID}.AbCdEf234567`));
    expect(useRoomStore.getState().invite).toBeNull();
    pendingLink = null;
  });

  it("entrar pela página do servidor; clicar na sala em que já estamos não entra de novo", async () => {
    useServersStore.setState({ servers: [server()], selected: ID });
    render(<App />);
    expect(screen.getByText(/1 membro\./)).toBeInTheDocument();
    fireEvent.click(within(screen.getByRole("list", { name: "Entrar numa sala" })).getByRole("button"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_join", expect.objectContaining({ code: "GERAL2" })));
    invoke.mockClear();
    act(() => useRoomStore.setState({ code: "GERAL2" }));
    await act(() => useServersStore.getState().enterRoom(server(), server().rooms[0]));
    expect(invoke).not.toHaveBeenCalledWith("room_join", expect.anything());
  });

  it("menu do servidor: convidar e criar sala", async () => {
    useServersStore.setState({ servers: [server()], selected: ID });
    render(<App />);
    fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
    fireEvent.click(within(await screen.findByRole("menu")).getByText("Convidar pessoas"));
    fireEvent.pointerDown(serverMenu(), { button: 0, ctrlKey: false });
    fireEvent.click(within(await screen.findByRole("menu")).getByText("Criar sala"));
    expect(await screen.findByRole("dialog", { name: "Criar sala" })).toBeInTheDocument();
    expect(actions()).toContainEqual({ type: "server-action", server: ID, op: { kind: "invite" } });
  });

  it("renomear o servidor e uma sala nas configurações", async () => {
    useServersStore.setState({
      servers: [server({ rooms: [...server().rooms, { id: "00000002", name: "Ranked", code: "RANKD3" }] })],
      selected: ID,
    });
    render(<App />);
    const dialog = await openSettings("Visão geral");
    expect(within(dialog).getByText(/1 membro · 2 salas/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: /Renomear/ }));
    const nome = await screen.findByRole("dialog", { name: "Nome do servidor" });
    fireEvent.change(within(nome).getByRole("textbox"), { target: { value: "Turma nova" } });
    fireEvent.click(within(nome).getByRole("button", { name: "Salvar" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: ID,
        op: { kind: "update", name: "Turma nova" },
      }),
    );

    fireEvent.click(within(dialog).getByRole("tab", { name: "Salas" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Renomear Ranked" }));
    const sala = await screen.findByRole("dialog", { name: "Nome da sala" });
    fireEvent.change(within(sala).getByRole("textbox"), { target: { value: "Casual" } });
    fireEvent.click(within(sala).getByRole("button", { name: "Salvar" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: ID,
        op: { kind: "room-rename", room: "00000002", name: "Casual" },
      }),
    );
  });

  it("administrador tira membros, mas não muda papéis nem tira outro administrador", async () => {
    const comoAdm = server({
      members: [
        { id: "111111111111111111", name: "Dona", avatar: "a".repeat(32), role: "owner" },
        { id: ME, name: "Kaique", avatar: null, role: "admin" },
        { id: ADM, name: "Outro adm", avatar: null, role: "admin" },
        { id: ZE, name: "Zé", avatar: null, role: "member" },
      ],
    });
    useServersStore.setState({ servers: [comoAdm], selected: ID });
    render(<App />);
    const dialog = await openSettings("Membros");
    expect(within(dialog).queryByRole("button", { name: /administrador/ })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Tirar Outro adm do servidor" })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Tirar Dona do servidor" })).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Tirar Zé do servidor" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({ type: "server-action", server: ID, op: { kind: "kick", member: ZE } }),
    );
  });

  it("o dono tira o administrador do cargo", async () => {
    useServersStore.setState({
      servers: [server({ members: [...server().members, { id: ADM, name: "Adm", avatar: null, role: "admin" }] })],
      selected: ID,
    });
    render(<App />);
    const dialog = await openSettings("Membros");
    fireEvent.click(within(dialog).getByRole("button", { name: "Tirar administrador" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: ID,
        op: { kind: "role", member: ADM, role: "member" },
      }),
    );
  });

  it("mensagens do registro: servidor muda no lugar, seleção some se ele sumir, copiar falhando não quebra", async () => {
    onServerMessage({ type: "servers", servers: [server()] } as never);
    act(() => useServersStore.getState().select(ID));
    onServerMessage({ type: "server", server: server({ name: "Turma 2" }) } as never);
    expect(useServersStore.getState().servers.map((s) => s.name)).toEqual(["Turma 2"]);
    expect(useServersStore.getState().selected).toBe(ID);
    onServerMessage({ type: "servers", servers: [server()] } as never);
    expect(useServersStore.getState().selected).toBe(ID);
    onServerMessage({ type: "servers", servers: [] } as never);
    expect(useServersStore.getState().selected).toBeNull();
    // Sumir um servidor que não estava na lista: sem aviso.
    onServerMessage({ type: "server-removed", id: "ffffffffffffffff" } as never);
    expect(useToastStore.getState().toasts).toHaveLength(0);

    Object.assign(navigator, { clipboard: { writeText: vi.fn(async () => Promise.reject(new Error("negado"))) } });
    onServerMessage({ type: "server-invite", server: ID, invite: `${ID}.AbCdEf234567` } as never);
    await Promise.resolve();
    expect(useServersStore.getState().invites[ID]).toBe(`squadview://servidor/${ID}.AbCdEf234567`);
    expect(onServerMessage({ type: "state" })).toBe(false);
  });

  it("texto dos erros: só os de servidor, e o too-many conforme o último pedido", async () => {
    expect(serverErrorText("unknown-person")).toBeNull();
    await useServersStore.getState().act(ID, { kind: "room-create", name: "X" });
    expect(serverErrorText("too-many")).toMatch(/20 salas/);
    await useServersStore.getState().join(`${ID}.AbCdEf234567`);
    expect(serverErrorText("too-many")).toMatch(/entrar neste/);
    await useServersStore.getState().act(ID, { kind: "leave" });
    expect(serverErrorText("too-many")).toBeNull();
    expect(myRole(server(), "outro")).toBeNull();
  });
});
