import { act, fireEvent, render, screen } from "@testing-library/react";
import { Toaster } from "./Toaster";
import { TOAST_EXIT_MS, toast, useToastStore } from "./toast-store";

beforeEach(() => {
  vi.useFakeTimers();
  useToastStore.setState({ toasts: [] });
});
afterEach(() => vi.useRealTimers());

describe("notificações", () => {
  it("cada tipo tem seu estilo e os urgentes são anunciados como alerta", () => {
    render(<Toaster />);
    act(() => {
      toast.info("Mensagem do sistema");
      toast.success("Deu certo");
      toast.warning("Cuidado");
      toast.alert("Atenção agora");
      toast.error("Deu errado");
    });
    expect(screen.getByRole("status", { name: "Aviso do sistema" })).toHaveTextContent("Mensagem do sistema");
    expect(screen.getByRole("status", { name: "Sucesso" })).toHaveTextContent("Deu certo");
    expect(screen.getByRole("status", { name: "Aviso" })).toHaveTextContent("Cuidado");
    expect(screen.getByRole("alert", { name: "Alerta" })).toHaveTextContent("Atenção agora");
    expect(screen.getByRole("alert", { name: "Erro" })).toHaveTextContent("Deu errado");
  });

  it("some sozinha no fim do tempo, com animação de saída", () => {
    render(<Toaster />);
    act(() => void toast.success("Entrou"));
    act(() => vi.advanceTimersByTime(3999));
    expect(useToastStore.getState().toasts[0].leaving).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(useToastStore.getState().toasts[0].leaving).toBe(true);
    act(() => vi.advanceTimersByTime(TOAST_EXIT_MS));
    expect(screen.queryByText("Entrou")).not.toBeInTheDocument();
  });

  it("pausa o tempo com o mouse em cima e retoma de onde parou", () => {
    render(<Toaster />);
    act(() => void toast.info("Fica"));
    act(() => vi.advanceTimersByTime(3000));
    fireEvent.mouseEnter(screen.getByRole("region", { name: "Notificações" }));
    act(() => vi.advanceTimersByTime(10_000));
    expect(useToastStore.getState().toasts[0].leaving).toBe(false);
    fireEvent.mouseLeave(screen.getByRole("region", { name: "Notificações" }));
    act(() => vi.advanceTimersByTime(1000));
    expect(useToastStore.getState().toasts[0].leaving).toBe(true);
  });

  it("empilha a mais nova na frente e abre a pilha com o mouse", () => {
    render(<Toaster />);
    act(() => {
      toast.info("Primeira");
      toast.info("Segunda");
    });
    const [front, back] = screen.getAllByRole("status");
    expect(front).toHaveTextContent("Segunda");
    expect(back.style.transform).toContain("scale(0.96)");
    fireEvent.mouseEnter(screen.getByRole("region", { name: "Notificações" }));
    expect(back.style.transform).not.toContain("scale");
  });

  it("fecha pelo botão, não repete a mesma mensagem e guarda no máximo 5", () => {
    render(<Toaster />);
    act(() => {
      toast.error("Mesmo erro");
      toast.error("Mesmo erro");
    });
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Fechar notificação" }));
    act(() => vi.advanceTimersByTime(TOAST_EXIT_MS));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    act(() => {
      for (let i = 0; i < 7; i++) toast.info(`n${i}`);
    });
    expect(useToastStore.getState().toasts.filter((t) => !t.leaving)).toHaveLength(5);
  });

  it("notificação sem tempo fica até fechar", () => {
    render(<Toaster />);
    act(() => void useToastStore.getState().push({ kind: "alert", message: "Fixa", duration: 0 }));
    act(() => vi.advanceTimersByTime(60_000));
    expect(screen.getByText("Fixa")).toBeInTheDocument();
    expect(screen.queryByTestId("tempo-da-notificacao")).not.toBeInTheDocument();
  });

  it("fica sobre a barra lateral quando há vídeo na tela", () => {
    render(<Toaster anchor="sidebar" />);
    expect(screen.getByRole("region", { name: "Notificações" }).className).toContain("left-2");
  });
});
