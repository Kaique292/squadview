import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useAccountStore } from "../account/account-store";
import { useFriendsStore } from "../friends/friends-store";
import { ServerSettingsDialog } from "./ServerSettingsDialog";
import type { Server } from "./servers-store";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const ME = "999999999999999999";
const ID = "0000000000000001";
const ICONE = "data:image/webp;base64,UklGRg==";

function server(icon: string | null = null): Server {
  return {
    id: ID,
    name: "Turma",
    icon,
    rooms: [{ id: "00000001", name: "Geral", code: "GERAL2" }],
    members: [{ id: ME, name: "Kaique", avatar: null, role: "owner" }],
  };
}

/** O jsdom não carrega imagens: esta "carrega" (ou falha) logo depois de receber o endereço. */
let imageFails = false;
class FakeImage {
  naturalWidth = 800;
  naturalHeight = 500;
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  src = "";
  constructor() {
    queueMicrotask(() => (imageFails ? this.onerror?.() : this.onload?.()));
  }
}

const drawImage = vi.fn();

beforeEach(() => {
  imageFails = false;
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  drawImage.mockClear();
  useAccountStore.setState({ profile: { id: ME, username: "k", globalName: "Kaique", avatar: null } });
  useFriendsStore.setState({ status: "online" });
  vi.stubGlobal("Image", FakeImage);
  URL.createObjectURL = vi.fn(() => "blob:foto");
  URL.revokeObjectURL = vi.fn();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    drawImage,
    imageSmoothingQuality: "low",
  } as never);
  vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockImplementation((type?: string) => `data:${type};base64,ok`);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function actions() {
  return invoke.mock.calls.filter(([cmd]) => cmd === "friends_action").map(([, args]) => args.action);
}

function choose(file: File) {
  fireEvent.change(screen.getByLabelText("Imagem do ícone"), { target: { files: [file] } });
}

const foto = (type = "image/png", size = 1000) => {
  const f = new File(["x"], "foto.png", { type });
  Object.defineProperty(f, "size", { value: size });
  return f;
};

describe("ícone do servidor", () => {
  it("escolher uma imagem abre o recorte, e aplicar salva o ícone de 96 px no servidor", async () => {
    render(<ServerSettingsDialog server={server()} open onOpenChange={() => {}} />);
    expect(screen.getByRole("button", { name: /Enviar ícone/ })).toBeInTheDocument();
    choose(foto());
    const recorte = await screen.findByRole("dialog", { name: "Ajustar o ícone" });
    const janela = within(recorte).getByRole("slider", { name: "Posição do ícone" });

    // Zoom pelo controle, pela roda e pelo teclado; arrastar e as setas movem.
    fireEvent.change(within(recorte).getByRole("slider", { name: "Zoom" }), { target: { value: "2" } });
    fireEvent.wheel(janela, { deltaY: -100, clientX: 10, clientY: 10 });
    fireEvent.wheel(janela, { deltaY: 100, clientX: 10, clientY: 10 });
    fireEvent.keyDown(janela, { key: "+" });
    fireEvent.keyDown(janela, { key: "-" });
    fireEvent.keyDown(janela, { key: "ArrowLeft", shiftKey: true });
    fireEvent.keyDown(janela, { key: "ArrowUp" });
    fireEvent.keyDown(janela, { key: "a" });
    fireEvent.pointerDown(janela, { clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(janela, { clientX: 60, clientY: 90, pointerId: 1 });
    fireEvent.pointerUp(janela, { pointerId: 1 });
    fireEvent.pointerMove(janela, { clientX: 0, clientY: 0, pointerId: 1 });
    expect(janela).toHaveAttribute("aria-valuetext", "zoom 2.0×");
    fireEvent.click(within(recorte).getByRole("button", { name: "Recomeçar" }));
    expect(janela).toHaveAttribute("aria-valuetext", "zoom 1.0×");

    fireEvent.click(within(recorte).getByRole("button", { name: "Aplicar" }));
    expect(drawImage).toHaveBeenCalledWith(
      expect.anything(),
      expect.closeTo(150),
      expect.closeTo(0),
      expect.closeTo(500),
      expect.closeTo(500),
      0,
      0,
      96,
      96,
    );
    await waitFor(() =>
      expect(actions()).toContainEqual({
        type: "server-action",
        server: ID,
        op: { kind: "update", icon: "data:image/webp;base64,ok" },
      }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Ajustar o ícone" })).not.toBeInTheDocument());
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:foto");
  });

  it("sem conseguir preparar a imagem, avisa e não salva", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    render(<ServerSettingsDialog server={server()} open onOpenChange={() => {}} />);
    choose(foto());
    const recorte = await screen.findByRole("dialog", { name: "Ajustar o ícone" });
    fireEvent.click(within(recorte).getByRole("button", { name: "Aplicar" }));
    expect(within(recorte).getByRole("alert")).toHaveTextContent("Não deu para preparar essa imagem");
    expect(actions()).toEqual([]);
  });

  it("recusa o que não é imagem, imagem grande demais e imagem quebrada", async () => {
    render(<ServerSettingsDialog server={server()} open onOpenChange={() => {}} />);
    choose(foto("application/pdf"));
    expect(screen.getByRole("alert")).toHaveTextContent("PNG, JPG, WebP ou GIF");
    choose(foto("image/png", 9 * 1024 * 1024));
    expect(screen.getByRole("alert")).toHaveTextContent("grande demais");
    imageFails = true;
    choose(foto());
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Não deu para abrir"));
    expect(URL.revokeObjectURL).toHaveBeenCalled();
    // Cancelar a escolha não muda nada.
    fireEvent.change(screen.getByLabelText("Imagem do ícone"), { target: { files: [] } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("com ícone, dá para trocar (clicando nele) ou remover", async () => {
    const click = vi.spyOn(HTMLInputElement.prototype, "click");
    render(<ServerSettingsDialog server={server(ICONE)} open onOpenChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Trocar o ícone do servidor" }));
    fireEvent.click(screen.getByRole("button", { name: /Trocar ícone/ }));
    expect(click).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Remover ícone" }));
    await waitFor(() =>
      expect(actions()).toContainEqual({ type: "server-action", server: ID, op: { kind: "update", icon: null } }),
    );
  });

  it("fechar o recorte sem aplicar não muda o ícone", async () => {
    render(<ServerSettingsDialog server={server()} open onOpenChange={() => {}} />);
    choose(foto());
    const recorte = await screen.findByRole("dialog", { name: "Ajustar o ícone" });
    act(() => fireEvent.click(within(recorte).getByRole("button", { name: "Cancelar" })));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Ajustar o ícone" })).not.toBeInTheDocument());
    expect(actions()).toEqual([]);
  });
});
