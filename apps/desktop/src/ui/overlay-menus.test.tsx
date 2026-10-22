import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { App } from "../App";
import { MemberMenu } from "../room/MemberMenu";
import { listenToRoomEvents, type Member, useRoomStore } from "../room/room-store";
import { MicButton } from "../voice/VoiceButtons";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { holdOverlay, measureHoles, openOverlays, useOverlayMenu, watchVideoHoles } from "./overlay";
import { Select } from "./Select";

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

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "voice_devices" ? { inputs: [], outputs: [] } : undefined),
  );
  handlers.clear();
  localStorage.clear();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: {}, pendingWatch: [] });
  // Só dentro do Tauri o app avisa o Rust para esconder o vídeo.
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  // O jsdom não rola elementos; a lista do Radix chama isso ao abrir.
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  delete (Element.prototype as Partial<Element>).scrollIntoView;
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

/**
 * O menu abriu por cima do vídeo sem escondê-lo (a live continua): o vídeo ganhou um buraco onde
 * o menu está e, depois de fechar, voltou inteiro.
 */
async function expectHoleThenWhole(close: () => void) {
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("set_video_holes", {
      holes: [expect.objectContaining({ width: expect.any(Number) })],
    }),
  );
  expect(invoke).not.toHaveBeenCalledWith("set_video_hidden", { hidden: true });
  close();
  await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("set_video_holes", { holes: [] }));
}

let stopHoles = () => {};
beforeEach(() => {
  // O jsdom não mede nada: todo elemento ocupa um retângulo de 200 × 100 em (10, 20).
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    x: 10,
    y: 20,
    left: 10,
    top: 20,
    width: 200,
    height: 100,
    right: 210,
    bottom: 120,
    toJSON: () => ({}),
  });
  // Como quando há vídeo na tela (a área do vídeo liga a vigia).
  stopHoles = watchVideoHoles();
});
afterEach(() => {
  stopHoles();
  vi.restoreAllMocks();
});

function pressEscape() {
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
}

describe("menus por cima do vídeo", () => {
  it("o gancho só repassa a mudança: abrir um menu não esconde mais o vídeo", () => {
    const changes: boolean[] = [];
    let toggle: (open: boolean) => void = () => {};
    function Probe() {
      toggle = useOverlayMenu((open) => changes.push(open));
      return null;
    }
    render(<Probe />);
    act(() => toggle(true));
    act(() => toggle(false));
    expect(openOverlays()).toBe(0);
    expect(changes).toEqual([true, false]);
  });

  it("o buraco tem o tamanho e os cantos exatos do que está por cima, em pixels físicos", () => {
    // Uma folga em volta aparecia como moldura preta (embaixo do buraco não há vídeo).
    const root = document.createElement("div");
    root.innerHTML =
      '<div data-video-hole class="cartao"></div><div data-radix-popper-content-wrapper><div class="pilula"></div></div>';
    // O jsdom não entende `border-radius`: o raio de cada um vem daqui.
    const style = vi
      .spyOn(window, "getComputedStyle")
      .mockImplementation(
        (el) =>
          ({ borderTopLeftRadius: (el as Element).className === "pilula" ? "999px" : "8px" }) as CSSStyleDeclaration,
      );
    const dpr = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", { value: 1.5, configurable: true });
    try {
      // Um pixel para dentro de cada lado (o recorte do Windows tem os cantos em degraus).
      expect(measureHoles(root)).toEqual([
        { x: 16, y: 31, width: 298, height: 148, radius: 11 },
        // Pílula: o raio para na metade da altura.
        { x: 16, y: 31, width: 298, height: 148, radius: 74 },
      ]);
    } finally {
      Object.defineProperty(window, "devicePixelRatio", { value: dpr, configurable: true });
      style.mockRestore();
    }
  });

  it("soltar duas vezes não conta em dobro, e o modal não esconde o vídeo (ele fica por baixo)", () => {
    const release = holdOverlay();
    expect(openOverlays()).toBe(1);
    release();
    release();
    expect(openOverlays()).toBe(0);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("o menu do botão direito num membro abre por cima da live, sem escondê-la", async () => {
    render(
      <MemberMenu member={ANA} isMe={false}>
        <div>Ana</div>
      </MemberMenu>,
    );
    fireEvent.contextMenu(screen.getByText("Ana"));
    await screen.findByRole("menu", { name: "Opções de Ana" });
    await expectHoleThenWhole(pressEscape);
  });

  it("a setinha do microfone abre o menu por cima da live", async () => {
    render(<MicButton />);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Opções de entrada" }), { button: 0, ctrlKey: false });
    const menu = await screen.findByRole("menu");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_devices"));
    // As setas no volume mexem nele, não fecham nem navegam o menu.
    fireEvent.keyDown(screen.getByRole("slider", { name: "Volume de entrada" }), { key: "ArrowRight" });
    expect(menu).toBeInTheDocument();
    await expectHoleThenWhole(pressEscape);
  });

  it("a lista suspensa abre por cima da live", async () => {
    const onChange = vi.fn();
    render(
      <Select
        label="Qualidade"
        value="a"
        options={[
          { value: "a", label: "1080p" },
          { value: "b", label: "720p" },
        ]}
        onChange={onChange}
      />,
    );
    fireEvent.click(screen.getByRole("combobox", { name: "Qualidade" }));
    const option = await screen.findByRole("option", { name: "720p" });
    await expectHoleThenWhole(() => fireEvent.click(option));
    expect(onChange).toHaveBeenCalledWith("b");
  });

  it("o menu do nome da sala abre por cima da live", async () => {
    await listenToRoomEvents();
    render(<App />);
    act(() => handlers.get("room-members")!({ payload: [ME, ANA] }));
    // O cabeçalho da lateral é o botão "Galera" que abre um menu (a barra de salas tem outro).
    const header = screen.getAllByRole("button", { name: "Galera" }).find((b) => b.getAttribute("aria-haspopup"));
    fireEvent.pointerDown(header!, { button: 0, ctrlKey: false });
    await screen.findByRole("menuitem", { name: "Convidar pessoas" });
    await expectHoleThenWhole(pressEscape);
  });
});
