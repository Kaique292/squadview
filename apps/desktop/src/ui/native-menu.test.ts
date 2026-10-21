import { blockNativeContextMenu } from "./native-menu";

function rightClick(target: Element) {
  const e = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
  target.dispatchEvent(e);
  return e.defaultPrevented;
}

describe("botão direito", () => {
  it("o menu do navegador não abre fora das caixas de texto; nelas, abre (Copiar e Colar)", () => {
    document.body.innerHTML =
      '<div id="sala"></div><input id="nome" /><div contenteditable="true"><b id="rico"></b></div>';
    const off = blockNativeContextMenu();
    expect(rightClick(document.getElementById("sala")!)).toBe(true);
    expect(rightClick(document.getElementById("nome")!)).toBe(false);
    expect(rightClick(document.getElementById("rico")!)).toBe(false);
    off();
    expect(rightClick(document.getElementById("sala")!)).toBe(false);
  });
});
