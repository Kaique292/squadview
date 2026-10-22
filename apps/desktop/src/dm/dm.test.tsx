import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useAccountStore } from "../account/account-store";
import { App } from "../App";
import { listenToFriendsEvents, useFriendsStore } from "../friends/friends-store";
import { listenToRoomEvents, useRoomStore } from "../room/room-store";
import { SettingsDialog } from "../settings/SettingsDialog";
import { useToastStore } from "../ui/toast-store";
import {
  AUTO_DOWNLOAD_MAX,
  RETRY_EVERY_MS,
  autoDownloads,
  type DmMessage,
  fileSize,
  listenToDmEvents,
  useDmStore,
} from "./dm-store";
import { stamp } from "./ChatView";

const { invoke, listen, handlers, drop } = vi.hoisted(() => {
  const handlers = new Map<string, (e: { payload: unknown }) => void>();
  return {
    handlers,
    drop: { handler: null as null | ((e: { payload: { type: string; paths?: string[] } }) => void), off: vi.fn() },
    invoke: vi.fn(),
    listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
      handlers.set(name, cb);
      return () => handlers.delete(name);
    }),
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (p: string) => `asset://${p}` }));
const { playCue } = vi.hoisted(() => ({ playCue: vi.fn() }));
vi.mock("../sounds/sounds", async (original) => ({ ...(await original<object>()), playCue }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (cb: NonNullable<typeof drop.handler>) => {
      drop.handler = cb;
      return drop.off;
    },
  }),
}));

const ME = "999999999999999999";
const BETO = "222222222222222222";
const ONLINE = { online: true, inRoom: false, room: null, live: false };

function msg(partial: Partial<DmMessage> & { id: string }): DmMessage {
  return { mine: false, at: Date.UTC(2026, 9, 21, 15, 0), status: "received", ...partial };
}

function emit(name: string, payload: unknown) {
  act(() => handlers.get(name)!({ payload }));
}

/** Respostas do Rust de mentira para cada comando. */
let answers: Record<string, unknown> = {};
let offs: (() => void)[] = [];

beforeEach(async () => {
  answers = {
    friends_status: "online",
    dm_conversations: [],
    dm_messages: [],
    dm_online: false,
    dm_pick_files: [],
    dm_pick_save: null,
    dm_preview: "C:/anexos/foto.png",
    dm_send_text: msg({ id: "x", mine: true, status: "pending", text: "oi" }),
  };
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string) => {
    const a = answers[cmd];
    if (a instanceof Error) throw a.message;
    return a;
  });
  handlers.clear();
  localStorage.clear();
  useToastStore.setState({ toasts: [] });
  useAccountStore.setState({ profile: { id: ME, username: "k", globalName: "Kaique", avatar: null } });
  useRoomStore.setState({ code: null, members: [], watching: {} });
  useFriendsStore.setState({
    status: "online",
    open: true,
    friends: [{ id: BETO, name: "Beto", avatar: null, direct: false, presence: ONLINE }],
  });
  useDmStore.setState({
    conversations: [],
    messages: {},
    open: null,
    online: {},
    unread: {},
    pendingFiles: null,
    copyChoice: "ask",
    downloading: {},
    replying: {},
  });
  offs = [await listenToDmEvents(), await listenToFriendsEvents(), await listenToRoomEvents()];
  useFriendsStore.setState({
    friends: [{ id: BETO, name: "Beto", avatar: null, direct: false, presence: ONLINE }],
  });
});

afterEach(() => offs.forEach((off) => off()));

function calls(cmd: string) {
  return invoke.mock.calls.filter(([c]) => c === cmd).map(([, args]) => args);
}

async function openBeto() {
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Mensagem" }));
  return screen.findByRole("region", { name: "Conversa com Beto" });
}

