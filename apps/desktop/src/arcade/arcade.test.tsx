import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemberMenu } from "../room/MemberMenu";
import { type Member, useRoomStore, type Watch } from "../room/room-store";
import { ArcadeSection } from "../settings/ArcadeSection";
import { WatchTile } from "../stream/WatchTile";
import { useToastStore } from "../ui/toast-store";
import { ArcadeHostDialog } from "./ArcadeHostDialog";
import { listenToArcadeEvents, useArcadeStore } from "./arcade-store";

const { invoke, handlers } = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return () => handlers.delete(name);
  }),
}));

const ME: Member = { id: "aaaaaaaaaaaaaaaa", name: "Kaique", live: true };
const ANA: Member = {
  id: "bbbbbbbbbbbbbbbb",
  name: "Ana",
  live: true,
  arcade: { game: "Bomberman", slots: 3, players: ["cccccccccccccccc"] },
};
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false };

const INSTALLED = { status: "installed", detail: null, version: "1.22.0", removable: true };
const MISSING = { status: "missing", detail: null, version: null, removable: false };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function respond(answers: Record<string, unknown>) {
  invoke.mockImplementation((cmd: string) => Promise.resolve(answers[cmd]));
}

const toasts = () => useToastStore.getState().toasts.map((t) => t.message);

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useToastStore.setState({ toasts: [] });
  useArcadeStore.setState({ hostDialog: false, seats: [], playing: null, controllers: null, gameFocused: null });
  useRoomStore.setState({ code: "B8RJ2Q", members: [ME, ANA, BETO], watching: {}, pendingWatch: [] });
  await listenToArcadeEvents();
});

