import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { listenToRoomEvents, useRoomStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { listenToFriendsEvents, presenceText, useFriendsStore } from "./friends-store";

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

const ANA = { id: "111111111111111111", name: "Ana", avatar: null };
const BETO = { id: "222222222222222222", name: "Beto", avatar: null };
const ONLINE_IN_ROOM = { online: true, inRoom: true, room: "Galera", live: false };
const OFFLINE = { online: false, inRoom: false, room: null, live: false };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function state(partial: Record<string, unknown> = {}) {
  emit("friends-event", {
    type: "state",
    me: { id: "999999999999999999", name: "Kaique", avatar: null },
    friends: [],
    incoming: [],
    outgoing: [],
    recents: [],
    ...partial,
  });
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
  useAccountStore.setState({
    profile: { id: "999999999999999999", username: "kaique", globalName: "Kaique", avatar: null },
  });
  useRoomStore.setState({ code: null, roomName: null, members: [], watching: null });
  useFriendsStore.setState({
    status: "signed-out",
    open: false,
    friends: [],
    incoming: [],
    outgoing: [],
    recents: [],
    joinRequests: [],
    invites: [],
    waitingFor: null,
  });
  offs.forEach((off) => off());
  offs = [await listenToRoomEvents(), await listenToFriendsEvents()];
});

function openFriends() {
  render(<App />);
  fireEvent.click(within(screen.getByRole("navigation", { name: "Salas" })).getByRole("button", { name: "Amigos" }));
  return screen.getByRole("region", { name: "Amigos" });
}

describe("amigos", () => {
  it("ao conectar, conta ao registro quem somos e onde estamos", async () => {
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", {
        action: { type: "profile", name: "Kaique", avatar: null },
      }),
    );
    expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "status", room: null, live: false } });
    // Entrou numa sala: os amigos ficam sabendo (o código só vai para o servidor).
    act(() => useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera" }));
    expect(invoke).toHaveBeenCalledWith("friends_action", {
      action: { type: "status", room: { code: "B8RJ2Q", name: "Galera" }, live: false },
    });
  });

  it("o logo abre a página de amigos com quem está online", () => {
    const page = openFriends();
    state({
      friends: [
        { ...ANA, direct: false, presence: ONLINE_IN_ROOM },
        { ...BETO, direct: false, presence: OFFLINE },
      ],
    });
    expect(within(page).getByText("Ana")).toBeInTheDocument();
    expect(within(page).getByText("Na sala Galera")).toBeInTheDocument();
    expect(within(page).queryByText("Beto")).not.toBeInTheDocument();
    fireEvent.click(within(page).getByRole("button", { name: "Todos" }));
    expect(within(page).getByText("Beto")).toBeInTheDocument();
  });

  it("pede para entrar na sala de um amigo e entra quando aceitam", async () => {
    const page = openFriends();
    state({ friends: [{ ...ANA, direct: false, presence: ONLINE_IN_ROOM }] });
    fireEvent.click(within(page).getByRole("button", { name: "Entrar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "join", id: ANA.id } }),
    );
    emit("friends-event", { type: "join-result", result: { kind: "pending", friend: "Ana" } });
    expect(within(page).getByRole("button", { name: "Pedido enviado" })).toBeDisabled();
    emit("friends-event", {
      type: "join-result",
      result: { kind: "approved", code: "B8RJ2Q", room: "Galera", by: "Caio" },
    });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("room_join", expect.objectContaining({ code: "B8RJ2Q", name: "Kaique" })),
    );
  });

  it("pedido de alguém de fora aparece com os botões, e aceitar responde à sala", async () => {
    render(<App />);
    emit("room-join-request", { id: "00000000000000aa", from: BETO });
    const card = screen.getByRole("alertdialog", { name: "Beto quer entrar na sala" });
    fireEvent.click(within(card).getByRole("button", { name: "Deixar entrar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("room_answer_join", { id: "00000000000000aa", accept: true }),
    );
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("o pedido some quando outra pessoa da sala responde", () => {
    render(<App />);
    emit("room-join-request", { id: "00000000000000aa", from: BETO });
    emit("room-join-request-closed", "00000000000000aa");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("convite de um amigo leva direto para a sala", async () => {
    render(<App />);
    emit("friends-event", { type: "invited", from: ANA, code: "B8RJ2Q", room: "Galera" });
    const card = screen.getByRole("alertdialog", { name: "Ana te chamou para a sala" });
    fireEvent.click(within(card).getByRole("button", { name: "Entrar" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_join", expect.objectContaining({ code: "B8RJ2Q" })));
  });

  it("adiciona quem esteve numa sala junto e aceita pedidos", async () => {
    const page = openFriends();
    state({ recents: [BETO], incoming: [ANA] });
    fireEvent.click(within(page).getByRole("button", { name: "Recentes" }));
    fireEvent.click(within(page).getByRole("button", { name: /Adicionar amigo/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "friend-request", id: BETO.id } }),
    );
    fireEvent.click(within(page).getByRole("button", { name: /Pendentes/ }));
    fireEvent.click(within(page).getByRole("button", { name: "Aceitar" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("friends_action", { action: { type: "friend-accept", id: ANA.id } }),
    );
  });

  it("sem sessão, pede para entrar com o Discord de novo", () => {
    const page = openFriends();
    emit("friends-status", "signed-out");
    expect(within(page).getByText("Ative os amigos")).toBeInTheDocument();
    expect(within(page).getByRole("button", { name: /Entrar com o Discord/ })).toBeInTheDocument();
  });

  it("descreve a presença como o Discord", () => {
    expect(presenceText(OFFLINE)).toBe("Offline");
    expect(presenceText({ online: true, inRoom: false, room: null, live: false })).toBe("Online");
    expect(presenceText({ online: true, inRoom: true, room: null, live: false })).toBe("Numa sala");
    expect(presenceText({ online: true, inRoom: true, room: "Galera", live: true })).toBe("Transmitindo em Galera");
  });
});