describe("mensagens privadas", () => {
  it("regras: fotos pequenas chegam sozinhas, o resto espera; tamanhos para gente ler", () => {
    const foto = msg({ id: "1", file: { name: "a.png", size: 1000, mime: "image/png", path: null, done: 0 } });
    expect(autoDownloads(foto)).toBe(true);
    expect(autoDownloads({ ...foto, mine: true })).toBe(false);
    expect(autoDownloads({ ...foto, file: { ...foto.file!, size: AUTO_DOWNLOAD_MAX + 1 } })).toBe(false);
    expect(autoDownloads({ ...foto, file: { ...foto.file!, mime: "video/mp4" } })).toBe(false);
    expect(autoDownloads({ ...foto, file: { ...foto.file!, path: "C:/a.png" } })).toBe(false);
    expect(fileSize(500)).toBe("500 B");
    expect(fileSize(2048)).toBe("2 KB");
    expect(fileSize(1.5 * 1024 * 1024)).toBe("1,5 MB");
    expect(fileSize(2.25 * 1024 * 1024 * 1024)).toBe("2,3 GB");
  });

  it("sem conversas, a lateral diz isso; com, mostra a última mensagem e as não lidas", async () => {
    render(<App />);
    const dms = screen.getByRole("complementary", { name: "Mensagens diretas" });
    expect(within(dms).getByText("Nenhuma conversa ainda.")).toBeInTheDocument();

    emit("dm-event", { type: "message", friend: BETO, message: msg({ id: "1", text: "bora jogar?" }) });
    const row = await within(dms).findByRole("button", { name: "Conversa com Beto" });
    expect(row).toHaveTextContent("bora jogar?");
    expect(within(row).getByLabelText("1 não lida(s)")).toBeInTheDocument();
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: msg({ id: "2", mine: true, text: "bora", status: "pending" }),
    });
    expect(row).toHaveTextContent("Você: bora");

    fireEvent.click(row);
    await screen.findByRole("region", { name: "Conversa com Beto" });
    expect(within(row).queryByLabelText(/não lida/)).not.toBeInTheDocument();
    fireEvent.click(within(dms).getByRole("button", { name: "Amigos" }));
    expect(screen.getByRole("region", { name: "Amigos" })).toBeInTheDocument();
  });

  it("abre a conversa pelo amigo, manda com Enter e mostra pendente e entregue", async () => {
    answers.dm_messages = [msg({ id: "1", text: "e aí" })];
    const chat = await openBeto();
    expect(await within(chat).findByText("e aí")).toBeInTheDocument();
    expect(within(chat).getByText(/Online/)).toBeInTheDocument();

    const caixa = within(chat).getByRole("textbox", { name: "Mensagem para Beto" });
    fireEvent.change(caixa, { target: { value: "bora\njogar" } });
    fireEvent.keyDown(caixa, { key: "Enter", shiftKey: true });
    expect(calls("dm_send_text")).toHaveLength(0);
    fireEvent.keyDown(caixa, { key: "Enter" });
    await waitFor(() => expect(calls("dm_send_text")).toEqual([{ friend: BETO, text: "bora\njogar", replyTo: null }]));
    await waitFor(() => expect(caixa).toHaveValue(""));

    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: msg({ id: "2", mine: true, text: "bora", status: "pending" }),
    });
    // Beto está online: a mensagem está conectando, não esperando ele aparecer.
    expect(within(chat).getByText("Conectando com Beto…")).toBeInTheDocument();
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: msg({ id: "2", mine: true, text: "bora", status: "delivered" }),
    });
    expect(within(chat).getByText("Entregue")).toBeInTheDocument();
    emit("dm-event", { type: "online", friend: BETO, online: true });
    expect(within(chat).getByText("Conectado direto")).toBeInTheDocument();
    emit("dm-event", { type: "unreachable", friend: BETO });
    expect(within(chat).queryByText("Conectado direto")).not.toBeInTheDocument();
  });

  it("fecha a conversa mesmo com o scrollIntoView do Chromium novo, que devolve uma Promise", async () => {
    Element.prototype.scrollIntoView = vi.fn(() => Promise.resolve()) as unknown as Element["scrollIntoView"];
    try {
      answers.dm_messages = [msg({ id: "1", text: "oi" })];
      const chat = await openBeto();
      await within(chat).findByText("oi");
      fireEvent.click(
        within(screen.getByRole("complementary", { name: "Mensagens diretas" })).getByRole("button", {
          name: "Amigos",
        }),
      );
      expect(await screen.findByRole("region", { name: "Amigos" })).toBeInTheDocument();
    } finally {
      delete (Element.prototype as Partial<Element>).scrollIntoView;
    }
  });

  it("não deixando mandar, avisa e não limpa a caixa", async () => {
    answers.dm_send_text = new Error("mensagem vazia");
    const chat = await openBeto();
    const caixa = within(chat).getByRole("textbox", { name: "Mensagem para Beto" });
    fireEvent.change(caixa, { target: { value: "oi" } });
    fireEvent.click(within(chat).getByRole("button", { name: "Enviar" }));
    await waitFor(() => expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "mensagem vazia" }));
    expect(caixa).toHaveValue("oi");
  });

  it("primeiro anexo pergunta da cópia; lembrar a escolha não pergunta de novo", async () => {
    answers.dm_pick_files = ["C:/Videos/clipe.mp4"];
    const chat = await openBeto();
    fireEvent.click(within(chat).getByRole("button", { name: "Anexar arquivos" }));
    const modal = await screen.findByRole("dialog", { name: "Mandar arquivo" });
    expect(calls("dm_send_file")).toHaveLength(0);
    fireEvent.click(within(modal).getByRole("checkbox"));
    fireEvent.click(within(modal).getByRole("button", { name: /Guardar uma cópia/ }));
    await waitFor(() =>
      expect(calls("dm_send_file")).toEqual([{ friend: BETO, path: "C:/Videos/clipe.mp4", keepCopy: true }]),
    );
    expect(useDmStore.getState().copyChoice).toBe("keep");
    expect(localStorage.getItem("squadview.anexos.copia")).toBe("keep");

    fireEvent.click(within(chat).getByRole("button", { name: "Anexar arquivos" }));
    await waitFor(() => expect(calls("dm_send_file")).toHaveLength(2));
    expect(screen.queryByRole("dialog", { name: "Mandar arquivo" })).not.toBeInTheDocument();
  });

  it("só mandar sem lembrar pergunta de novo; fechar o modal não manda nada", async () => {
    answers.dm_pick_files = ["C:/a.zip", "C:/b.zip"];
    const chat = await openBeto();
    fireEvent.click(within(chat).getByRole("button", { name: "Anexar arquivos" }));
    let modal = await screen.findByRole("dialog", { name: "Mandar 2 arquivos" });
    fireEvent.click(within(modal).getByRole("button", { name: /Só mandar/ }));
    await waitFor(() => expect(calls("dm_send_file")).toHaveLength(2));
    expect(calls("dm_send_file")[0]).toMatchObject({ keepCopy: false });
    expect(useDmStore.getState().copyChoice).toBe("ask");

    fireEvent.click(within(chat).getByRole("button", { name: "Anexar arquivos" }));
    modal = await screen.findByRole("dialog", { name: "Mandar 2 arquivos" });
    fireEvent.keyDown(modal, { key: "Escape" });
    await waitFor(() => expect(useDmStore.getState().pendingFiles).toBeNull());
    expect(calls("dm_send_file")).toHaveLength(2);
    // Cancelar a escolha de arquivos não abre nada.
    answers.dm_pick_files = [];
    fireEvent.click(within(chat).getByRole("button", { name: "Anexar arquivos" }));
    await waitFor(() => expect(calls("dm_pick_files")).toHaveLength(3));
    expect(useDmStore.getState().pendingFiles).toBeNull();
  });

  it("foto pequena chega sozinha e aparece; arquivo grande pede onde salvar e mostra o progresso", async () => {
    const chat = await openBeto();
    const foto = msg({ id: "f", file: { name: "print.png", size: 50_000, mime: "image/png", path: null, done: 0 } });
    emit("dm-event", { type: "message", friend: BETO, message: foto });
    await waitFor(() => expect(calls("dm_download")).toEqual([{ friend: BETO, id: "f", dest: null }]));
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: { ...foto, file: { ...foto.file!, path: "C:/anexos/f-print.png", done: 50_000 } },
    });
    const img = await within(chat).findByRole("img", { name: "print.png" });
    expect(img).toHaveAttribute("src", "asset://C:/anexos/foto.png");
    // A foto cresce a conversa ao carregar: continua no fim.
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    fireEvent.load(img);
    expect(scroll).toHaveBeenCalledWith({ block: "end" });
    delete (Element.prototype as Partial<Element>).scrollIntoView;
    fireEvent.click(within(chat).getByRole("button", { name: "Abrir print.png" }));
    expect(calls("dm_open")).toEqual([{ friend: BETO, id: "f" }]);

    const zip = msg({
      id: "z",
      file: { name: "mapa.zip", size: 10_000_000, mime: "application/zip", path: null, done: 0 },
    });
    emit("dm-event", { type: "message", friend: BETO, message: zip });
    expect(calls("dm_download")).toHaveLength(1);
    answers.dm_pick_save = "D:/Downloads/mapa.zip";
    fireEvent.click(within(chat).getByRole("button", { name: /Baixar/ }));
    await waitFor(() =>
      expect(calls("dm_download")).toContainEqual({ friend: BETO, id: "z", dest: "D:/Downloads/mapa.zip" }),
    );
    emit("dm-event", { type: "message", friend: BETO, message: { ...zip, file: { ...zip.file!, done: 5_000_000 } } });
    expect(within(chat).getByRole("progressbar", { name: "Baixando mapa.zip" })).toHaveAttribute("aria-valuenow", "50");
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: { ...zip, file: { ...zip.file!, done: 10_000_000, path: "D:/Downloads/mapa.zip" } },
    });
    fireEvent.click(within(chat).getByRole("button", { name: "Mostrar mapa.zip na pasta" }));
    expect(calls("dm_reveal")).toEqual([{ friend: BETO, id: "z" }]);
    fireEvent.click(within(chat).getByRole("button", { name: "Abrir mapa.zip" }));
    expect(calls("dm_open")).toContainEqual({ friend: BETO, id: "z" });
  });

  it("arrastar arquivos para a janela manda para a conversa aberta", async () => {
    Object.assign(window, { __TAURI_INTERNALS__: {} });
    useDmStore.setState({ copyChoice: "send" });
    try {
      const chat = await openBeto();
      await waitFor(() => expect(drop.handler).not.toBeNull());
      const caixa = within(chat).getByRole("textbox", { name: "Mensagem para Beto" });
      act(() => drop.handler!({ payload: { type: "enter" } }));
      expect(caixa).toHaveAttribute("placeholder", "Solte para mandar");
      act(() => drop.handler!({ payload: { type: "drop", paths: ["C:/a.txt"] } }));
      expect(caixa).toHaveAttribute("placeholder", "Conversar com Beto");
      await waitFor(() => expect(calls("dm_send_file")).toEqual([{ friend: BETO, path: "C:/a.txt", keepCopy: false }]));
      // Arquivo que não dá para mandar avisa, e os outros seguem.
      answers.dm_send_file = new Error("Arquivo grande demais.");
      act(() => drop.handler!({ payload: { type: "drop", paths: ["C:/b.iso"] } }));
      await waitFor(() =>
        expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "Arquivo grande demais." }),
      );
      fireEvent.click(
        within(screen.getByRole("complementary", { name: "Mensagens diretas" })).getByRole("button", {
          name: "Amigos",
        }),
      );
      expect(drop.off).toHaveBeenCalled();
    } finally {
      delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
      drop.handler = null;
    }
  });

  it("separa os dias, agrupa a mesma pessoa e mostra as fotos de perfil", async () => {
    useFriendsStore.setState({
      friends: [{ id: BETO, name: "Beto", avatar: "abc", direct: false, presence: { ...ONLINE, online: false } }],
    });
    useAccountStore.setState({ profile: null });
    const ontem = Date.UTC(2026, 9, 20, 15, 0);
    answers.dm_messages = [
      msg({ id: "1", at: ontem, text: "ontem" }),
      msg({ id: "2", at: ontem + 1000, text: "junto" }),
      msg({ id: "3", text: "hoje", mine: true, status: "delivered" }),
    ];
    useDmStore.setState({
      conversations: [
        { friend: BETO, last: null, pending: 0 },
        {
          friend: "4",
          last: msg({ id: "f", file: { name: "x.zip", size: 1, mime: "application/zip", path: null, done: 0 } }),
          pending: 0,
        },
      ],
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Conversa com Beto" }));
    const chat = await screen.findByRole("region", { name: "Conversa com Beto" });
    await within(chat).findByText("ontem");
    expect(within(chat).getByText(/20 de outubro/)).toBeInTheDocument();
    expect(within(chat).getByText(/21 de outubro/)).toBeInTheDocument();
    expect(within(chat).getAllByText("Beto", { selector: "span.font-semibold" })).toHaveLength(2);
    expect(within(chat).getByText("Você")).toBeInTheDocument();
    expect(within(chat).getByText(/Offline/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conversa com Alguém" })).toHaveTextContent("Anexo: x.zip");
  });

  it("vídeo baixado toca na conversa; cancelar o salvar não baixa; erro ao baixar avisa", async () => {
    answers.dm_messages = [
      msg({ id: "v", file: { name: "clipe.mp4", size: 1, mime: "video/mp4", path: "C:/clipe.mp4", done: 1 } }),
      msg({ id: "d", file: { name: "doc.pdf", size: 1, mime: "application/pdf", path: null, done: 0 } }),
    ];
    answers.dm_preview = "C:/clipe.mp4";
    const chat = await openBeto();
    expect(await within(chat).findByLabelText("clipe.mp4")).toHaveAttribute("src", "asset://C:/clipe.mp4");
    fireEvent.click(within(chat).getByRole("button", { name: /Baixar/ }));
    await waitFor(() => expect(calls("dm_pick_save")).toHaveLength(1));
    expect(calls("dm_download")).toHaveLength(0);

    answers.dm_download = new Error("Esse anexo não está nesta conversa.");
    answers.dm_pick_save = "D:/doc.pdf";
    fireEvent.click(within(chat).getByRole("button", { name: /Baixar/ }));
    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "Esse anexo não está nesta conversa." }),
    );
    expect(useDmStore.getState().downloading.d).toBe(false);
  });

  it("anexo que sumiu do PC fica como cartão; amigo que saiu da lista aparece como Alguém", async () => {
    answers.dm_messages = [
      msg({
        id: "p",
        mine: true,
        status: "delivered",
        file: { name: "a.png", size: 9, mime: "image/png", path: "C:/a.png", done: 9 },
      }),
    ];
    answers.dm_preview = new Error("O arquivo não está mais onde estava.");
    useDmStore.setState({
      conversations: [{ friend: "333333333333333333", last: msg({ id: "o", text: "oi" }), pending: 0 }],
    });
    const chat = await openBeto();
    expect(await within(chat).findByText("a.png")).toBeInTheDocument();
    expect(within(chat).queryByRole("img", { name: "a.png" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conversa com Alguém" })).toBeInTheDocument();
  });

  it("erro ao abrir a conversa avisa; sair da conta limpa as conversas", async () => {
    answers.dm_messages = new Error("Entre com o Discord para conversar.");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Mensagem" }));
    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]).toMatchObject({ message: "Entre com o Discord para conversar." }),
    );
    emit("friends-status", "signed-out");
    expect(useDmStore.getState()).toMatchObject({ conversations: [], open: null });
    answers.dm_conversations = [{ friend: BETO, last: msg({ id: "1", text: "oi" }), pending: 0 }];
    emit("friends-status", "online");
    await waitFor(() => expect(useDmStore.getState().conversations).toHaveLength(1));
    answers.dm_conversations = new Error("Entre com o Discord");
    await act(() => useDmStore.getState().load());
    expect(useDmStore.getState().conversations).toHaveLength(1);
  });

  it("com o amigo offline, a pendente espera ele ficar online", async () => {
    useFriendsStore.setState({
      friends: [{ id: BETO, name: "Beto", avatar: null, direct: false, presence: { ...ONLINE, online: false } }],
    });
    answers.dm_messages = [msg({ id: "1", mine: true, status: "pending", text: "tá aí?" })];
    render(<App />);
    await act(() => useDmStore.getState().openChat(BETO));
    const chat = await screen.findByRole("region", { name: "Conversa com Beto" });
    expect(await within(chat).findByText("Esperando Beto ficar online")).toBeInTheDocument();
  });

  it("o amigo ficando online faz a pendente tentar de novo, sem precisar mandar outra", async () => {
    useDmStore.setState({
      conversations: [{ friend: BETO, last: msg({ id: "1", mine: true, status: "pending" }), pending: 1 }],
    });
    act(() =>
      useFriendsStore.setState({
        friends: [{ id: BETO, name: "Beto", avatar: null, direct: false, presence: { ...ONLINE, online: false } }],
      }),
    );
    expect(calls("dm_retry")).toHaveLength(0);
    act(() =>
      useFriendsStore.setState({
        friends: [{ id: BETO, name: "Beto", avatar: null, direct: false, presence: ONLINE }],
      }),
    );
    expect(calls("dm_retry")).toEqual([{ friend: BETO }]);
  });

  it("enquanto houver pendente para um amigo online sem conexão, tenta de novo de tempos em tempos", async () => {
    vi.useFakeTimers();
    try {
      offs.forEach((off) => off());
      offs = [await listenToDmEvents()];
      useDmStore.setState({
        conversations: [
          { friend: BETO, last: msg({ id: "1", mine: true, status: "pending" }), pending: 1 },
          { friend: "333333333333333333", last: msg({ id: "2", mine: true, status: "pending" }), pending: 1 },
        ],
        online: {},
      });
      vi.advanceTimersByTime(RETRY_EVERY_MS);
      // Só o Beto (online); o outro amigo não está na lista de online.
      expect(calls("dm_retry")).toEqual([{ friend: BETO }]);
      // Conectado: não tenta mais.
      useDmStore.setState({ online: { [BETO]: true } });
      vi.advanceTimersByTime(RETRY_EVERY_MS);
      expect(calls("dm_retry")).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("um download no meio pode ser cancelado", async () => {
    const chat = await openBeto();
    const zip = msg({
      id: "z",
      file: { name: "filme.mkv", size: 1000, mime: "video/x-matroska", path: null, done: 0 },
    });
    emit("dm-event", { type: "message", friend: BETO, message: zip });
    answers.dm_pick_save = "D:/filme.mkv";
    fireEvent.click(within(chat).getByRole("button", { name: /Baixar/ }));
    await waitFor(() => expect(calls("dm_download")).toHaveLength(1));
    emit("dm-event", { type: "message", friend: BETO, message: { ...zip, file: { ...zip.file!, done: 400 } } });
    fireEvent.click(within(chat).getByRole("button", { name: "Cancelar o download de filme.mkv" }));
    await waitFor(() => expect(calls("dm_cancel_download")).toEqual([{ friend: BETO, id: "z" }]));
    // Voltou a mostrar Baixar.
    emit("dm-event", { type: "message", friend: BETO, message: zip });
    expect(within(chat).getByRole("button", { name: /Baixar/ })).toBeInTheDocument();
  });

  it("arquivo que quem mandou apagou avisa e não oferece baixar", async () => {
    const chat = await openBeto();
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: msg({
        id: "g",
        file: { name: "jogo.zip", size: 2048, mime: "application/zip", path: null, done: 0, gone: true },
      }),
    });
    expect(within(chat).getByText(/Não está mais disponível/)).toBeInTheDocument();
    expect(within(chat).queryByRole("button", { name: /Baixar/ })).toBeNull();
  });

  it("mensagem nova toca o aviso e aparece no contador do ícone do SquadView", async () => {
    render(<App />);
    playCue.mockClear();
    emit("dm-event", { type: "message", friend: BETO, message: msg({ id: "n1", text: "oi" }) });
    expect(playCue).toHaveBeenCalledWith("message", expect.any(Number));
    const rail = screen.getByRole("navigation", { name: "Salas" });
    expect(within(rail).getByLabelText("1 novidade(s): pedidos de amizade e mensagens")).toBeInTheDocument();
    // As nossas e as repetidas (status mudando) não tocam.
    playCue.mockClear();
    emit("dm-event", { type: "message", friend: BETO, message: msg({ id: "n2", mine: true, text: "e aí" }) });
    emit("dm-event", { type: "message", friend: BETO, message: msg({ id: "n1", text: "oi" }) });
    expect(playCue).not.toHaveBeenCalled();
    // Com a conversa aberta e o app na frente, também não.
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    await act(() => useDmStore.getState().openChat(BETO));
    emit("dm-event", { type: "message", friend: BETO, message: msg({ id: "n3", text: "tá aí?" }) });
    expect(playCue).not.toHaveBeenCalled();
  });

  it("configurações: a escolha da cópia e as partes novas da limpeza", async () => {
    answers.storage_usage = [
      { id: "attachments", bytes: 2048 },
      { id: "conversations", bytes: 1024 },
    ];
    render(<SettingsDialog trigger={<button type="button">Configurações</button>} />);
    fireEvent.click(screen.getByRole("button", { name: "Configurações" }));
    const nav = screen.getByRole("navigation", { name: "Categorias" });
    fireEvent.click(within(nav).getByRole("button", { name: "Mensagens" }));
    const opcoes = screen.getByRole("radiogroup", { name: "Anexos que você manda" });
    expect(within(opcoes).getByRole("radio", { name: /Perguntar/ })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(opcoes).getByRole("radio", { name: /Só mandar/ }));
    expect(useDmStore.getState().copyChoice).toBe("send");

    fireEvent.click(within(nav).getByRole("button", { name: "Armazenamento" }));
    const partes = await screen.findByRole("list", { name: "Partes do app" });
    expect(within(partes).getByText("Anexos das conversas")).toBeInTheDocument();
    expect(within(partes).getByText("Histórico de conversas")).toBeInTheDocument();
  });

  it("sem armazenamento, a escolha vale até fechar o app", async () => {
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("bloqueado");
    });
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("bloqueado");
    });
    vi.resetModules();
    const { useDmStore: fresh } = await import("./dm-store");
    expect(fresh.getState().copyChoice).toBe("ask");
    fresh.getState().setCopyChoice("keep");
    expect(fresh.getState().copyChoice).toBe("keep");
    get.mockRestore();
    setItem.mockRestore();
  });

  it("escolha salva de antes vale ao abrir o app", async () => {
    localStorage.setItem("squadview.anexos.copia", "send");
    vi.resetModules();
    const { useDmStore: fresh } = await import("./dm-store");
    expect(fresh.getState().copyChoice).toBe("send");
  });
});

