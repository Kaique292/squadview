import * as RadixTooltip from "@radix-ui/react-tooltip";
import type { ComponentPropsWithRef, ReactNode } from "react";

type TooltipProps = {
  label: string;
  side?: "top" | "right" | "bottom" | "left";
  children: ReactNode;
} & Omit<ComponentPropsWithRef<"button">, "children">;

/**
 * Dica escura ao passar o mouse, como as do Discord (aparece rápido, com uma setinha). Repassa
 * props e ref ao filho, então pode ficar dentro de outro gatilho (ex.: `<Dialog.Trigger asChild>`).
 */
export function Tooltip({ label, side, children, ...rest }: TooltipProps) {
  return (
    <RadixTooltip.Provider delayDuration={150} skipDelayDuration={300}>
      <RadixTooltip.Root>
        <RadixTooltip.Trigger asChild {...rest}>
          {children}
        </RadixTooltip.Trigger>
        <RadixTooltip.Portal>
          <RadixTooltip.Content
            side={side ?? "top"}
            sideOffset={8}
            className="z-50 rounded-md bg-surface-float px-3 py-1.5 text-sm font-semibold text-text shadow-xl shadow-black/40 data-[state=closed]:animate-overlay-out data-[state=delayed-open]:animate-tooltip-in"
          >
            {label}
            <RadixTooltip.Arrow className="fill-surface-float" />
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}
