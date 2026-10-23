import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { type Member, useRoomStore, type Watch } from "../room/room-store";
import { ArcadeSection, keyLabel } from "../settings/ArcadeSection";
import { StreamMenuItems } from "../stream/StreamMenu";
import { useStreamStore } from "../stream/stream-store";
import { WatchTile } from "../stream/WatchTile";
import { ArcadeBadge } from "../ui/LiveBadge";
import { useToastStore } from "../ui/toast-store";
import { ArcadeHostDialog } from "./ArcadeHostDialog";
import { freeSlots, listenToArcadeEvents, useArcadeStore } from "./arcade-store";

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
  arcade: { game: "Bomberman", slots: 3, players: [] },
};

const INSTALLED = { status: "installed", detail: null, version: null, removable: true };

const toasts = () => useToastStore.getState().toasts.map((t) => t.message);

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

function respond(answers: Record<string, unknown>, failing: Record<string, unknown> = {}) {
  invoke.mockImplementation((cmd: string) =>
    cmd in failing ? Promise.reject(failing[cmd]) : Promise.resolve(answers[cmd]),
  );
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useToastStore.setState({ toasts: [] });
  useArcadeStore.setState({ hostDialog: false, seats: [], playing: null });
  useRoomStore.setState({ code: "B8RJ2Q", members: [ME, ANA], watching: {}, pendingWatch: [] });
  await listenToArcadeEvents();
});

describe("store do fliperama", () => {
  it("mostra o erro do Rust ao abrir, ao mudar o controle total e ao entrar", async () => {
    respond({}, { arcade_open: "Comece a transmitir o jogo antes de abrir o fliperama." });
    expect(await useArcadeStore.getState().open(2)).toBe(false);
    expect(toasts()[0]).toBe("Comece a transmitir o jogo antes de abrir o fliperama.");

    respond({}, { arcade_open: new Error("x"), arcade_grant_kbm: new Error("x"), arcade_join: new Error("x") });
    expect(await useArcadeStore.getState().open(2)).toBe(false);
    expect(toasts()[0]).toBe("Não deu para abrir o fliperama.");
    await useArcadeStore.getState().grantKbm("cccccccccccccccc");
    expect(toasts()[0]).toBe("Não deu para mudar o controle total.");
    await useArcadeStore.getState().join(ANA);
    expect(toasts()[0]).toBe("Não deu para entrar no fliperama.");
    expect(useArcadeStore.getState().playing).toBeNull();

    respond({}, { arcade_grant_kbm: "O controle total precisa da transmissão de uma janela." });
    await useArcadeStore.getState().grantKbm("cccccccccccccccc");
    expect(toasts()[0]).toMatch(/precisa da transmissão de uma janela/);
    respond({}, { arcade_join: "Entre numa sala primeiro." });
    await useArcadeStore.getState().join(ANA);
    expect(toasts()[0]).toBe("Entre numa sala primeiro.");
  });

  it("abrir dá certo com aviso; fechar limpa os jogadores; sair sem jogar não faz nada", async () => {
    expect(await useArcadeStore.getState().open(3, "Bomberman")).toBe(true);
    expect(invoke).toHaveBeenCalledWith("arcade_open", { slots: 3, game: "Bomberman" });
    useArcadeStore.setState({ seats: [{ memberId: "x", slot: 2, connected: true, kbm: false }] });
    await useArcadeStore.getState().close();
    expect(useArcadeStore.getState().seats).toEqual([]);
    invoke.mockClear();
    await useArcadeStore.getState().leave();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("entrar não pede a live de novo se já assiste ou já pediu", async () => {
    useRoomStore.setState({ pendingWatch: [ANA.id] });
    await useArcadeStore.getState().join(ANA);
    expect(invoke).not.toHaveBeenCalledWith("room_watch", expect.anything());
  });

  it("avisos de outro fliperama são ignorados, e desligar o controle total avisa", () => {
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: 2, kbm: true } });
    emit("arcade-seat", { hostId: "outro", slot: 4 });
    emit("arcade-kbm", { hostId: "outro", granted: false });
    emit("arcade-left", { hostId: "outro", reason: "" });
    expect(useArcadeStore.getState().playing).toEqual({ hostId: ANA.id, slot: 2, kbm: true });
    expect(toasts()).toEqual([]);
    emit("arcade-kbm", { hostId: ANA.id, granted: false });
    expect(toasts()[0]).toMatch(/controle total foi desligado/);
    emit("arcade-left", { hostId: ANA.id, reason: "" });
    expect(useArcadeStore.getState().playing).toBeNull();
    expect(toasts()).toHaveLength(1);
  });

  it("jogador desconhecido aparece com o nome genérico, e quem já estava não avisa de novo", () => {
    emit("arcade-players", [{ memberId: "zzzzzzzzzzzzzzzz", slot: 2, connected: false, kbm: false }]);
    expect(toasts()[0]).toBe("Quem hospeda entrou no fliperama como P2.");
    emit("arcade-players", [{ memberId: "zzzzzzzzzzzzzzzz", slot: 2, connected: true, kbm: false }]);
    expect(toasts()).toHaveLength(1);
  });

  it("vagas livres só com o fliperama aberto", () => {
    expect(freeSlots(ME)).toBe(0);
    expect(freeSlots(ANA)).toBe(3);
    expect(freeSlots({ ...ANA, arcade: { game: null, slots: 1, players: ["a", "b"] } })).toBe(0);
  });
});