describe("reações, respostas e a hora das mensagens", () => {
  it("a hora fica como no Discord: só a hora hoje, Ontem às ontem, e data e hora antes", () => {
    const now = new Date(2026, 9, 22, 10, 30).getTime();
    expect(stamp(new Date(2026, 9, 22, 0, 0).getTime(), now)).toBe("00:00");
    expect(stamp(new Date(2026, 9, 21, 23, 59).getTime(), now)).toBe("Ontem às 23:59");
    expect(stamp(new Date(2026, 9, 19, 14, 45).getTime(), now)).toBe("19/10/2026 14:45");
    // Virada de mês: no dia 1º, o dia 31 é ontem.
    expect(stamp(new Date(2026, 9, 31, 22, 0).getTime(), new Date(2026, 10, 1, 0, 10).getTime())).toBe(
      "Ontem às 22:00",
    );
  });

  it("a barra da mensagem reage com um clique, pelo seletor e responde", async () => {
    const oi = msg({ id: "1", text: "bora jogar?", at: Date.now() - 60_000 });
    answers.dm_messages = [oi];
    answers.dm_react = { ...oi, reactions: [{ emoji: "👍", mine: true, theirs: false }] };
    const chat = await openBeto();
    const barra = await within(chat).findByRole("toolbar", { name: "Ações da mensagem de Beto" });
    fireEvent.click(within(barra).getByRole("button", { name: "Reagir com 👍" }));
    await waitFor(() => expect(calls("dm_react")).toEqual([{ friend: BETO, id: "1", emoji: "👍", on: true }]));
    // A reação aparece embaixo, marcada como nossa.
    const nossa = await within(chat).findByRole("button", { name: "👍 1" });
    expect(nossa).toHaveAttribute("aria-pressed", "true");
    // Clicar nela tira.
    answers.dm_react = { ...oi, reactions: [] };
    fireEvent.click(nossa);
    await waitFor(() => expect(calls("dm_react")[1]).toEqual({ friend: BETO, id: "1", emoji: "👍", on: false }));
    await waitFor(() => expect(within(chat).queryByRole("button", { name: "👍 1" })).toBeNull());

    // Mais reações, pelo seletor.
    fireEvent.pointerDown(within(barra).getByRole("button", { name: "Adicionar reação" }), {
      button: 0,
      ctrlKey: false,
    });
    const grade = await screen.findByRole("menu", { name: "Adicionar reação" });
    fireEvent.click(within(grade).getByRole("menuitem", { name: "Reagir com 🔥" }));
    await waitFor(() => expect(calls("dm_react")[2]).toEqual({ friend: BETO, id: "1", emoji: "🔥", on: true }));

    // Responder: a caixa mostra a quem, e a mensagem vai com a resposta.
    fireEvent.click(within(barra).getByRole("button", { name: "Responder" }));
    expect(await within(chat).findByText(/Respondendo a/)).toHaveTextContent("Respondendo a Beto");
    const caixa = within(chat).getByRole("textbox", { name: "Mensagem para Beto" });
    fireEvent.change(caixa, { target: { value: "bora!" } });
    fireEvent.keyDown(caixa, { key: "Enter" });
    await waitFor(() => expect(calls("dm_send_text")).toEqual([{ friend: BETO, text: "bora!", replyTo: "1" }]));
    await waitFor(() => expect(within(chat).queryByText(/Respondendo a/)).toBeNull());
  });

  it("a resposta pode ser cancelada pelo X ou pelo Esc", async () => {
    answers.dm_messages = [msg({ id: "1", text: "oi", mine: true, at: Date.now() - 60_000 })];
    const chat = await openBeto();
    const barra = await within(chat).findByRole("toolbar", { name: "Ações da mensagem de Kaique" });
    fireEvent.click(within(barra).getByRole("button", { name: "Responder" }));
    // A nossa própria mensagem: "Respondendo a" quem somos.
    expect(await within(chat).findByText(/Respondendo a/)).toHaveTextContent("Respondendo a Kaique");
    fireEvent.click(within(chat).getByRole("button", { name: "Cancelar resposta" }));
    expect(within(chat).queryByText(/Respondendo a/)).toBeNull();

    fireEvent.click(within(barra).getByRole("button", { name: "Responder" }));
    const caixa = within(chat).getByRole("textbox", { name: "Mensagem para Beto" });
    fireEvent.keyDown(caixa, { key: "Escape" });
    expect(within(chat).queryByText(/Respondendo a/)).toBeNull();
    // Esc sem resposta não faz nada (nem manda).
    fireEvent.keyDown(caixa, { key: "Escape" });
    expect(calls("dm_send_text")).toHaveLength(0);
  });

  it("a resposta mostra o que foi respondido e leva até lá; sem a original, avisa", async () => {
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    answers.dm_messages = [
      msg({ id: "1", text: "viu o clutch?", at: Date.now() - 120_000 }),
      msg({ id: "2", text: "vi!", mine: true, replyTo: "1", at: Date.now() - 60_000 }),
      msg({ id: "3", text: "e esse?", replyTo: "sumiu", at: Date.now() - 30_000 }),
      msg({
        id: "4",
        file: { name: "mapa.zip", size: 1, mime: "application/zip", path: null, done: 0 },
        at: Date.now() - 20_000,
      }),
      msg({ id: "5", text: "esse arquivo", replyTo: "4", at: Date.now() - 10_000 }),
    ];
    const chat = await openBeto();
    const [ir, irArquivo] = await within(chat).findAllByRole("button", { name: "Ir para a mensagem de Beto" });
    expect(ir).toHaveTextContent("@Betoviu o clutch?");
    fireEvent.click(ir);
    expect(scroll).toHaveBeenCalledWith({ block: "center", behavior: "smooth" });
    expect(within(chat).getByText("A mensagem original não está mais aqui")).toBeInTheDocument();
    // Resposta a um anexo mostra o nome do arquivo.
    expect(irArquivo).toHaveTextContent("📎 mapa.zip");
    delete (Element.prototype as Partial<Element>).scrollIntoView;
  });

  it("reações das duas pessoas somam, e a dica diz quem reagiu", async () => {
    answers.dm_messages = [
      msg({
        id: "1",
        text: "gg",
        at: Date.now() - 60_000,
        reactions: [
          { emoji: "🔥", mine: true, theirs: true },
          { emoji: "😂", mine: false, theirs: true },
          { emoji: "👀", mine: true, theirs: false },
        ],
      }),
    ];
    // A dica do Radix mede o tamanho do conteúdo.
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const chat = await openBeto();
    const fogo = await within(chat).findByRole("button", { name: "🔥 2" });
    expect(fogo).toHaveAttribute("aria-pressed", "true");
    const riso = within(chat).getByRole("button", { name: "😂 1" });
    expect(riso).toHaveAttribute("aria-pressed", "false");
    fireEvent.focus(fogo);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Você e Beto reagiram com 🔥");
    fireEvent.blur(fogo);
    fireEvent.focus(riso);
    expect(await screen.findByRole("tooltip", {}, { timeout: 2000 })).toHaveTextContent("Beto reagiu com 😂");
    vi.unstubAllGlobals();
  });

  it("reação numa mensagem antiga, com a conversa fechada, não conta como nova", () => {
    useDmStore.setState({
      conversations: [{ friend: BETO, last: msg({ id: "9", text: "última", at: 2_000 }), pending: 0 }],
    });
    emit("dm-event", {
      type: "message",
      friend: BETO,
      message: msg({ id: "1", text: "antiga", at: 1_000, reactions: [{ emoji: "🔥", mine: false, theirs: true }] }),
    });
    expect(useDmStore.getState().unread[BETO]).toBeUndefined();
    expect(playCue).not.toHaveBeenCalledWith("message", expect.anything());
  });

  it("falha ao reagir avisa, sem mudar nada", async () => {
    answers.dm_react = new Error("Amigo inválido.");
    await useDmStore.getState().react(BETO, msg({ id: "1", text: "oi" }), "👍");
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "warning", message: "Amigo inválido." });
  });
});
