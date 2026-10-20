import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";
import { useToastStore } from "../ui/toast-store";
import { listenToFriendsEvents, useFriendsStore } from "./friends-store";

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

const ME = { id: "999999999999999999", name: "Kaique", avatar: null };
const ANA = { id: "111111111111111111", name: "Ana", avatar: null };
const BETO = { id: "222222222222222222", name: "Beto", avatar: null };
const ONLINE = { online: true, inRoom: false, room: null, live: false };
const OFFLINE = { online: false, inRoom: false, room: null, live: false };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function state(partial: Record<string, unknown> = {}) {
  emit("friends-event", { type: "state", me: ME, friends: [], incoming: [], outgoing: [], recents: [], ...partial });
}

/** O aviso mais novo (a lista começa pelo mais recente). */
function lastToast() {
  return useToastStore.getState().toasts[0];
}

let offs: (() => void)[] = [];

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "friends_status" ? "online" : cmd === "room_join" ? { code: "B8RJ2Q" } : undefined),
  );
  handlers.clear();
  localStorage.clear();
  useToastStore.setState({ toasts: [] });
  useAccountStore.setState({ profile: null });
  useRoomStore.setState({ code: null, roomName: null, members: [], watching: null });
  useStreamStore.setState({ status: "idle" });
  useFriendsStore.setState({
    status: "signed-out",
    open: false,
    me: null,
    friends: [],
    incoming: [],
    outgoing: [],
    recents: [],
    joinRequests: [],
    invites: [],
    waitingFor: null,
  });
  offs.forEach((off) => off());
  offs = [await listenToFriendsEvents()];
});

function openFriends() {
  render(<App />);
  fireEvent.click(within(screen.getByRole("navigation", { name: "Salas" })).getByRole("button", { name: "Amigos" }));
  return screen.getByRole("region", { name: "Amigos" });
}

describe("página de amigos", () => {
  it("sem amigos e sem ninguém online, explica o que fazer", () => {
    const page = openFriends();
    state();
    expect(within(page).getByText(/Ainda sem amigos/)).toBeInTheDocument();
    state({ friends: [{ ...BETO, direct: false, presence: OFFLINE }] });
    expect(within(page).getByText("Ninguém online agora.")).toBeInTheDocument();
    fireEvent.click(within(page).getByRole("button", { name: "Pendentes" }));
    expect(within(page).getByText("Nenhum pedido de amizade pendente.")).toBeInTheDocument();
    fireEvent.click(within(page).getByRole("button", { name: "Recentes" }));
    expect(within(page).getByText(/aparece aqui/)).toBeInTheDocument();
  });

  it("conectando, mostra que está carregando; sem login, pede para entrar", () => {
    const page = openFriends();
    emit("friends-status", "connecting");
    expect(within(page).getByText("Conectando aos amigos…")).toBeInTheDocument();
    emit("friends-status", "signed-out");
    expect(within(page).getByRole("heading", { name: "Entre para ver seus amigos" })).toBeInTheDocument();
  });

  it("recusa pedidos recebidos e cancela os enviados", async () => {
    const page = openFriends();
    state({ incoming: [ANA], outgoing: [BETO] });
    fireEvent.click(within(page).getByRole("button", { name: /Pendentes/ }));
    expect(within(page).getByText("Recebidos — 1")).toBeInTheDocument();
    expect(within(page).getByText("Enviados — 1")).toBeInTheDocument();
    fireEvent.click(within(page).getByRole("button", { name: "Recusar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "friend-decline", id: ANA.id } }),
    );
    fireEvent.click(within(page).getByRole("button", { name: "Cancelar pedido" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "friend-cancel", id: BETO.id } }),
    );
  });

  it("em Recentes, quem já recebeu pedido aparece sem o botão de adicionar", () => {
    const page = openFriends();
    state({ recents: [BETO], outgoing: [BETO] });
    fireEvent.click(within(page).getByRole("button", { name: "Recentes" }));
    expect(within(page).getByText("Pedido enviado")).toBeInTheDocument();
    expect(within(page).queryByRole("button", { name: /Adicionar amigo/ })).not.toBeInTheDocument();
  });

  it("numa sala, chama um amigo online para ela", async () => {
    useRoomStore.setState({ code: "B8RJ2Q" });
    const page = openFriends();
    state({ friends: [{ ...ANA, direct: false, presence: ONLINE }] });
    fireEvent.click(within(page).getByRole("button", { name: "Chamar para a sala" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "invite", id: ANA.id } }),
    );
  });

  it("pelo menu do amigo, deixa entrar direto e desfaz a amizade", async () => {
    const page = openFriends();
    state({ friends: [{ ...ANA, direct: true, presence: ONLINE }] });
    const menu = () => {
      fireEvent.pointerDown(within(page).getByRole("button", { name: "Mais opções de Ana" }), {
        button: 0,
        ctrlKey: false,
      });
      return screen.findByRole("menu");
    };
    const direto = within(await menu()).getByRole("menuitemcheckbox", { name: /Entra direto/ });
    expect(direto).toHaveAttribute("aria-checked", "true");
    fireEvent.click(direto);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", {
        action: { type: "set-direct", id: ANA.id, direct: false },
      }),
    );
    fireEvent.click(within(await menu()).getByRole("menuitem", { name: "Desfazer amizade" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "unfriend", id: ANA.id } }),
    );
  });

  it("mostra o erro quando o registro recusa uma ação", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "friends_action" ? Promise.reject("Registro fora do ar.") : Promise.resolve(undefined),
    );
    await useFriendsStore.getState().act({ type: "unfriend", id: ANA.id });
    expect(lastToast()).toMatchObject({ kind: "error", message: "Registro fora do ar.", title: "Amigos" });
  });
});

