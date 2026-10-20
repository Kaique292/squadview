import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { DeafenButton, MicButton } from "./VoiceButtons";
import { DEFAULT_PREFS, useVoiceStore } from "./voice-store";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const DEVICES = {
  inputs: [
    { id: "mic-1", name: "Microfone do headset", isDefault: true },
    { id: "mic-2", name: "Microfone da webcam", isDefault: false },
  ],
  outputs: [
    { id: "out-1", name: "Caixas de som", isDefault: false },
    { id: "out-2", name: "Fone USB", isDefault: false },
  ],
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "voice_devices" ? DEVICES : undefined));
  localStorage.clear();
  useVoiceStore.setState({
    prefs: DEFAULT_PREFS,
    peers: {},
    speaking: {},
    connected: [],
    level: 0,
    devices: { inputs: [], outputs: [] },
  });
});

/** Abre o menu da setinha (o Radix abre no "pointerdown" do botão esquerdo). */
async function openMenu(name: string) {
  fireEvent.pointerDown(screen.getByRole("button", { name }), { button: 0, ctrlKey: false });
  return screen.findByRole("menu");
}

describe("botões de microfone e fone", () => {
  it("a setinha do microfone busca os dispositivos e mostra o padrão do Windows em uso", async () => {
    render(<MicButton />);
    const menu = await openMenu("Opções de entrada");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_devices"));
    // Sem escolha salva, mostra o dispositivo que o Windows marca como padrão.
    expect(await within(menu).findByText("Microfone do headset")).toBeInTheDocument();
    expect(within(menu).getByText("Dispositivo de entrada")).toBeInTheDocument();
  });

  it("troca o microfone pelo submenu e manda a escolha para o Rust", async () => {
    render(<MicButton />);
    const menu = await openMenu("Opções de entrada");
    await within(menu).findByText("Microfone do headset");
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Dispositivo de entrada/ }));
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "Microfone da webcam" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("voice_set_settings", {
        prefs: expect.objectContaining({ inputDevice: "mic-2" }),
      }),
    );
    expect(useVoiceStore.getState().prefs.inputDevice).toBe("mic-2");
  });

  it("voltar para o padrão do Windows limpa a escolha do microfone", async () => {
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, inputDevice: "mic-2" } });
    render(<MicButton />);
    const menu = await openMenu("Opções de entrada");
    expect(await within(menu).findByText("Microfone da webcam")).toBeInTheDocument();
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Dispositivo de entrada/ }));
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "Padrão do Windows" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.inputDevice).toBeNull());
  });

  it("muda o volume de entrada e o isolamento de voz pelo menu", async () => {
    render(<MicButton large />);
    const menu = await openMenu("Opções de entrada");
    fireEvent.change(within(menu).getByRole("slider", { name: "Volume de entrada" }), { target: { value: "150" } });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("voice_set_settings", {
        prefs: expect.objectContaining({ inputVolume: 1.5 }),
      }),
    );
    expect(within(menu).getByText("150%")).toBeInTheDocument();
    const isolamento = within(menu).getByRole("menuitemcheckbox", { name: /Isolamento de voz/ });
    expect(isolamento).toHaveAttribute("aria-checked", "true");
    fireEvent.click(isolamento);
    await waitFor(() => expect(useVoiceStore.getState().prefs.noiseSuppression).toBe(false));
    // O menu continua aberto para mexer em outras coisas.
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("o nível de entrada acende as barrinhas, em verde quando estou falando", async () => {
    render(<MicButton />);
    const menu = await openMenu("Opções de entrada");
    act(() => useVoiceStore.setState({ level: 0.5, speaking: { me: true } }));
    const meter = within(menu).getByRole("meter", { name: "Nível de entrada" });
    expect(meter).toHaveAttribute("aria-valuenow", "50");
    expect(meter.querySelectorAll(".bg-online")).toHaveLength(19);
    act(() => useVoiceStore.setState({ speaking: { me: false } }));
    expect(meter.querySelectorAll(".bg-online")).toHaveLength(0);
    expect(meter.querySelectorAll(".bg-text-muted")).toHaveLength(19);
  });

  it("ensurdecido, o microfone aparece cortado e o clique liga os dois", async () => {
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, deafened: true } });
    render(
      <>
        <MicButton large />
        <DeafenButton large />
      </>,
    );
    expect(screen.getByRole("button", { name: "Ativar áudio" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "Ativar microfone" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs).toMatchObject({ muted: false, deafened: false }));
    expect(screen.getByRole("button", { name: "Silenciar microfone" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Ensurdecer" })).toBeInTheDocument();
  });

  it("a setinha do fone troca a saída e o volume de saída", async () => {
    render(<DeafenButton />);
    const menu = await openMenu("Opções de saída");
    // Sem padrão marcado pelo Windows, aparece "Padrão do Windows".
    expect(await within(menu).findByText("Padrão do Windows")).toBeInTheDocument();
    fireEvent.change(within(menu).getByRole("slider", { name: "Volume de saída" }), { target: { value: "50" } });
    await waitFor(() => expect(useVoiceStore.getState().prefs.outputVolume).toBe(50));
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Dispositivo de saída/ }));
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "Fone USB" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.outputDevice).toBe("out-2"));
  });

  it("o clique no fone ensurdece", async () => {
    render(<DeafenButton />);
    fireEvent.click(screen.getByRole("button", { name: "Ensurdecer" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_set_deafened", { deafened: true }));
  });

  it("lista de dispositivos fora do esperado não derruba o menu", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "voice_devices" ? { inputs: null } : undefined));
    render(<MicButton />);
    const menu = await openMenu("Opções de entrada");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_devices"));
    expect(within(menu).getByText("Padrão do Windows")).toBeInTheDocument();
    expect(useVoiceStore.getState().devices).toEqual({ inputs: [], outputs: [] });
  });

  it("sem o Rust, a busca de dispositivos falha em silêncio", async () => {
    invoke.mockRejectedValue(new Error("fora do app"));
    await useVoiceStore.getState().loadDevices();
    expect(useVoiceStore.getState().devices).toEqual({ inputs: [], outputs: [] });
  });
});
