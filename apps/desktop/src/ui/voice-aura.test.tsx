import { render } from "@testing-library/react";
import { SPEAKING_EFFECTS } from "../voice/speaking-effect";
import { Avatar } from "./Avatar";

describe("efeito de fala (desenho)", () => {
  it.each(SPEAKING_EFFECTS)("%s só anima camadas inteiras, nunca o que está dentro de um SVG", (kind) => {
    // Animar algo dentro do SVG faz o Chromium redesenhar a cada quadro (custava até 40% da
    // thread da interface com a sala cheia falando); camadas inteiras ficam na GPU.
    const { container } = render(<Avatar name="Ana" size={80} speaking effect={{ kind, color: "#3ef0b0" }} />);
    const aura = container.querySelector(`[data-effect=${kind}]`)!;
    expect(aura).toHaveAttribute("data-active");
    const animated = aura.querySelectorAll(".aura-pulse, .aura-spin, .aura-spin-reverse, .aura-breathe");
    expect(animated.length).toBeGreaterThan(1);
    for (const el of animated) {
      expect(el.parentElement?.closest("svg")).toBeNull();
      expect(el.tagName === "svg" || el.classList.contains("aura-pulse")).toBe(true);
    }
    for (const svg of aura.querySelectorAll("svg")) {
      expect(svg.querySelector("[class]")).toBeNull();
      expect(svg.querySelector("[style]")).toBeNull();
    }
  });

  it("sem falar, o efeito fica montado mas apagado (e a animação pausada pelo CSS)", () => {
    const { container } = render(<Avatar name="Ana" size={24} effect={{ kind: "dots", color: "#3ef0b0" }} />);
    expect(container.querySelector("[data-effect=dots]")).not.toHaveAttribute("data-active");
    // Sem efeito, falando: o anel verde padrão, sem desenho extra.
    const { container: padrao } = render(<Avatar name="Ana" size={24} speaking />);
    expect(padrao.querySelector("[data-effect]")).toBeNull();
    expect(padrao.querySelector(".ring-online")).not.toBeNull();
  });
});
