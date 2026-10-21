import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useAccountStore } from "../account/account-store";
import { DEFAULT_PREFS, useVoiceStore } from "../voice/voice-store";
import { SettingsDialog } from "./SettingsDialog";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const PROFILE = { id: "123456789012345678", username: "kaique", globalName: "K.A", avatar: null };
const DEVICES = {
  inputs: [{ id: "mic-1", name: "Microfone do headset", isDefault: true }],
  outputs: [{ id: "out-1", name: "Fone USB", isDefault: true }],
};

/** O jsdom não tem rolagem: a página e as listas suspensas usam esta versão falsa. */
const scrollIntoView = vi.fn();

beforeEach(() => {
  scrollIntoView.mockReset();
  Element.prototype.scrollIntoView = scrollIntoView;
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "voice_devices" ? DEVICES : undefined));
  localStorage.clear();
  useAccountStore.setState({ profile: null });
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, devices: { inputs: [], outputs: [] }, level: 0, speaking: {} });
});

afterEach(() => {
  delete (Element.prototype as Partial<Element>).scrollIntoView;
});

function openSettings() {
  render(<SettingsDialog trigger={<button type="button">Configurações</button>} />);
  fireEvent.click(screen.getByRole("button", { name: "Configurações" }));
  return screen.getByRole("navigation", { name: "Categorias" });
}

describe("configurações: conta", () => {
  it("sem login, a conta oferece entrar com o Discord", () => {
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: /Você/ }));
    expect(screen.getByRole("heading", { name: "Conta do Discord" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Entrar com o Discord/ }).length).toBeGreaterThan(0);
    expect(within(nav).queryByRole("button", { name: "Sair" })).not.toBeInTheDocument();
  });

  it("com login, mostra o perfil e sai da conta", async () => {
    useAccountStore.setState({ profile: PROFILE });
    const nav = openSettings();
    expect(within(nav).getByText("Editar perfil")).toBeInTheDocument();
    fireEvent.click(within(nav).getByRole("button", { name: "Minha conta" }));
    expect(screen.getByText("@kaique · Discord")).toBeInTheDocument();
    const page = screen.getByRole("heading", { name: "Conta do Discord" }).parentElement!;
    fireEvent.click(within(page).getByRole("button", { name: "Sair" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("discord_logout"));
    await waitFor(() => expect(useAccountStore.getState().profile).toBeNull());
  });

  it("o Sair da lateral também desconecta", async () => {
    useAccountStore.setState({ profile: PROFILE });
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: "Sair" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("discord_logout"));
  });
});

describe("configurações: busca e navegação", () => {
  it("a busca filtra as categorias e avisa quando não acha nada", () => {
    const nav = openSettings();
    const busca = within(nav).getByRole("searchbox", { name: "Buscar nas configurações" });
    fireEvent.change(busca, { target: { value: "microfone" } });
    expect(within(nav).getByRole("button", { name: "Voz e vídeo" })).toBeInTheDocument();
    expect(within(nav).queryByRole("button", { name: "Aparência" })).not.toBeInTheDocument();
    fireEvent.change(busca, { target: { value: "xyz123" } });
    expect(within(nav).getByText("Nada encontrado.")).toBeInTheDocument();
  });

  it("os subitens de Voz e vídeo rolam até a parte certa da página", () => {
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: "Teste do microfone" }));
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
    expect(scrollIntoView.mock.contexts[0]).toBe(document.getElementById("voz-teste"));
  });

  it("abre a página de desempenho", async () => {
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "diagnostics_status" ? { enabled: false, files: 0 } : undefined),
    );
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: "Desempenho" }));
    expect(await screen.findByRole("switch", { name: "Gravar registro de desempenho" })).toBeInTheDocument();
  });
});

