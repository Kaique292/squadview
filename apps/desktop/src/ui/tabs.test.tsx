import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { TAB_OUT_MS, TabContent, Tabs } from "./Tabs";

const TABS = [
  ["a", "Primeira"],
  ["b", "Segunda"],
  ["c", "Terceira"],
] as const;
type Id = (typeof TABS)[number][0];

function Demo() {
  const [tab, setTab] = useState<Id>("a");
  return (
    <>
      <Tabs label="Partes" tabs={TABS} value={tab} onChange={setTab} />
      <TabContent value={tab} order={TABS.map(([id]) => id)}>
        {(t) => <p>Conteúdo {t}</p>}
      </TabContent>
    </>
  );
}

// No jsdom nada tem posição: cada aba "fica" 100 px depois da anterior, com 80 px de largura.
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetLeft", {
    configurable: true,
    get(this: HTMLElement) {
      const tabs = [...(this.parentElement?.querySelectorAll('[role="tab"]') ?? [])];
      return Math.max(0, tabs.indexOf(this)) * 100;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 80 });
});
afterAll(() => {
  // As medidas de mentira valem só aqui.
  Reflect.deleteProperty(HTMLElement.prototype, "offsetLeft");
  Reflect.deleteProperty(HTMLElement.prototype, "offsetWidth");
});
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

const bar = () => document.querySelector<HTMLElement>("[data-tab-bar]")!;
const panel = () => screen.getByRole("tabpanel");

describe("abas do app", () => {
  it("a barra fica embaixo da aba escolhida e desliza até a nova", () => {
    render(<Demo />);
    expect(bar().style.transform).toBe("translateX(0px)");
    expect(bar().style.width).toBe("80px");
    // A primeira posição aparece direto; depois, desliza.
    act(() => vi.advanceTimersByTime(20));
    fireEvent.click(screen.getByRole("tab", { name: "Terceira" }));
    expect(screen.getByRole("tab", { name: "Terceira" })).toHaveAttribute("aria-selected", "true");
    expect(bar().style.transform).toBe("translateX(200px)");
  });

  it("o conteúdo da aba que sai some antes do novo entrar, pelo lado para onde você foi", () => {
    render(<Demo />);
    expect(panel()).toHaveTextContent("Conteúdo a");

    fireEvent.click(screen.getByRole("tab", { name: "Segunda" }));
    // Primeiro a antiga sai (para a esquerda, porque a nova está à direita)...
    expect(panel()).toHaveTextContent("Conteúdo a");
    expect(panel()).toHaveAttribute("data-leaving");
    expect(panel().className).toContain("animate-tab-out");
    expect(panel().style.getPropertyValue("--tab-direction")).toBe("1");
    // ...depois a nova entra.
    act(() => vi.advanceTimersByTime(TAB_OUT_MS));
    expect(panel()).toHaveTextContent("Conteúdo b");
    expect(panel()).not.toHaveAttribute("data-leaving");
    expect(panel().className).toContain("animate-tab-in");

    // Voltando para uma aba à esquerda, o lado inverte.
    fireEvent.click(screen.getByRole("tab", { name: "Primeira" }));
    expect(panel().style.getPropertyValue("--tab-direction")).toBe("-1");
    act(() => vi.advanceTimersByTime(TAB_OUT_MS));
    expect(panel()).toHaveTextContent("Conteúdo a");
  });

  it("trocando de novo no meio da saída, entra a última escolhida", () => {
    render(<Demo />);
    fireEvent.click(screen.getByRole("tab", { name: "Segunda" }));
    act(() => vi.advanceTimersByTime(TAB_OUT_MS / 2));
    fireEvent.click(screen.getByRole("tab", { name: "Terceira" }));
    act(() => vi.advanceTimersByTime(TAB_OUT_MS));
    expect(panel()).toHaveTextContent("Conteúdo c");
  });
});
