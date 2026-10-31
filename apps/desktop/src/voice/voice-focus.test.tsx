import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../App";
import { listenToRoomEvents, type Member, useRoomStore, type Watch } from "../room/room-store";
import { DEFAULT_PREFS, useVoiceStore } from "./voice-store";

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
const BETO: Member = { id: "cccccccccccccccc", name: "Beto", live: false, muted: true };

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  handlers.clear();
  localStorage.clear();
  useVoiceStore.setState({ prefs: DEFAULT_PREFS, peers: {}, speaking: {}, connected: [], level: 0 });
  useRoomStore.setState({ code: "B8RJ2Q", roomName: "Galera", members: [], watching: {}, pendingWatch: [] });
  await listenToRoomEvents();
});

function canal() {
  return screen.getByRole("region", { name: "Canal de voz" });
}

describe("voz com alguém em foco", () => {
  it("clicar num quadro põe a pessoa grande no centro e os outros na faixa de baixo", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Beto em foco" }));

    expect(within(canal()).queryByRole("list", { name: "Quem está na voz" })).toBeNull();
    const foco = within(canal()).getByRole("group", { name: "Em foco" });
    expect(within(foco).getByText("Beto")).toBeInTheDocument();
    expect(within(foco).getByLabelText("Microfone silenciado")).toBeInTheDocument();
    const faixa = within(canal()).getByRole("list", { name: "Outros na voz" });
    expect(within(faixa).getAllByRole("listitem")).toHaveLength(2);
    expect(within(faixa).getByText("Kaique")).toBeInTheDocument();
    expect(within(faixa).getByText("Ana")).toBeInTheDocument();
  });

  it("clicar de novo no quadro em foco volta para a grade com todos", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    const emFoco = within(canal()).getByRole("button", { name: "Voltar para a grade (Ana em foco)" });
    expect(emFoco).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(emFoco);
    const grade = within(canal()).getByRole("list", { name: "Quem está na voz" });
    expect(within(grade).getAllByRole("listitem")).toHaveLength(3);
  });

  it("um quadro da faixa troca quem está em foco", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    const faixa = within(canal()).getByRole("list", { name: "Outros na voz" });
    fireEvent.click(within(faixa).getByRole("button", { name: "Pôr Kaique em foco" }));
    expect(within(within(canal()).getByRole("group", { name: "Em foco" })).getByText("Kaique")).toBeInTheDocument();
    expect(within(within(canal()).getByRole("list", { name: "Outros na voz" })).getByText("Ana")).toBeInTheDocument();
  });

  it("funciona pelo teclado, com Enter e espaço", () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.keyDown(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }), { key: "Enter" });
    const emFoco = within(canal()).getByRole("button", { name: "Voltar para a grade (Ana em foco)" });
    // Outras teclas não mexem no foco.
    fireEvent.keyDown(emFoco, { key: "a" });
    expect(within(canal()).getByRole("group", { name: "Em foco" })).toBeInTheDocument();
    fireEvent.keyDown(emFoco, { key: " " });
    expect(within(canal()).getByRole("list", { name: "Quem está na voz" })).toBeInTheDocument();
  });

  it("a setinha recolhe e mostra a faixa", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    const esconder = within(canal()).getByRole("button", { name: "Esconder participantes" });
    expect(esconder).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(esconder);
    const mostrar = within(canal()).getByRole("button", { name: "Mostrar participantes" });
    expect(mostrar).toHaveAttribute("aria-expanded", "false");
    expect(within(canal()).getByRole("list", { name: "Outros na voz", hidden: true })).toHaveAttribute("inert");
    fireEvent.click(mostrar);
    expect(within(canal()).getByRole("list", { name: "Outros na voz" })).not.toHaveAttribute("inert");
  });

  it("a sombra dos controles não pega o clique da setinha (só os botões pegam)", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    const controles = within(canal()).getByRole("navigation", { name: "Controles da voz" });
    // A sombra tem 96 px e ficava por cima da setinha da faixa: o clique ia para ela.
    expect(controles).toHaveClass("pointer-events-none");
  });

  it("sozinho na sala, em foco não tem faixa", () => {
    render(<App />);
    emit("room-members", [ME]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Kaique em foco" }));
    expect(within(canal()).getByRole("group", { name: "Em foco" })).toBeInTheDocument();
    expect(within(canal()).queryByRole("button", { name: "Esconder participantes" })).toBeNull();
  });

  it("se quem está em foco sai da sala, volta para a grade", () => {
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    emit("room-members", [ME, BETO]);
    const grade = within(canal()).getByRole("list", { name: "Quem está na voz" });
    expect(within(grade).getAllByRole("listitem")).toHaveLength(2);
  });

  it("assistir a transmissão em foco não mexe no foco", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    const foco = within(canal()).getByRole("group", { name: "Em foco" });
    expect(within(foco).getByText("Ao vivo")).toBeInTheDocument();
    fireEvent.click(within(foco).getByRole("button", { name: "Assistir a transmissão" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", expect.anything()));
    expect(within(canal()).getByRole("group", { name: "Em foco" })).toBeInTheDocument();
  });

  it("o botão direito num quadro abre o menu do membro, de onde dá para assistir", async () => {
    render(<App />);
    emit("room-members", [ME, ANA]);
    fireEvent.contextMenu(within(canal()).getByRole("button", { name: "Pôr Ana em foco" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Assistir a transmissão/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("room_watch", { memberId: ANA.id }));
  });
});

describe("só transmissões na grade", () => {
  /** Já assistindo a live da Ana (antes de abrir a sala, para ela não ir para o foco). */
  function assistindoAna() {
    const watch: Watch = { member: ANA, stats: null, history: [], listening: true, volume: 100 };
    useRoomStore.setState({ watching: { [ANA.id]: watch } });
  }

  it("deixa só as lives na grade e põe as pessoas na faixa de baixo", () => {
    assistindoAna();
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    expect(
      within(within(canal()).getByRole("list", { name: "Quem está na voz" })).getAllByRole("listitem"),
    ).toHaveLength(4);

    const botao = within(canal()).getByRole("button", { name: "Só transmissões na grade" });
    expect(botao).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(botao);

    expect(botao).toHaveAttribute("aria-pressed", "true");
    const grade = within(canal()).getByRole("list", { name: "Quem está na voz" });
    expect(within(grade).getAllByRole("listitem")).toHaveLength(1);
    expect(within(grade).getByRole("button", { name: /live de Ana/ })).toBeInTheDocument();
    const faixa = within(canal()).getByRole("list", { name: "Outros na voz" });
    expect(within(faixa).getAllByRole("listitem")).toHaveLength(3);
    expect(within(faixa).getByText("Kaique")).toBeInTheDocument();
    expect(within(faixa).getByText("Beto")).toBeInTheDocument();

    // A setinha recolhe a faixa, como no foco.
    fireEvent.click(within(canal()).getByRole("button", { name: "Esconder participantes" }));
    expect(within(canal()).getByRole("list", { name: "Outros na voz", hidden: true })).toHaveAttribute("inert");

    fireEvent.click(botao);
    expect(
      within(within(canal()).getByRole("list", { name: "Quem está na voz" })).getAllByRole("listitem"),
    ).toHaveLength(4);
    expect(within(canal()).queryByRole("list", { name: "Outros na voz", hidden: true })).toBeNull();
  });

  it("um quadro da faixa põe a pessoa em foco", () => {
    assistindoAna();
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Só transmissões na grade" }));
    const faixa = within(canal()).getByRole("list", { name: "Outros na voz" });
    fireEvent.click(within(faixa).getByRole("button", { name: "Pôr Beto em foco" }));
    expect(within(within(canal()).getByRole("group", { name: "Em foco" })).getByText("Beto")).toBeInTheDocument();
  });

  it("lembra a escolha na próxima vez que abre a sala", () => {
    assistindoAna();
    const { unmount } = render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    fireEvent.click(within(canal()).getByRole("button", { name: "Só transmissões na grade" }));
    unmount();

    render(<App />);
    expect(within(canal()).getByRole("button", { name: "Só transmissões na grade" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      within(within(canal()).getByRole("list", { name: "Quem está na voz" })).getAllByRole("listitem"),
    ).toHaveLength(1);
  });

  it("sem nenhuma live na tela, a grade mostra todos e o botão some", () => {
    localStorage.setItem("squadview.so-transmissoes", "1");
    render(<App />);
    emit("room-members", [ME, ANA, BETO]);
    expect(
      within(within(canal()).getByRole("list", { name: "Quem está na voz" })).getAllByRole("listitem"),
    ).toHaveLength(3);
    expect(within(canal()).queryByRole("button", { name: "Só transmissões na grade" })).toBeNull();
    expect(within(canal()).queryByRole("list", { name: "Outros na voz", hidden: true })).toBeNull();
  });
});
