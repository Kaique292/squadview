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