describe("respostas do registro", () => {
  it("explica pedidos recusados, amigos fora de sala e amizades desfeitas", () => {
    useFriendsStore.setState({ waitingFor: "Ana" });
    emit("friends-event", { type: "join-result", result: { kind: "denied", by: "Caio" } });
    expect(lastToast()?.message).toBe("Caio recusou o pedido.");
    expect(useFriendsStore.getState().waitingFor).toBeNull();

    useFriendsStore.setState({ waitingFor: "Ana" });
    emit("friends-event", { type: "join-result", result: { kind: "not-in-room" } });
    expect(lastToast()?.message).toBe("Ana não está numa sala agora.");
    emit("friends-event", { type: "join-result", result: { kind: "not-in-room" } });
    expect(lastToast()?.message).toBe("Seu amigo não está numa sala agora.");

    emit("friends-event", { type: "join-result", result: { kind: "not-friend" } });
    expect(lastToast()?.message).toBe("Vocês não são mais amigos.");
  });

  it("avisa de pedido de amizade novo e traduz os erros conhecidos", () => {
    emit("friends-event", { type: "friend-request", from: ANA });
    expect(lastToast()?.message).toBe("Ana quer ser seu amigo. Veja em Amigos.");
    emit("friends-event", { type: "error", code: "too-many" });
    expect(lastToast()?.message).toBe("Muitos pedidos pendentes. Espere responderem.");
    emit("friends-event", { type: "error", code: "codigo-novo" });
    expect(lastToast()?.message).toBe("Não deu certo agora. Tente de novo.");
  });

  it("um convite novo do mesmo amigo substitui o anterior", () => {
    emit("friends-event", { type: "invited", from: ANA, code: "AAAAAA", room: null });
    emit("friends-event", { type: "invited", from: ANA, code: "B8RJ2Q", room: null });
    expect(useFriendsStore.getState().invites.map((i) => i.code)).toEqual(["B8RJ2Q"]);
  });

  it("sair da conta limpa a lista de amigos", () => {
    state({ friends: [{ ...ANA, direct: false, presence: ONLINE }] });
    emit("friends-status", "signed-out");
    expect(useFriendsStore.getState().friends).toEqual([]);
    expect(useFriendsStore.getState().me).toBeNull();
  });

  it("começar a transmitir avisa os amigos", () => {
    emit("friends-status", "online");
    act(() => useStreamStore.setState({ status: "live" }));
    expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "status", room: null, live: true } });
  });

  it("entrar com o Discord avisa os amigos do nome e da foto", () => {
    emit("friends-status", "online");
    act(() =>
      useAccountStore.setState({ profile: { id: ME.id, username: "kaique", globalName: "K.A", avatar: "abc" } }),
    );
    expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "profile", name: "K.A", avatar: "abc" } });
  });

  it("desconectado do registro, não manda nada", () => {
    emit("friends-status", "connecting");
    invoke.mockClear();
    act(() => useStreamStore.setState({ status: "live" }));
    expect(invoke).not.toHaveBeenCalledWith("friends_action", expect.anything());
  });

  it("sem o Rust responder, fica desconectado", async () => {
    invoke.mockRejectedValue(new Error("fora do app"));
    offs.forEach((off) => off());
    offs = [await listenToFriendsEvents()];
    expect(useFriendsStore.getState().status).toBe("signed-out");
  });
});

