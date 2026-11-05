import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import { useRoomStore } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { lineTime, MeetingPanel } from "./MeetingPanel";
import { listenToMeetingEvents, useMeetingStore } from "./meeting-store";

const { invoke, handlers } = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  }),
}));

const SMALL = { kind: "small", size: 190_085_487 };
const BASE = { kind: "base", size: 59_707_625 };

function status(installed: boolean, cpuSupported = true) {
  return {
    cpuSupported,
    models: [
      { ...SMALL, installed },
      { ...BASE, installed: false },
    ],
  };
}

let installed = false;
let off: (() => void) | undefined;

beforeEach(async () => {
  invoke.mockReset();
  installed = false;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "meeting_status") return Promise.resolve(status(installed));
    return Promise.resolve(undefined);
  });
  localStorage.clear();
  useToastStore.setState({ toasts: [] });
  useRoomStore.setState({ code: "ABC123", members: [{ id: "eu", name: "Kaique", live: false }] });
  useMeetingStore.setState({
    open: false,
    cpuSupported: null,
    models: [],
    prefs: { model: "small", language: "pt" },
    download: null,
    starting: false,
    transcribing: false,
    lines: [],
  });
  off = await listenToMeetingEvents();
});

afterEach(() => off?.());

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)?.({ payload }));
}

async function openPanel() {
  render(<MeetingPanel />);
  act(() => useMeetingStore.getState().setOpen(true));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("meeting_status"));
}

describe("aba Reunião", () => {
  it("sem o modelo, oferece o download e mostra o progresso até poder transcrever", async () => {
    let finish: () => void = () => {};
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "meeting_status") return Promise.resolve(status(installed));
      if (cmd === "meeting_download_model")
        return new Promise<void>((resolve) => {
          finish = () => {
            installed = true;
            resolve();
          };
        });
      return Promise.resolve(undefined);
    });
    await openPanel();
    expect(await screen.findByText(/baixa uma vez o modelo de voz \(190 MB\)/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Baixar o modelo" }));
    expect(invoke).toHaveBeenCalledWith("meeting_download_model", { kind: "small" });

    emit("meeting-model-progress", { kind: "small", received: 95_000_000, total: 190_085_487 });
    const bar = screen.getByRole("progressbar", { name: "Download do modelo" });
    expect(bar).toHaveAttribute("aria-valuenow", "50");
    expect(screen.getByText("95 MB de 190 MB")).toBeInTheDocument();

    act(() => finish());
    expect(await screen.findByRole("button", { name: "Transcrever minha voz" })).toBeInTheDocument();
  });

  it("cancelar o download não mostra erro", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "meeting_status") return Promise.resolve(status(false));
      if (cmd === "meeting_download_model") return Promise.reject("Download cancelado.");
      return Promise.resolve(undefined);
    });
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Baixar o modelo" }));
    await waitFor(() => expect(useMeetingStore.getState().download).toBeNull());
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("liga e desliga a transcrição com o modelo e o idioma escolhidos", async () => {
    installed = true;
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Transcrever minha voz" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("meeting_set_transcribing", {
        enabled: true,
        model: "small",
        language: "pt",
      }),
    );
    expect(await screen.findByText("Transcrevendo")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Parar de transcrever" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("meeting_set_transcribing", {
        enabled: false,
        model: "small",
        language: "pt",
      }),
    );
    expect(screen.queryByText("Transcrevendo")).not.toBeInTheDocument();
  });

  it("detectar o idioma sozinho manda idioma nulo", async () => {
    installed = true;
    useMeetingStore.getState().setPrefs({ language: null });
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Transcrever minha voz" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("meeting_set_transcribing", {
        enabled: true,
        model: "small",
        language: null,
      }),
    );
    // A escolha fica salva neste PC.
    expect(JSON.parse(localStorage.getItem("squadview.reuniao") ?? "{}")).toMatchObject({ language: null });
  });

  it("falha ao ligar avisa e continua desligado", async () => {
    installed = true;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "meeting_status") return Promise.resolve(status(true));
      if (cmd === "meeting_set_transcribing") return Promise.reject("Entre na voz da sala para transcrever.");
      return Promise.resolve(undefined);
    });
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Transcrever minha voz" }));
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0].message).toBe("Entre na voz da sala para transcrever.");
    expect(useMeetingStore.getState().transcribing).toBe(false);
  });

  it("processador sem AVX2 avisa em vez de oferecer o download", async () => {
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "meeting_status" ? status(false, false) : undefined),
    );
    await openPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent("AVX2");
    expect(screen.queryByRole("button", { name: "Baixar o modelo" })).not.toBeInTheDocument();
  });

  it("cada fala aparece com o nome, a hora e o texto, na ordem em que foi dita", async () => {
    installed = true;
    await openPanel();
    const at = new Date(2026, 10, 5, 14, 32).getTime();
    emit("meeting-line", { kind: "text", atMs: at + 60_000, text: "e aí, bora?" });
    emit("meeting-line", { kind: "text", atMs: at, text: "bom dia, pessoal" });
    emit("meeting-line", { kind: "skipped", atMs: at + 120_000 });
    const items = within(screen.getByRole("list", { name: "Transcrição" })).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("Kaique");
    expect(items[0]).toHaveTextContent("bom dia, pessoal");
    expect(items[1]).toHaveTextContent("e aí, bora?");
    expect(items[2]).toHaveTextContent("[trecho não transcrito]");
  });

  it("sair da sala fecha a aba e apaga a transcrição", async () => {
    installed = true;
    await openPanel();
    emit("meeting-line", { kind: "text", atMs: 1, text: "oi" });
    useMeetingStore.setState({ transcribing: true });
    act(() => useRoomStore.setState({ code: null }));
    const state = useMeetingStore.getState();
    expect(state.open).toBe(false);
    expect(state.transcribing).toBe(false);
    expect(state.lines).toHaveLength(0);
  });

  it("antes do status do modelo chegar, não oferece o download", async () => {
    let answer: (value: unknown) => void = () => {};
    invoke.mockImplementation((cmd: string) =>
      cmd === "meeting_status" ? new Promise((resolve) => (answer = resolve)) : Promise.resolve(undefined),
    );
    useMeetingStore.setState({ transcribing: true });
    await openPanel();
    // Transcrevendo: o parar vale mesmo sem o status (antes, aparecia "Baixar o modelo (0 MB)").
    expect(screen.getByRole("button", { name: "Parar de transcrever" })).toBeInTheDocument();
    act(() => useMeetingStore.setState({ transcribing: false }));
    expect(screen.getByText("Conferindo o modelo…")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Baixar o modelo" })).not.toBeInTheDocument();
    await act(async () => answer(status(true)));
    expect(await screen.findByRole("button", { name: "Transcrever minha voz" })).toBeInTheDocument();
  });

  it("a hora mostra a data quando a fala não é de hoje", () => {
    const now = new Date(2026, 10, 5, 18, 0);
    expect(lineTime(new Date(2026, 10, 5, 9, 7).getTime(), now)).toBe("09:07");
    expect(lineTime(new Date(2026, 10, 4, 23, 59).getTime(), now)).toBe("04/11 23:59");
  });
});
