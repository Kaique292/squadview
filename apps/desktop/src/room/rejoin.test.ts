import { useAccountStore } from "../account/account-store";
import { useToastStore } from "../ui/toast-store";
import { rejoinAfterUpdate, rememberRoomToRejoin, useRoomStore } from "./room-store";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "room_join" ? { code: "B8RJ2Q" } : undefined));
  localStorage.clear();
  useToastStore.setState({ toasts: [] });
  useAccountStore.setState({ profile: null });
});

describe("volta para a sala depois de atualizar", () => {
  it("entra de novo na sala de antes, com o nome de sempre", async () => {
    useAccountStore.setState({
      profile: { id: "123456789012345678", username: "kaique", globalName: "K.A", avatar: null },
    });
    useRoomStore.setState({ code: "B8RJ2Q" });
    rememberRoomToRejoin();
    useRoomStore.setState({ code: null });
    await rejoinAfterUpdate();
    expect(invoke).toHaveBeenCalledWith("room_join", expect.objectContaining({ code: "B8RJ2Q", name: "K.A" }));
    expect(useToastStore.getState().toasts[0]?.message).toContain("voltou para a sala");
  });

  it("sem sala guardada (ou já numa sala), não faz nada", async () => {
    await rejoinAfterUpdate();
    useRoomStore.setState({ code: "B8RJ2Q" });
    rememberRoomToRejoin();
    await rejoinAfterUpdate();
    expect(invoke).not.toHaveBeenCalledWith("room_join", expect.anything());
    useRoomStore.setState({ code: null });
  });
});
