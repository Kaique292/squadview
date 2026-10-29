import { controllerStatus } from "./controllers";

describe("estado dos controles de quem joga", () => {
  it("ainda conferindo: não diz nada", () => {
    expect(controllerStatus(null)).toBeNull();
  });

  // Regressão: quem jogava com um controle que o app não lia não recebia aviso nenhum.
  it("sem controle, avisa e sugere o teclado", () => {
    expect(controllerStatus([])).toEqual({
      ok: false,
      text: "Nenhum controle encontrado: ligue um ou jogue com o teclado",
    });
  });

  it("diz a família do controle achado", () => {
    expect(controllerStatus([{ name: "DualSense Wireless Controller", kind: "playstation", supported: true }])).toEqual(
      { ok: true, text: "Controle de PlayStation conectado" },
    );
    expect(controllerStatus([{ name: "Controle de Xbox", kind: "xbox", supported: true }])?.text).toBe(
      "Controle de Xbox conectado",
    );
  });

  it("controle genérico aparece pelo nome do Windows", () => {
    expect(controllerStatus([{ name: "8BitDo Pro 2", kind: "other", supported: true }])?.text).toBe(
      "8BitDo Pro 2 conectado",
    );
  });

  it("conta vários e ignora os sem mapa quando há um que serve", () => {
    expect(
      controllerStatus([
        { name: "Controle de Xbox", kind: "xbox", supported: true },
        { name: "Joystick velho", kind: "other", supported: false },
      ]),
    ).toEqual({ ok: true, text: "Controle de Xbox conectado" });
    expect(
      controllerStatus([
        { name: "Controle de Xbox", kind: "xbox", supported: true },
        { name: "Pro Controller", kind: "nintendo", supported: true },
      ])?.text,
    ).toBe("2 controles conectados");
  });

  it("só controle sem mapa: avisa que ele não serve", () => {
    expect(controllerStatus([{ name: "Joystick velho", kind: "other", supported: false }])).toEqual({
      ok: false,
      text: "Joystick velho sem mapa de botões: jogue com o teclado ou outro controle",
    });
  });
});
