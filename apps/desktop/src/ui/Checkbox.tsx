import { type ReactNode, useState } from "react";

/**
 * A caixinha marcada do app: ao marcar, ela enche na cor da marca com um "pulo" e o ✓ se desenha;
 * ao desmarcar, o ✓ se apaga e a caixa esvazia encolhendo de leve. Só a caixa, sem rótulo: use
 * sozinha dentro dos `CheckboxItem` dos menus (Radix), ou pelo `Checkbox` em formulários.
 */
export function CheckMark(props: { checked: boolean; className?: string }) {
  // Sem animação na primeira vez que aparece: só quando o valor muda.
  const [initial] = useState(props.checked);
  const [changed, setChanged] = useState(false);
  if (!changed && props.checked !== initial) setChanged(true);
  return (
    <span
      aria-hidden
      data-checked={props.checked || undefined}
      data-animate={changed || undefined}
      className={`check-mark ${props.className ?? ""}`}
    >
      <svg viewBox="0 0 16 16">
        <path d="M3.5 8.5l3 3 6-7" pathLength={1} />
      </svg>
    </span>
  );
}

/** Checkbox com rótulo, para formulários e modais. A caixa nativa fica escondida, mas é ela que o teclado e o leitor de tela usam. */
export function Checkbox(props: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  children: ReactNode;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <label
      className={`flex items-center gap-2 ${props.disabled ? "opacity-50" : "cursor-pointer"} ${props.className ?? ""}`}
    >
      <input
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onChange={(e) => props.onChange(e.target.checked)}
        className="peer sr-only"
      />
      <CheckMark
        checked={props.checked}
        className="peer-focus-visible:ring-2 peer-focus-visible:ring-brand peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-surface-2"
      />
      <span className="min-w-0">{props.children}</span>
    </label>
  );
}