describe("painel do fliperama (host)", () => {
  it("sem o driver, explica e leva ao instalador oficial", async () => {
    respond({ arcade_driver: MISSING });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    fireEvent.click(await screen.findByRole("button", { name: "Baixar o instalador oficial" }));
    expect(invoke).toHaveBeenCalledWith("arcade_install_driver");
    expect(screen.getByText(/quem joga não precisa de nada/)).toBeInTheDocument();
  });

  it("com o driver e ao vivo, abre com as vagas escolhidas", async () => {
    respond({ arcade_driver: INSTALLED });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    fireEvent.click(await screen.findByRole("radio", { name: "2 jogadores" }));
    fireEvent.click(screen.getByRole("button", { name: "Abrir fliperama" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_open", { slots: 2, game: null, reserveP1: true }));
  });

  it("sem transmitir, pede para começar a live antes", async () => {
    respond({ arcade_driver: INSTALLED });
    useRoomStore.setState({ members: [{ ...ME, live: false }, ANA] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    expect(await screen.findByRole("alert")).toHaveTextContent(/Comece a transmitir/);
    expect(screen.getByRole("button", { name: "Abrir fliperama" })).toBeDisabled();
  });

  it("aberto: mostra quem joga, tira alguém e liga o controle total com o aviso do corte", () => {
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 2, players: [BETO.id] } }, BETO] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    emit("arcade-players", [{ memberId: BETO.id, slot: 2, connected: true, kbm: false }]);
    expect(toasts()).toContain("Beto entrou no fliperama como P2.");
    const list = screen.getByRole("list", { name: "Jogadores do fliperama" });
    expect(within(list).getByText("P2")).toBeInTheDocument();
    expect(screen.getByText(/1 vaga livre/)).toBeInTheDocument();

    fireEvent.click(within(list).getByRole("switch", { name: "Controle total (teclado e mouse)" }));
    expect(invoke).toHaveBeenCalledWith("arcade_grant_kbm", { memberId: BETO.id });
    emit("arcade-players", [{ memberId: BETO.id, slot: 2, connected: true, kbm: true }]);
    expect(screen.getByText(/Ctrl\+Alt\+F12/)).toBeInTheDocument();
    fireEvent.click(within(list).getByRole("switch", { name: "Controle total (teclado e mouse)" }));
    expect(invoke).toHaveBeenCalledWith("arcade_grant_kbm", { memberId: null });

    fireEvent.click(within(list).getByRole("button", { name: "Tirar Beto do fliperama" }));
    expect(invoke).toHaveBeenCalledWith("arcade_kick", { memberId: BETO.id });
    fireEvent.click(screen.getByRole("button", { name: "Fechar fliperama" }));
    expect(invoke).toHaveBeenCalledWith("arcade_close");
  });

  it("dá para abrir sem guardar o P1", async () => {
    respond({ arcade_driver: INSTALLED });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    const reserve = await screen.findByRole("switch", { name: "Guardar o P1 para mim" });
    expect(reserve).toBeChecked();
    fireEvent.click(reserve);
    fireEvent.click(screen.getByRole("button", { name: "Abrir fliperama" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_open", { slots: 3, game: null, reserveP1: false }));
  });

  // Regressão: com as 4 vagas do XInput ocupadas, o controle do amigo não aparecia no jogo e o app
  // mostrava "P2" como se estivesse tudo certo.
  it("avisa quando o Windows não deu vaga ao controle do jogador", () => {
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 2, players: [BETO.id] } }, BETO] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    emit("arcade-players", [{ memberId: BETO.id, slot: 2, connected: true, kbm: false, xinput: false }]);
    expect(screen.getByRole("alert")).toHaveTextContent(/não deu vaga a este controle/);
    expect(toasts().some((t) => /não deu vaga ao controle de Beto/.test(t))).toBe(true);
    emit("arcade-players", [{ memberId: BETO.id, slot: 2, connected: true, kbm: false, xinput: true }]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  // Regressão: muitos jogos ignoram os controles fora do primeiro plano, e o amigo achava que o
  // controle dele não funcionava.
  it("com alguém jogando, avisa (uma vez) quando o jogo sai do primeiro plano", async () => {
    respond({ arcade_driver: INSTALLED });
    await useArcadeStore.getState().open(1);
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 1, players: [BETO.id] } }, BETO] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    emit("arcade-focus", false);
    expect(screen.queryByRole("alert")).toBeNull();
    emit("arcade-players", [{ memberId: BETO.id, slot: 2, connected: true, kbm: false, xinput: true }]);
    emit("arcade-focus", true);
    emit("arcade-focus", false);
    expect(screen.getByRole("alert")).toHaveTextContent(/não está em primeiro plano/);
    emit("arcade-focus", true);
    emit("arcade-focus", false);
    expect(toasts().filter((t) => /saiu do primeiro plano/.test(t))).toHaveLength(1);
    emit("arcade-focus", true);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("entrar no fliperama de alguém", () => {
  function openMenu(member: Member) {
    render(
      <MemberMenu member={member} isMe={false}>
        <div>{member.name}</div>
      </MemberMenu>,
    );
    fireEvent.contextMenu(screen.getByText(member.name));
  }

  it("com vaga, entra como jogador e assiste a live de quem hospeda", async () => {
    openMenu(ANA);
    fireEvent.click(await screen.findByRole("menuitem", { name: /Entrar no fliperama \(2 vagas\)/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_join", { hostId: ANA.id }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
    expect(useArcadeStore.getState().playing).toEqual({ hostId: ANA.id, slot: null, kbm: false });
  });

  it("cheio não deixa entrar; jogando, oferece sair", async () => {
    openMenu({ ...ANA, arcade: { game: null, slots: 1, players: [BETO.id] } });
    expect(await screen.findByRole("menuitem", { name: /Fliperama cheio/ })).toHaveAttribute("aria-disabled", "true");
  });

  it("jogando nesse fliperama, o menu tem sair", async () => {
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: 2, kbm: false } });
    openMenu(ANA);
    fireEvent.click(await screen.findByRole("menuitem", { name: /Sair do fliperama/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_leave"));
    expect(useArcadeStore.getState().playing).toBeNull();
  });

  it("os avisos do host chegam: vaga, controle total e saída com o motivo", () => {
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: null, kbm: false } });
    emit("arcade-seat", { hostId: ANA.id, slot: 3 });
    expect(useArcadeStore.getState().playing?.slot).toBe(3);
    expect(toasts()[0]).toMatch(/Você é o P3 no fliperama de Ana/);
    emit("arcade-kbm", { hostId: ANA.id, granted: true });
    expect(useArcadeStore.getState().playing?.kbm).toBe(true);
    emit("arcade-left", { hostId: ANA.id, reason: "As vagas do fliperama estão cheias." });
    expect(useArcadeStore.getState().playing).toBeNull();
    expect(toasts()[0]).toBe("As vagas do fliperama estão cheias.");
  });
});

describe("jogando: o teclado e o mouse no quadro do jogo", () => {
  function watchOf(member: Member): Watch {
    return { member, stats: null, history: [], listening: true, volume: 100 };
  }

  function renderTile() {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA) } });
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: 2, kbm: false } });
    render(<WatchTile memberId={ANA.id} variant="focus" focused onActivate={() => {}} onDetails={() => {}} />);
    return screen.getByRole("application", { name: "Jogando como P2" });
  }

  it("mostra o P e manda as teclas como controle (sem repetir a tecla segurada)", () => {
    const area = renderTile();
    expect(screen.getByText("Você é o P2")).toBeInTheDocument();
    fireEvent.keyDown(area, { code: "KeyJ" });
    fireEvent.keyDown(area, { code: "KeyJ", repeat: true });
    fireEvent.keyUp(area, { code: "KeyJ" });
    const keys = invoke.mock.calls.filter(([cmd]) => cmd === "arcade_key");
    expect(keys).toEqual([
      ["arcade_key", { code: "KeyJ", down: true }],
      ["arcade_key", { code: "KeyJ", down: false }],
    ]);
  });

  // Regressão: o amigo não sabia que tecla virava qual botão (achou que só W, S e Enter funcionavam).
  it("mostra as teclas que viram o controle; com o controle total, não", async () => {
    respond({
      arcade_keymap: {
        keys: {
          KeyW: "left-stick-up",
          KeyA: "left-stick-left",
          KeyS: "left-stick-down",
          KeyD: "left-stick-right",
          KeyJ: "a",
        },
      },
    });
    renderTile();
    expect(await screen.findByText(/WASD anda · J = A/)).toBeInTheDocument();
    act(() => useArcadeStore.setState({ playing: { hostId: ANA.id, slot: 2, kbm: true } }));
    expect(screen.queryByText(/WASD anda/)).toBeNull();
  });

  it("Esc fica com o navegador e perder o foco solta tudo", () => {
    const area = renderTile();
    fireEvent.keyDown(area, { code: "Escape" });
    expect(invoke).not.toHaveBeenCalledWith("arcade_key", expect.anything());
    fireEvent.blur(area);
    expect(invoke).toHaveBeenCalledWith("arcade_release");
  });

  // Regressão: com um controle que o app não lia (DualSense), o amigo achava que ele "não
  // conectava": nada dizia se o controle tinha sido achado.
  it("diz se o controle foi achado, e qual", () => {
    const area = renderTile();
    expect(within(area).queryByText(/controle/i)).toBeNull();
    emit("arcade-controllers", []);
    expect(within(area).getByText("Nenhum controle encontrado: ligue um ou jogue com o teclado")).toBeInTheDocument();
    emit("arcade-controllers", [{ name: "DualSense Wireless Controller", kind: "playstation", supported: true }]);
    expect(within(area).getByText("Controle de PlayStation conectado")).toBeInTheDocument();
  });

  it("pelo repassador, avisa que o controle pode atrasar", () => {
    renderTile();
    expect(screen.queryByText(/pelo repassador/)).toBeNull();
    emit("arcade-seat", { hostId: ANA.id, slot: 2, relayed: true });
    expect(screen.getByText(/Conectado pelo repassador/)).toBeInTheDocument();
  });

  it("sem a vaga ainda, não captura nada", () => {
    useRoomStore.setState({ watching: { [ANA.id]: watchOf(ANA) } });
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: null, kbm: false } });
    render(<WatchTile memberId={ANA.id} variant="focus" focused onActivate={() => {}} onDetails={() => {}} />);
    expect(screen.queryByRole("application")).not.toBeInTheDocument();
  });
});

