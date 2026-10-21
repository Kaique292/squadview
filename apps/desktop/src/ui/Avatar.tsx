import type { SpeakingEffect } from "../voice/speaking-effect";
import { VoiceAura } from "./VoiceAura";

const COLORS = ["#f0b232", "#23a55a", "#3ba0ff", "#e06c9f", "#6e4ff6", "#f23f43"];

/** Cor fixa por nome (a mesma em todo lugar: lateral, grade da sala, painel). */
export function avatarColor(name: string): string {
  const hash = [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  return COLORS[hash % COLORS.length];
}

/**
 * Avatar redondo: a foto (do Discord) ou a inicial na cor do nome. Quando a pessoa está falando,
 * o efeito de fala que ela escolheu (ou o anel verde padrão); anel vermelho quando está ao vivo, e
 * o ponto de status no canto.
 */
export function Avatar(props: {
  name: string;
  size: number;
  live?: boolean;
  speaking?: boolean;
  /** Efeito de fala escolhido pela pessoa (`null`: o anel verde padrão). */
  effect?: SpeakingEffect | null;
  status?: "online" | "offline";
  /** Cor da borda do ponto de status (o fundo onde o avatar está). */
  statusBorder?: string;
  src?: string;
}) {
  const ring =
    props.speaking && !props.effect
      ? "ring-2 ring-online ring-offset-2 ring-offset-transparent"
      : props.live
        ? "ring-2 ring-live"
        : "";
  return (
    // Sem `isolate`: o efeito de quem está do lado precisa enxergar este para as cores se
    // misturarem onde eles se tocam. A foto fica por cima do efeito pela ordem (os dois posicionados).
    <span aria-hidden className="relative shrink-0">
      {props.effect && <VoiceAura effect={props.effect} size={props.size} active={!!props.speaking} />}
      {props.src ? (
        <img
          src={props.src}
          alt=""
          width={props.size}
          height={props.size}
          className={`relative rounded-full object-cover transition-shadow duration-150 ${ring}`}
          style={{ width: props.size, height: props.size }}
        />
      ) : (
        <span
          className={`relative grid place-items-center rounded-full font-semibold text-white transition-shadow duration-150 ${ring}`}
          style={{
            width: props.size,
            height: props.size,
            fontSize: props.size * 0.42,
            background: avatarColor(props.name),
          }}
        >
          {props.name.slice(0, 1).toUpperCase()}
        </span>
      )}
      {props.status && (
        <span
          className={`absolute -right-0.5 -bottom-0.5 size-3.5 rounded-full border-[3px] ${
            props.status === "online" ? "bg-online" : "bg-text-faint"
          } ${props.statusBorder ?? "border-surface-panel"}`}
        />
      )}
    </span>
  );
}