describe("painel do fliperama: os outros estados", () => {
  it("conferindo, driver com problema e driver incompatível", async () => {
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd: string) =>
      cmd === "arcade_driver" ? new Promise((r) => (answer = r)) : Promise.resolve(),
    );
    useArcadeStore.setState({ hostDialog: true });
    const { unmount } = render(<ArcadeHostDialog />);
    expect(screen.getByText(/Conferindo o driver/)).toBeInTheDocument();
    await act(async () => answer({ status: "incompatible", detail: "versão", version: null, removable: false }));
    expect(screen.getByText(/numa versão que o SquadView não reconhece/)).toBeInTheDocument();
    unmount();

    respond({}, { arcade_driver: "falhou" });
    render(<ArcadeHostDialog />);
    expect(await screen.findByRole("button", { name: "Baixar o instalador oficial" })).toBeInTheDocument();
  });

  it("aberto sem ninguém, cheio e com quem ainda conecta; o rodapé fecha", () => {
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: "Bomberman", slots: 1, players: [] } }] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    expect(screen.getByText("Ninguém entrou ainda.")).toBeInTheDocument();
    expect(screen.getByText(/Bomberman · 1 vaga livre/)).toBeInTheDocument();
    emit("arcade-players", [{ memberId: "zzzzzzzzzzzzzzzz", slot: 2, connected: false, kbm: false }]);
    expect(screen.getByText(/Vagas cheias/)).toBeInTheDocument();
    expect(screen.getByText("Conectando…")).toBeInTheDocument();
    expect(screen.getByText("Alguém")).toBeInTheDocument();
    // O "Fechar" do rodapé (o outro é o X do canto).
    fireEvent.click(screen.getAllByRole("button", { name: "Fechar" }).find((b) => b.textContent === "Fechar")!);
    expect(useArcadeStore.getState().hostDialog).toBe(false);
  });

  it("enquanto abre, o botão espera", async () => {
    let answer: () => void = () => {};
    invoke.mockImplementation((cmd: string) =>
      cmd === "arcade_driver"
        ? Promise.resolve(INSTALLED)
        : cmd === "arcade_open"
          ? new Promise<void>((r) => (answer = r))
          : Promise.resolve(),
    );
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    const button = await screen.findByRole("button", { name: "Abrir fliperama" });
    fireEvent.click(button);
    await waitFor(() => expect(button).toBeDisabled());
    await act(async () => answer());
    await waitFor(() => expect(button).toBeEnabled());
  });
});

