import { Gamepad2 } from "lucide-react";
import type { HTMLAttributes } from "react";

/**
 * O selo "AO VIVO" do app: pílula vermelha, em caixa alta, como o do Discord. O texto continua
 * "Ao vivo" (o leitor de tela lê normal); só aparece em maiúsculas.
 */
export function LiveBadge({ size, className, ...rest }: { size?: "sm" | "md" } & HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      {...rest}
      className={`inline-flex shrink-0 items-center rounded-full bg-live font-bold tracking-wide text-white uppercase ${
        size === "sm" ? "px-1.5 py-px text-[9px]" : "px-2 py-0.5 text-[11px]"
      } ${className ?? ""}`}
    >
      Ao vivo
    </span>
  );
}

/** Selo escuro ao lado do "AO VIVO" (a qualidade da live, como "720P 30FPS"). */
export function InfoBadge({ children, className, ...rest }: { children: string } & HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      {...rest}
      className={`inline-flex shrink-0 items-center rounded-full bg-black/60 px-2 py-0.5 text-[11px] font-bold tracking-wide text-white uppercase ${className ?? ""}`}
    >
      {children}
    </span>
  );
}

/**
 * O selo do fliperama aberto (ADR 0023): roxo da marca, com o controle e quantos jogam. Para todos
 * da sala saberem que dá para entrar como jogador. `compact`: só o controle e a conta; `iconOnly`:
 * só o controle (na lista da sala, ao lado do AO VIVO, não cabe mais nada; a conta vai na dica).
 */
export function ArcadeBadge({
  arcade,
  compact,
  iconOnly,
  size,
  className,
  ...rest
}: {
  arcade: { slots: number; players: string[] };
  compact?: boolean;
  iconOnly?: boolean;
  size?: "sm" | "md";
} & HTMLAttributes<HTMLSpanElement>) {
  const count = `${arcade.players.length}/${arcade.slots}`;
  return (
    <span
      {...rest}
      role="img"
      aria-label={`Fliperama aberto: ${arcade.players.length} de ${arcade.slots} ${arcade.slots === 1 ? "vaga ocupada" : "vagas ocupadas"}`}
      title={iconOnly ? `Fliperama: ${count}` : undefined}
      className={`inline-flex shrink-0 items-center gap-1 rounded-full bg-brand font-bold tracking-wide text-white uppercase ${
        size === "sm" ? "px-1.5 py-px text-[9px]" : "px-2 py-0.5 text-[11px]"
      } ${className ?? ""}`}
    >
      <Gamepad2 aria-hidden size={size === "sm" ? 10 : 12} />
      {iconOnly ? null : compact ? count : `Fliperama ${count}`}
    </span>
  );
}