describe("avisos na tela", () => {
  it("recusar o pedido de alguém de fora responde à sala", async () => {
    render(<App />);
    emit("room-join-request", { id: "00000000000000aa", from: BETO });
    // O mesmo pedido chegando de novo não duplica o aviso.
    emit("room-join-request", { id: "00000000000000aa", from: BETO });
    expect(screen.getAllByRole("alertdialog")).toHaveLength(1);
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Recusar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("room_answer_join", { id: "00000000000000aa", accept: false }),
    );
  });

  it("se a resposta ao pedido falhar, explica", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "room_answer_join" ? Promise.reject("A sala fechou.") : Promise.resolve(undefined),
    );
    await useFriendsStore.getState().answerJoin("x", true);
    expect(lastToast()).toMatchObject({ kind: "error", message: "A sala fechou." });
  });

  it("'Agora não' dispensa o convite, que mostra o código quando a sala não tem nome", () => {
    render(<App />);
    emit("friends-event", { type: "invited", from: ANA, code: "B8RJ2Q", room: null });
    const card = screen.getByRole("alertdialog", { name: "Ana te chamou para a sala" });
    expect(within(card).getByText("Sala B8RJ2Q")).toBeInTheDocument();
    fireEvent.click(within(card).getByRole("button", { name: "Agora não" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("entrar pelo convite usa o apelido salvo quando não há Discord", async () => {
    localStorage.setItem("squadview.nome", "Kaká");
    useFriendsStore.setState({ open: true });
    await useFriendsStore.getState().acceptInvite({ from: ANA, code: "B8RJ2Q", room: null });
    expect(invoke).toHaveBeenCalledWith("room_join", expect.objectContaining({ code: "B8RJ2Q", name: "Kaká" }));
    expect(useFriendsStore.getState().open).toBe(false);
  });
});

describe("amigo que já está na nossa sala", () => {
  const ANA_IN_ROOM = { ...ANA, direct: false, presence: { ...ONLINE, inRoom: true, room: "hwemnaso" } };
  const inRoom = () =>
    useRoomStore.setState({
      code: "B8RJ2Q",
      members: [
        { id: "p1", name: "Kaique", live: false, discord: { id: ME.id, avatar: null } },
        { id: "p2", name: "Ana", live: false, discord: { id: ANA.id, avatar: null } },
      ],
    });

  it("mostra 'Na sua sala' no lugar de Entrar, e Chamar para a sala fica desativado", () => {
    inRoom();
    const page = openFriends();
    state({ friends: [ANA_IN_ROOM] });
    const linha = within(page).getByText("Na sua sala").closest("li")!;
    expect(within(linha).queryByRole("button", { name: "Entrar" })).toBeNull();
    const chamar = within(linha).getByRole("button", { name: "Já está na sua sala" });
    expect(chamar).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(chamar);
    expect(invoke).not.toHaveBeenCalledWith("friends_action", { action: { type: "invite", id: ANA.id } });
  });

  it("pedido aprovado para a sala em que já estamos não entra de novo (duplicava a gente)", async () => {
    inRoom();
    emit("friends-event", { type: "join-result", result: { kind: "approved", code: "B8RJ2Q", room: null, by: "Ana" } });
    await waitFor(() => expect(lastToast()?.title).toBe("Entrando na sala"));
    expect(invoke).not.toHaveBeenCalledWith("room_join", expect.anything());
  });

  it("o pedido de entrada some quando a pessoa já entrou (outro membro aceitou)", () => {
    act(() => useRoomStore.setState({ code: "B8RJ2Q", members: [{ id: "p1", name: "Kaique", live: false }] }));
    emit("room-join-request", { id: "r1", from: ANA });
    expect(useFriendsStore.getState().joinRequests).toHaveLength(1);
    act(() => inRoom());
    expect(useFriendsStore.getState().joinRequests).toEqual([]);
  });
});