describe("jogando: o mouse preso no jogo", () => {
  let locked: Element | null = null;

  beforeEach(() => {
    locked = null;
    Object.defineProperty(document, "pointerLockElement", { configurable: true, get: () => locked });
    const lock = (element: Element) => {
      locked = element;
      document.dispatchEvent(new Event("pointerlockchange"));
      return Promise.resolve();
    };
    HTMLElement.prototype.requestPointerLock = vi.fn(function (this: HTMLElement) {
      return lock(this);
    });
    document.exitPointerLock = vi.fn(() => {
      locked = null;
      document.dispatchEvent(new Event("pointerlockchange"));
    });
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      cb(0);
      return 1;
    });
  });

  function renderTile(kbm = false, variant: "focus" | "grid" = "focus") {
    const watch: Watch = { member: ANA, stats: null, history: [], listening: true, volume: 100 };
    useRoomStore.setState({ watching: { [ANA.id]: watch } });
    useArcadeStore.setState({ playing: { hostId: ANA.id, slot: 3, kbm } });
    const view = render(
      <WatchTile memberId={ANA.id} variant={variant} focused onActivate={() => {}} onDetails={() => {}} />,
    );
    return { area: screen.getByRole("application", { name: "Jogando como P3" }), ...view };
  }

  function mouse(area: HTMLElement, type: string, init: Record<string, number>) {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: init.button ?? 0 });
    for (const [k, v] of Object.entries(init)) Object.defineProperty(event, k, { value: v });
    act(() => {
      area.dispatchEvent(event);
    });
  }

  it("clicar prende o ponteiro: movimento, botões e soltar", () => {
    const { area } = renderTile();
    expect(screen.getByText(/Clique no jogo para jogar/)).toBeInTheDocument();
    // Sem o ponteiro preso, nada vai.
    mouse(area, "mousemove", { movementX: 4, movementY: 1 });
    expect(invoke).not.toHaveBeenCalledWith("arcade_mouse", expect.anything());

    fireEvent.click(area);
    expect(area.requestPointerLock).toHaveBeenCalled();
    expect(screen.getByText(/Teclado e mouse viram controle/)).toBeInTheDocument();
    // O movimento cru (`pointerrawupdate`) vai na hora, sem esperar o quadro da página.
    mouse(area, "pointerrawupdate", { movementX: 6, movementY: -2 });
    expect(invoke).toHaveBeenCalledWith("arcade_mouse", { dx: 6, dy: -2 });
    // O mousemove não manda de novo o mesmo movimento.
    mouse(area, "mousemove", { movementX: 50, movementY: 50 });
    expect(invoke).not.toHaveBeenCalledWith("arcade_mouse", { dx: 50, dy: 50 });
    mouse(area, "mousedown", { button: 0 });
    mouse(area, "mouseup", { button: 2 });
    mouse(area, "mousedown", { button: 7 });
    expect(invoke).toHaveBeenCalledWith("arcade_mouse_button", { button: "left", down: true });
    expect(invoke).toHaveBeenCalledWith("arcade_mouse_button", { button: "right", down: false });
    fireEvent.contextMenu(area);
    fireEvent.doubleClick(area);
    // Rolagem só vai com o controle total.
    fireEvent.wheel(area, { deltaY: 100 });
    expect(invoke).not.toHaveBeenCalledWith("arcade_wheel", expect.anything());

    act(() => document.exitPointerLock());
    expect(invoke).toHaveBeenCalledWith("arcade_release");
    expect(screen.getByText(/Clique no jogo para jogar/)).toBeInTheDocument();
  });

  it("enquanto uma chamada não volta, o movimento se junta e vai logo depois", async () => {
    let answer: () => void = () => {};
    invoke.mockImplementation((cmd: string) =>
      cmd === "arcade_mouse" ? new Promise<void>((r) => (answer = r)) : Promise.resolve(),
    );
    const { area } = renderTile();
    fireEvent.click(area);
    mouse(area, "pointerrawupdate", { movementX: 1, movementY: 0 });
    mouse(area, "pointerrawupdate", { movementX: 2, movementY: 3 });
    mouse(area, "pointerrawupdate", { movementX: 4, movementY: -1 });
    expect(invoke.mock.calls.filter(([c]) => c === "arcade_mouse")).toEqual([["arcade_mouse", { dx: 1, dy: 0 }]]);
    await act(async () => answer());
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("arcade_mouse", { dx: 6, dy: 2 }));
  });

  it("com o controle total, a rolagem vai e o selo avisa", () => {
    const { area } = renderTile(true);
    fireEvent.click(area);
    expect(screen.getByText(/vão direto para o jogo/)).toBeInTheDocument();
    expect(screen.getByLabelText("Controle total")).toBeInTheDocument();
    fireEvent.wheel(area, { deltaY: -50 });
    fireEvent.wheel(area, { deltaY: 50 });
    fireEvent.wheel(area, { deltaY: 0 });
    expect(invoke).toHaveBeenCalledWith("arcade_wheel", { delta: 120 });
    expect(invoke).toHaveBeenCalledWith("arcade_wheel", { delta: -120 });
  });

  it("no quadro pequeno, só o selo; ao sair, o ponteiro é solto", () => {
    const { area, unmount } = renderTile(false, "grid");
    expect(screen.queryByText(/Clique no jogo/)).not.toBeInTheDocument();
    fireEvent.click(area);
    unmount();
    expect(document.exitPointerLock).toHaveBeenCalled();
  });
});

