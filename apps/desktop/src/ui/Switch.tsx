interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  /** Texto menor embaixo do rótulo. */
  hint?: string;
  disabled?: boolean;
}

/** Chave liga/desliga no estilo do Discord: trilho arredondado, bolinha que desliza. */
export function Switch(props: SwitchProps) {
  return (
    <label className={`flex items-center gap-3 ${props.disabled ? "opacity-50" : "cursor-pointer"}`}>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium">{props.label}</span>
        {props.hint && <span className="block text-xs text-text-faint">{props.hint}</span>}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={props.checked}
        aria-label={props.label}
        disabled={props.disabled}
        onClick={() => props.onChange(!props.checked)}
        className={`relative h-6 w-10 shrink-0 rounded-full transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none ${
          props.checked ? "bg-online" : "bg-surface-3"
        }`}
      >
        <span
          className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform duration-200 ease-out ${
            props.checked ? "translate-x-4" : ""
          }`}
        />
      </button>
    </label>
  );
}
