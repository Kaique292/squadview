import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { App } from "../App";
import { MemberMenu } from "../room/MemberMenu";
import { listenToRoomEvents, type Member, useRoomStore } from "../room/room-store";
import { MicButton } from "../voice/VoiceButtons";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { holdOverlay, openOverlays, useOverlayMenu } from "./overlay";
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
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: null, pendingWatch: null });
  // Só dentro do Tauri o app avisa o Rust para esconder o vídeo.
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  // O jsdom não rola elementos; a lista do Radix chama isso ao abrir.
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  delete (Element.prototype as Partial<Element>).scrollIntoView;
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

/** O vídeo foi escondido e, depois de fechar, voltou. */
async function expectHiddenThenShown(close: () => void) {
  expect(invoke).toHaveBeenCalledWith("set_video_hidden", { hidden: true });
  expect(openOverlays()).toBe(1);
  close();
  await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("set_video_hidden", { hidden: false }));
  expect(openOverlays()).toBe(0);
}

function pressEscape() {
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
}

describe("menus por cima do vídeo", () => {
  it("o gancho segura o vídeo escondido enquanto o menu está aberto e repassa a mudança", () => {
    const changes: boolean[] = [];
    let toggle: (open: boolean) => void = () => {};
    function Probe() {
      toggle = useOverlayMenu((open) => changes.push(open));
      return null;
    }
    const { unmount } = render(<Probe />);
    act(() => toggle(true));
    expect(openOverlays()).toBe(1);
    act(() => toggle(false));
    expect(openOverlays()).toBe(0);
    act(() => toggle(true));
    // Desmontar com o menu aberto também solta.
    unmount();
    expect(openOverlays()).toBe(0);
    expect(changes).toEqual([true, false, true]);
  });

  it("soltar duas vezes não conta em dobro, e erro do Rust não quebra nada", async () => {
    invoke.mockRejectedValue(new Error("sem janela de vídeo"));
    const release = holdOverlay();
    expect(openOverlays()).toBe(1);
    release();
    release();
    expect(openOverlays()).toBe(0);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  });

  it("o menu do botão direito num membro esconde o vídeo e devolve ao fechar", async () => {
    render(
      <MemberMenu member={ANA} isMe={false}>
        <div>Ana</div>
      </MemberMenu>,
    );
    fireEvent.contextMenu(screen.getByText("Ana"));
    await screen.findByRole("menu", { name: "Opções de Ana" });
    await expectHiddenThenShown(pressEscape);
  });

  it("a setinha do microfone esconde o vídeo enquanto o menu está aberto", async () => {
    render(<MicButton />);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Opções de entrada" }), { button: 0, ctrlKey: false });
    const menu = await screen.findByRole("menu");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_devices"));
    // As setas no volume mexem nele, não fecham nem navegam o menu.
    fireEvent.keyDown(screen.getByRole("slider", { name: "Volume de entrada" }), { key: "ArrowRight" });
    expect(menu).toBeInTheDocument();
    await expectHiddenThenShown(pressEscape);
  });

  it("a lista suspensa esconde o vídeo enquanto está aberta", async () => {
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
    await expectHiddenThenShown(() => fireEvent.click(option));
    expect(onChange).toHaveBeenCalledWith("b");
  });

  it("o menu do nome da sala esconde o vídeo enquanto está aberto", async () => {
    await listenToRoomEvents();
    render(<App />);
    act(() => handlers.get("room-members")!({ payload: [ME, ANA] }));
    // O cabeçalho da lateral é o botão "Galera" que abre um menu (a barra de salas tem outro).
    const header = screen.getAllByRole("button", { name: "Galera" }).find((b) => b.getAttribute("aria-haspopup"));
    fireEvent.pointerDown(header!, { button: 0, ctrlKey: false });
    await screen.findByRole("menuitem", { name: "Convidar pessoas" });
    await expectHiddenThenShown(pressEscape);
  });
});