describe("Configurações › Fliperama: os outros caminhos", () => {
  it("nomes das teclas para a pessoa ler", () => {
    expect(keyLabel("KeyQ")).toBe("Q");
    expect(keyLabel("Digit5")).toBe("5");
    expect(keyLabel("Numpad8")).toBe("Num 8");
    expect(keyLabel("ArrowLeft")).toBe("←");
    expect(keyLabel("ShiftLeft")).toBe("Shift esq.");
    expect(keyLabel("F5")).toBe("F5");
  });

  it("sem driver, confere de novo; driver sem desinstalador não oferece remover", async () => {
    respond(
      { arcade_driver: { status: "missing", detail: null, version: null, removable: false } },
      { arcade_keymap: "x" },
    );
    render(<ArcadeSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Já instalei: conferir de novo" }));
    await waitFor(() => expect(invoke.mock.calls.filter(([c]) => c === "arcade_driver")).toHaveLength(2));
    // Sem o mapeamento salvo, a lista aparece vazia.
    expect(within(screen.getByRole("list", { name: "Teclas do controle" })).getAllByText("—").length).toBeGreaterThan(
      10,
    );

    respond({ arcade_driver: { ...INSTALLED, removable: false }, arcade_keymap: { keys: {} } });
    render(<ArcadeSection />);
    expect(await screen.findAllByText(/ViGEmBus instalado/)).not.toHaveLength(0);
  });

  it("driver que não responde vira o aviso de instalar", async () => {
    respond({ arcade_keymap: { keys: {} } }, { arcade_driver: "x" });
    render(<ArcadeSection />);
    expect(await screen.findByRole("button", { name: "Baixar o instalador oficial" })).toBeInTheDocument();
  });

  it("remover que falha avisa; driver que continua instalado pede para reiniciar", async () => {
    respond(
      { arcade_driver: INSTALLED, arcade_keymap: { keys: {} } },
      { arcade_uninstall_driver: "A remoção foi cancelada." },
    );
    const { unmount } = render(<ArcadeSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remover o driver de controle" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remover" }));
    await waitFor(() => expect(toasts()[0]).toBe("A remoção foi cancelada."));
    unmount();

    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: {} } }, { arcade_uninstall_driver: new Error("x") });
    const second = render(<ArcadeSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remover o driver de controle" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remover" }));
    await waitFor(() => expect(toasts()[0]).toBe("Não deu para remover o driver."));
    second.unmount();

    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: {} }, arcade_uninstall_driver: INSTALLED });
    render(<ArcadeSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remover o driver de controle" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remover" }));
    await waitFor(() => expect(toasts()[0]).toMatch(/reiniciar o PC/));
  });

  it("Esc cancela a troca, clicar de novo desiste, e voltar ao padrão salva (ou avisa a falha)", async () => {
    respond({ arcade_driver: INSTALLED, arcade_keymap: { keys: { KeyJ: "a" } }, arcade_reset_keymap: { keys: {} } });
    render(<ArcadeSection />);
    const button = await screen.findByRole("button", { name: "Trocar a tecla de A" });
    fireEvent.click(button);
    fireEvent.keyDown(window, { code: "Escape" });
    expect(button).toHaveTextContent("J");
    expect(invoke).not.toHaveBeenCalledWith("arcade_set_keymap", expect.anything());
    fireEvent.click(button);
    fireEvent.click(button);
    expect(button).toHaveTextContent("J");

    fireEvent.click(screen.getByRole("button", { name: "Voltar ao padrão" }));
    await waitFor(() => expect(button).toHaveTextContent("—"));
    respond({ arcade_driver: INSTALLED }, { arcade_reset_keymap: "x", arcade_set_keymap: "x" });
    fireEvent.click(screen.getByRole("button", { name: "Voltar ao padrão" }));
    await waitFor(() => expect(toasts()[0]).toBe("Não deu para voltar ao padrão."));
    fireEvent.click(button);
    fireEvent.keyDown(window, { code: "KeyP" });
    await waitFor(() => expect(toasts()[0]).toBe("Não deu para salvar."));
  });
});