describe("Configurações › Fliperama", () => {
  it("remove o driver depois de confirmar", async () => {
    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: {} }, arcade_uninstall_driver: MISSING });
    render(<ArcadeSection />);
    expect(await screen.findByText("ViGEmBus instalado (1.22.0)")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remover o driver de controle" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remover" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_uninstall_driver"));
    expect(await screen.findByRole("button", { name: "Baixar o instalador oficial" })).toBeInTheDocument();
    expect(toasts()).toContain("Driver de controle removido.");
  });

  it("com o fliperama aberto, não deixa remover", async () => {
    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: {} } });
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 1, players: [] } }] });
    render(<ArcadeSection />);
    expect(await screen.findByRole("button", { name: "Remover o driver de controle" })).toBeDisabled();
  });

  it("troca a tecla de um botão e salva", async () => {
    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: { KeyJ: "a", Space: "a", KeyK: "b" } } });
    render(<ArcadeSection />);
    const button = await screen.findByRole("button", { name: "Trocar a tecla de A" });
    expect(button).toHaveTextContent("J / Espaço");
    fireEvent.click(button);
    expect(button).toHaveTextContent("Aperte…");
    fireEvent.keyDown(window, { code: "KeyZ" });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("arcade_set_keymap", { keymap: { keys: { KeyK: "b", KeyZ: "a" } } }),
    );
    expect(button).toHaveTextContent("Z");
  });
});