describe("configurações: voz", () => {
  it("escolhe o microfone e a saída pelas listas", async () => {
    openSettings();
    const mic = screen.getByRole("combobox", { name: "Microfone" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("voice_devices"));
    fireEvent.click(mic);
    fireEvent.click(await screen.findByRole("option", { name: "Microfone do headset" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.inputDevice).toBe("mic-1"));

    fireEvent.click(screen.getByRole("combobox", { name: "Saída" }));
    fireEvent.click(await screen.findByRole("option", { name: "Fone USB" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.outputDevice).toBe("out-1"));

    // "Padrão do Windows" volta a não ter escolha salva.
    fireEvent.click(screen.getByRole("combobox", { name: "Saída" }));
    fireEvent.click(await screen.findByRole("option", { name: "Padrão do Windows" }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.outputDevice).toBeNull());
  });

  it("muda os volumes e o isolamento de voz, e mostra o nível do microfone", async () => {
    useVoiceStore.setState({ level: 0.25, speaking: { me: true } });
    openSettings();
    fireEvent.change(screen.getByRole("slider", { name: "Volume do microfone" }), { target: { value: "120" } });
    fireEvent.change(screen.getByRole("slider", { name: "Volume da saída" }), { target: { value: "80" } });
    fireEvent.click(screen.getByRole("switch", { name: "Isolamento de voz" }));
    await waitFor(() =>
      expect(useVoiceStore.getState().prefs).toMatchObject({
        inputVolume: 120,
        outputVolume: 80,
        noiseSuppression: false,
      }),
    );
    const meter = screen.getByRole("meter", { name: "Nível do microfone" });
    expect(meter).toHaveAttribute("aria-valuenow", "25");
    expect(meter.querySelectorAll(".bg-online")).toHaveLength(13);
  });

  it("controle automático de ganho vem ligado e dá para desligar", async () => {
    openSettings();
    const agc = screen.getByRole("switch", { name: "Controle automático de ganho" });
    expect(agc).toBeChecked();
    fireEvent.click(agc);
    await waitFor(() => expect(useVoiceStore.getState().prefs.autoGain).toBe(false));
    expect(invoke).toHaveBeenCalledWith(
      "voice_set_settings",
      expect.objectContaining({ prefs: expect.objectContaining({ autoGain: false }) }),
    );
  });
});

describe("configurações: amigos e sons", () => {
  it("Minha conta mostra se os amigos estão ativos, ativando ou desligados", async () => {
    const { useFriendsStore } = await import("../friends/friends-store");
    useAccountStore.setState({ profile: PROFILE });
    useFriendsStore.setState({ status: "online" });
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: "Minha conta" }));
    expect(screen.getByText(/Amigos ativos/)).toBeInTheDocument();
    act(() => useFriendsStore.setState({ status: "connecting" }));
    expect(screen.getByText("Ativando os amigos…")).toBeInTheDocument();
    // A sessão não saiu: dá para entrar de novo ali mesmo, sem ir na página de amigos.
    act(() => useFriendsStore.setState({ status: "signed-out" }));
    expect(screen.getByText(/Os amigos não estão ativos/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Entrar com o Discord/ }).length).toBeGreaterThan(0);
  });

  it("Sons: volume, cada som liga e desliga, e o botão de ouvir toca", async () => {
    vi.useFakeTimers();
    const { useSoundStore } = await import("../sounds/sound-store");
    useSoundStore.setState({ volume: 60, off: [] });
    const nav = openSettings();
    fireEvent.click(within(nav).getByRole("button", { name: "Sons" }));
    const volume = screen.getByRole("slider", { name: "Volume dos avisos" });
    fireEvent.change(volume, { target: { value: "30" } });
    fireEvent.pointerUp(volume);
    expect(useSoundStore.getState().volume).toBe(30);
    fireEvent.click(screen.getByRole("switch", { name: "Mudo" }));
    expect(useSoundStore.getState().off).toEqual(["mute", "unmute"]);
    // Som desligado: o botão de ouvir fica desativado; os outros tocam (os dois sons, um depois do outro).
    expect(screen.getByRole("button", { name: "Ouvir: Mudo" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Ouvir: Surdo" }));
    act(() => vi.advanceTimersByTime(1000));
    fireEvent.click(screen.getByRole("switch", { name: "Mudo" }));
    expect(useSoundStore.getState().off).toEqual([]);
    vi.useRealTimers();
  });
});
