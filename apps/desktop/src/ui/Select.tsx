import * as RadixSelect from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import type { ReactNode } from "react";
import { useOverlayMenu } from "./overlay";

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * Lista suspensa no visual do app (como as do Discord), no lugar da `<select>` do navegador:
 * ícone opcional, texto cortado com reticências, setinha que gira e menu que abre animado.
 */
export function Select(props: {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  icon?: ReactNode;
}) {
  const onOpenChange = useOverlayMenu();
  const current = props.options.find((o) => o.value === props.value);
  return (
    <RadixSelect.Root value={props.value} onValueChange={props.onChange} onOpenChange={onOpenChange}>
      <RadixSelect.Trigger
        aria-label={props.label}
        className="group flex h-10 w-full min-w-0 items-center gap-2 rounded-md border border-transparent bg-surface-0 px-3 text-left text-sm text-text transition-colors outline-none hover:border-black/40 focus-visible:ring-2 focus-visible:ring-brand data-[state=open]:border-brand"
      >
        {props.icon && <span className="shrink-0 text-text-muted">{props.icon}</span>}
        <span className="min-w-0 flex-1 truncate">
          <RadixSelect.Value>{current?.label}</RadixSelect.Value>
        </span>
        <RadixSelect.Icon asChild>
          <ChevronDown
            size={16}
            className="shrink-0 text-text-muted transition-transform duration-200 group-data-[state=open]:rotate-180"
          />
        </RadixSelect.Icon>
      </RadixSelect.Trigger>
      <RadixSelect.Portal>
        <RadixSelect.Content
          position="popper"
          sideOffset={6}
          className="z-50 max-h-72 w-[var(--radix-select-trigger-width)] overflow-hidden rounded-md border border-black/40 bg-surface-float p-1 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
        >
          <RadixSelect.Viewport>
            {props.options.map((o) => (
              <RadixSelect.Item
                key={o.value}
                value={o.value}
                className="flex cursor-pointer items-center justify-between gap-2 rounded px-2 py-2 text-sm text-text-muted outline-none select-none data-[highlighted]:bg-brand data-[highlighted]:text-white data-[state=checked]:text-text"
              >
                <RadixSelect.ItemText>
                  <span className="block truncate">{o.label}</span>
                </RadixSelect.ItemText>
                <RadixSelect.ItemIndicator>
                  <Check size={14} />
                </RadixSelect.ItemIndicator>
              </RadixSelect.Item>
            ))}
          </RadixSelect.Viewport>
        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  );
}
