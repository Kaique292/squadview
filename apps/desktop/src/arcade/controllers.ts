/** Espelha `sv_input::PadDevice`: um controle achado no PC de quem joga. */
export interface PadDevice {
  name: string;
  kind: "xbox" | "playstation" | "nintendo" | "other";
  /** O app sabe quais são os botões dele (sem isso, ele fica de fora). */
  supported: boolean;
}

const KIND_NAMES: Record<PadDevice["kind"], string> = {
  xbox: "de Xbox",
  playstation: "de PlayStation",
  nintendo: "da Nintendo",
  other: "",
};

/** "Controle de PlayStation" (ou o nome que o Windows dá, para os genéricos). */
function describe(device: PadDevice): string {
  const kind = KIND_NAMES[device.kind];
  return kind ? `Controle ${kind}` : device.name || "Controle";
}

/**
 * O que dizer a quem joga sobre os controles do PC dele. Antes não dizia nada: quem tinha um
 * controle que o app não lia achava que ele "não conectava". `null`: ainda conferindo.
 */
export function controllerStatus(devices: PadDevice[] | null): { ok: boolean; text: string } | null {
  if (devices === null) return null;
  const usable = devices.filter((d) => d.supported);
  if (usable.length === 1) return { ok: true, text: `${describe(usable[0])} conectado` };
  if (usable.length > 1) return { ok: true, text: `${usable.length} controles conectados` };
  const unknown = devices.find((d) => !d.supported);
  if (unknown) {
    return { ok: false, text: `${describe(unknown)} sem mapa de botões: jogue com o teclado ou outro controle` };
  }
  return { ok: false, text: "Nenhum controle encontrado: ligue um ou jogue com o teclado" };
}
