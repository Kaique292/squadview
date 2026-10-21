import { Check } from "lucide-react";
import { type ReactNode, useState } from "react";
import { avatarUrl, displayName, useAccountStore } from "../account/account-store";
import { Avatar } from "../ui/Avatar";
import { ColorPicker } from "../ui/ColorPicker";
import {
  EFFECT_COLORS,
  EFFECT_LABELS,
  SPEAKING_EFFECTS,
  type SpeakingEffect,
  type SpeakingEffectKind,
  useSpeakingEffectStore,
} from "../voice/speaking-effect";

/**
 * Efeito de fala: o que aparece em volta da sua foto quando você fala, para toda a sala (ADR 0015).
 * A escolha fica na prévia até "Aplicar"; "Remover efeito" volta ao anel verde padrão.
 */
export function SpeakingEffectSection() {
  const applied = useSpeakingEffectStore((s) => s.effect);
  const setEffect = useSpeakingEffectStore((s) => s.setEffect);
  const profile = useAccountStore((s) => s.profile);
  const name = profile ? displayName(profile) : "Você";
  const photo = profile ? avatarUrl(profile) : undefined;

  const [kind, setKind] = useState<SpeakingEffectKind | null>(applied?.kind ?? null);
  const [color, setColor] = useState(applied?.color ?? EFFECT_COLORS[0]);
  const draft: SpeakingEffect | null = kind ? { kind, color } : null;
  const unchanged = draft?.kind === applied?.kind && (!draft || draft.color === applied?.color);
  const custom = !EFFECT_COLORS.includes(color);
  const [pickerOpen, setPickerOpen] = useState(false);
  // Sem efeito não há cor para escolher: o seletor fecha junto.
  const open = pickerOpen && kind !== null;

  return (
    <>
      <h3 id="voz-efeito" className="mt-5 scroll-mt-4 text-xs font-semibold tracking-wide text-text-faint uppercase">
        Efeito de fala
      </h3>
      <p className="-mt-1 text-xs text-text-faint">
        Aparece em volta da sua foto quando você fala, para todos da sala. Sem efeito, fica o anel verde.
      </p>

      <div
        role="img"
        aria-label={`Prévia: ${draft ? EFFECT_LABELS[draft.kind] : "anel padrão"}`}
        className="relative grid h-44 place-items-center overflow-hidden rounded-lg bg-surface-0"
      >
        <Avatar name={name} size={76} speaking effect={draft} src={photo} />
        <span className="absolute top-2 left-3 text-xs font-medium text-text-faint">Prévia</span>
      </div>

      <div role="radiogroup" aria-label="Efeito" className="grid grid-cols-5 gap-2">
        <EffectOption label="Padrão" selected={kind === null} onSelect={() => setKind(null)}>
          <Avatar name={name} size={30} speaking src={photo} />
        </EffectOption>
        {SPEAKING_EFFECTS.map((k) => (
          <EffectOption key={k} label={EFFECT_LABELS[k]} selected={kind === k} onSelect={() => setKind(k)}>
            <Avatar name={name} size={30} speaking effect={{ kind: k, color }} src={photo} />
          </EffectOption>
        ))}
      </div>

      <div className={`flex flex-col gap-2 transition-opacity ${kind ? "" : "pointer-events-none opacity-40"}`}>
        <span className="text-sm font-medium">Cor</span>
        <div role="radiogroup" aria-label="Cor" aria-disabled={!kind} className="flex flex-wrap items-center gap-2">
          {EFFECT_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              role="radio"
              aria-checked={color === c}
              aria-label={`Cor ${c}`}
              disabled={!kind}
              onClick={() => setColor(c)}
              className={`grid size-8 place-items-center rounded-full ring-offset-2 ring-offset-surface-2 transition-shadow ${
                color === c ? "ring-2 ring-text" : "hover:ring-2 hover:ring-text-faint"
              }`}
              style={{ background: c }}
            >
              {color === c && <Check size={16} className="text-black/70" />}
            </button>
          ))}
          <button
            type="button"
            aria-label="Outra cor"
            aria-expanded={pickerOpen}
            aria-controls="outra-cor"
            disabled={!kind}
            onClick={() => setPickerOpen((v) => !v)}
            className={`grid size-8 place-items-center rounded-full ring-offset-2 ring-offset-surface-2 transition-shadow ${
              custom || pickerOpen ? "ring-2 ring-text" : "hover:ring-2 hover:ring-text-faint"
            }`}
            style={{
              background: custom
                ? color
                : "conic-gradient(#ff5a4f, #f5e27a, #3ef0b0, #3ba0ff, #7c3aed, #ff6ac1, #ff5a4f)",
            }}
          >
            {custom && <Check size={16} className="text-black/70" />}
          </button>
        </div>
        {/* O seletor abre embaixo das cores, animando a altura (0fr ↔ 1fr), como os subitens da lateral. */}
        <div
          id="outra-cor"
          inert={!open}
          aria-hidden={!open}
          className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out ${
            open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"
          }`}
        >
          <div className="min-h-0 overflow-hidden">
            <div className="max-w-xs pt-1">
              <ColorPicker value={color} onChange={setColor} />
            </div>
          </div>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          disabled={unchanged}
          onClick={() => setEffect(draft)}
          className="rounded-md bg-brand px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-brand/85 disabled:cursor-default disabled:opacity-50 disabled:hover:bg-brand"
        >
          Aplicar
        </button>
        {applied && (
          <button
            type="button"
            onClick={() => {
              setEffect(null);
              setKind(null);
            }}
            className="rounded-md px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
          >
            Remover efeito
          </button>
        )}
        {unchanged && (
          <span className="text-xs text-text-faint">{applied ? "Efeito aplicado." : "Usando o anel padrão."}</span>
        )}
      </div>
    </>
  );
}

function EffectOption(props: { label: string; selected: boolean; onSelect: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={props.selected}
      aria-label={props.label}
      onClick={props.onSelect}
      className={`flex flex-col items-center gap-2 rounded-lg border px-1 pt-4 pb-2 transition-colors ${
        props.selected
          ? "border-brand bg-brand/10 text-text"
          : "border-white/[0.06] bg-surface-1 text-text-muted hover:bg-surface-hover hover:text-text"
      }`}
    >
      <span className="grid h-14 place-items-center">{props.children}</span>
      <span className="text-center text-xs leading-tight font-medium">{props.label}</span>
    </button>
  );
}