describe("menu da nossa live", () => {
  it("o item Fliperama abre o painel", () => {
    render(
      <DropdownMenu.Root open>
        <DropdownMenu.Trigger>menu</DropdownMenu.Trigger>
        <DropdownMenu.Content>
          <StreamMenuItems kind="dropdown" onChange={() => {}} onDetails={() => {}} />
        </DropdownMenu.Content>
      </DropdownMenu.Root>,
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Fliperama" }));
    expect(useArcadeStore.getState().hostDialog).toBe(true);
  });
});

describe("selo do fliperama", () => {
  it("mostra que o fliperama está aberto e quantos jogam, para todos", () => {
    const { rerender } = render(<ArcadeBadge arcade={{ slots: 3, players: ["a"] }} />);
    expect(screen.getByRole("img", { name: "Fliperama aberto: 1 de 3 vagas ocupadas" })).toHaveTextContent(
      "Fliperama 1/3",
    );
    rerender(<ArcadeBadge arcade={{ slots: 1, players: [] }} compact size="sm" />);
    expect(screen.getByRole("img", { name: "Fliperama aberto: 0 de 1 vaga ocupada" })).toHaveTextContent("0/1");
    // Na lista da sala, só o controle (a conta fica na dica).
    rerender(<ArcadeBadge arcade={{ slots: 2, players: ["a"] }} iconOnly size="sm" />);
    const icon = screen.getByRole("img", { name: "Fliperama aberto: 1 de 2 vagas ocupadas" });
    expect(icon).toHaveTextContent("");
    expect(icon).toHaveAttribute("title", "Fliperama: 1/2");
  });

  it("aparece na live de quem hospeda", () => {
    const watch: Watch = { member: ANA, stats: null, history: [], listening: true, volume: 100 };
    useRoomStore.setState({ watching: { [ANA.id]: watch } });
    render(<WatchTile memberId={ANA.id} variant="grid" focused={false} onActivate={() => {}} onDetails={() => {}} />);
    expect(screen.getByRole("img", { name: /Fliperama aberto/ })).toHaveTextContent("Fliperama 0/3");
  });
});

describe("fluidez do fliperama", () => {
  it("abaixo de 60 fps, o painel avisa e troca a qualidade com um clique", async () => {
    const source = { Window: 7 } as never;
    useStreamStore.setState({
      quality: { resolution: "1080", fps: 30 },
      current: { source, audioPid: null, lastAudioPid: null },
    });
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 2, players: [] } }] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    expect(screen.getByText(/Sua live está a 30 fps/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Mudar para 60 fps" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_preview", {
        source,
        quality: { resolution: "1080", fps: 60 },
        audioPid: null,
      }),
    );
  });

  it("a 60 fps, nada de aviso", () => {
    useStreamStore.setState({ quality: { resolution: "1080", fps: 60 } });
    useRoomStore.setState({ members: [{ ...ME, arcade: { game: null, slots: 2, players: [] } }] });
    useArcadeStore.setState({ hostDialog: true });
    render(<ArcadeHostDialog />);
    expect(screen.queryByText(/Sua live está a/)).not.toBeInTheDocument();
  });
});
