import * as Menu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown, ChevronRight, Headphones, HeadphoneOff, Mic, MicOff } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { useOverlayMenu } from "../ui/overlay";
import { Tooltip } from "../ui/Tooltip";
import { type AudioDevice, MAX_VOICE_VOLUME, useVoiceStore } from "./voice-store";
import { CheckMark } from "../ui/Checkbox";

/** Microfone: liga e desliga o mudo; a setinha abre o dispositivo, o volume e o nível. */
export function MicButton(props: { large?: boolean }) {
  const { prefs, setMuted, setPrefs, devices, loadDevices } = useVoiceStore();
  const off = prefs.muted || prefs.deafened;
  return (
    <SplitButton
      large={props.large}
      off={off}
      label={off ? "Ativar microfone" : "Silenciar microfone"}
      icon={off ? <MicOff size={props.large ? 20 : 18} /> : <Mic size={props.large ? 20 : 18} />}
      onClick={() => void setMuted(!off)}
      menuLabel="Opções de entrada"
      onOpenMenu={() => void loadDevices()}
    >
      <DeviceMenu
        title="Dispositivo de entrada"
        devices={devices.inputs}
        selected={prefs.inputDevice}
        onSelect={(id) => void setPrefs({ inputDevice: id })}
      />
      <Menu.Separator className="my-1 h-px bg-surface-3" />
      <VolumeSlider
        label="Volume de entrada"
        value={prefs.inputVolume}
        onChange={(v) => void setPrefs({ inputVolume: v })}
      />
      <LevelMeter />
      <Menu.Separator className="my-1 h-px bg-surface-3" />
      <Menu.CheckboxItem
        checked={prefs.noiseSuppression}
        onCheckedChange={(v) => void setPrefs({ noiseSuppression: v })}
        onSelect={(e) => e.preventDefault()}
        className="flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white"
      >
        <span>
          Isolamento de voz
          <span className="block text-xs font-normal opacity-70">Corta ruído de fundo (teclado, ventilador)</span>
        </span>
        <CheckMark checked={prefs.noiseSuppression} />
      </Menu.CheckboxItem>
    </SplitButton>
  );
}

/** Fone: ensurdece (não ouve ninguém e fica mudo); a setinha abre a saída e o volume. */
export function DeafenButton(props: { large?: boolean }) {
  const { prefs, setDeafened, setPrefs, devices, loadDevices } = useVoiceStore();
  return (
    <SplitButton
      large={props.large}
      off={prefs.deafened}
      label={prefs.deafened ? "Ativar áudio" : "Ensurdecer"}
      icon={
        prefs.deafened ? <HeadphoneOff size={props.large ? 20 : 18} /> : <Headphones size={props.large ? 20 : 18} />
      }
      onClick={() => void setDeafened(!prefs.deafened)}
      menuLabel="Opções de saída"
      onOpenMenu={() => void loadDevices()}
    >
      <DeviceMenu
        title="Dispositivo de saída"
        devices={devices.outputs}
        selected={prefs.outputDevice}
        onSelect={(id) => void setPrefs({ outputDevice: id })}
      />
      <Menu.Separator className="my-1 h-px bg-surface-3" />
      <VolumeSlider
        label="Volume de saída"
        value={prefs.outputVolume}
        onChange={(v) => void setPrefs({ outputVolume: v })}
      />
    </SplitButton>
  );
}

/** Botão com a ação principal e a setinha que abre o menu (como os do painel do Discord). */
function SplitButton(props: {
  large?: boolean;
  off: boolean;
  label: string;
  icon: ReactNode;
  onClick: () => void;
  menuLabel: string;
  onOpenMenu: () => void;
  children: ReactNode;
}) {
  // O grande fica dentro de um grupo da barra da chamada: sem fundo próprio, só no hover.
  const tone = props.off
    ? "text-live hover:bg-live/25"
    : props.large
      ? "text-text hover:bg-surface-hover"
      : "text-text-muted hover:bg-surface-hover hover:text-text";
  const height = props.large ? "h-10" : "h-8";
  const onOpenChange = useOverlayMenu((open) => open && props.onOpenMenu());
  return (
    <div className={`flex items-center rounded-md transition-colors ${props.off ? "bg-live/15" : ""}`}>
      <Tooltip label={props.label} side="top">
        <button
          type="button"
          aria-label={props.label}
          aria-pressed={props.off}
          onClick={props.onClick}
          className={`grid ${height} ${props.large ? "w-10" : "w-8"} place-items-center rounded-l-md transition-colors ${tone}`}
        >
          {props.icon}
        </button>
      </Tooltip>
      <Menu.Root onOpenChange={onOpenChange}>
        <Menu.Trigger asChild>
          <button
            type="button"
            aria-label={props.menuLabel}
            className={`group grid ${height} ${props.large ? "w-6" : "w-5"} place-items-center rounded-r-md transition-colors ${
              props.off
                ? "text-live hover:bg-live/25 data-[state=open]:bg-live/25"
                : props.large
                  ? "bg-white/[0.03] text-text-muted hover:bg-surface-hover hover:text-text data-[state=open]:bg-surface-hover"
                  : "text-text-muted hover:bg-surface-hover hover:text-text data-[state=open]:bg-surface-hover"
            }`}
          >
            <ChevronDown size={14} className="transition-transform duration-200 group-data-[state=open]:rotate-180" />
          </button>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            side="top"
            align="end"
            sideOffset={8}
            className="z-50 w-72 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            {props.children}
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
    </div>
  );
}

function DeviceMenu(props: {
  title: string;
  devices: AudioDevice[];
  selected: string | null;
  onSelect: (id: string | null) => void;
}) {
  const current = props.devices.find((d) => d.id === props.selected) ?? props.devices.find((d) => d.isDefault) ?? null;
  return (
    <Menu.Sub>
      <Menu.SubTrigger className="flex cursor-pointer items-center justify-between gap-2 rounded px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white data-[state=open]:bg-surface-hover">
        <span className="min-w-0">
          <span className="block font-medium">{props.title}</span>
          <span className="block truncate text-xs opacity-70">{current?.name ?? "Padrão do Windows"}</span>
        </span>
        <ChevronRight size={16} className="shrink-0" />
      </Menu.SubTrigger>
      <Menu.Portal>
        <Menu.SubContent
          sideOffset={6}
          className="z-50 max-h-80 w-72 overflow-y-auto rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=open]:animate-menu-in"
        >
          <Menu.RadioGroup
            value={props.selected ?? "padrao"}
            onValueChange={(v) => props.onSelect(v === "padrao" ? null : v)}
          >
            <RadioItem value="padrao">Padrão do Windows</RadioItem>
            {props.devices.map((d) => (
              <RadioItem key={d.id} value={d.id}>
                {d.name}
              </RadioItem>
            ))}
          </Menu.RadioGroup>
        </Menu.SubContent>
      </Menu.Portal>
    </Menu.Sub>
  );
}

function RadioItem(props: { value: string; children: ReactNode }) {
  return (
    <Menu.RadioItem
      value={props.value}
      className="flex cursor-pointer items-center justify-between gap-2 rounded px-2 py-1.5 text-sm text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white data-[state=checked]:text-text"
    >
      <span className="min-w-0 truncate">{props.children}</span>
      <Menu.ItemIndicator>
        <Check size={14} />
      </Menu.ItemIndicator>
    </Menu.RadioItem>
  );
}

/** Controle de volume dentro do menu (as setas do teclado mexem nele, não no menu). */
export function VolumeSlider(props: { label: string; value: number; onChange: (v: number) => void }) {
  return (
    <div className="w-full px-2 py-2" onKeyDown={(e) => e.stopPropagation()}>
      {/* Bloco na largura toda: o controle ocupa a linha inteira, como no Discord. */}
      <label className="block w-full text-sm font-medium">
        <span className="flex items-center justify-between">
          {props.label}
          <span className="text-xs font-normal text-text-faint tabular-nums">{props.value}%</span>
        </span>
        <input
          type="range"
          aria-label={props.label}
          min={0}
          max={MAX_VOICE_VOLUME}
          step={5}
          value={props.value}
          onChange={(e) => props.onChange(Number(e.target.value))}
          style={{ "--fill": `${(props.value / MAX_VOICE_VOLUME) * 100}%` } as CSSProperties}
          className="glass-slider mt-3 block w-full"
        />
      </label>
    </div>
  );
}

/** Nível do microfone ao vivo, em barrinhas (como o "Nível de entrada" do Discord). */
function LevelMeter() {
  const level = useVoiceStore((s) => s.level);
  const speaking = useVoiceStore((s) => !!s.speaking.me);
  const bars = 24;
  const lit = Math.round(Math.min(1, level * 1.6) * bars);
  return (
    <div className="px-2 pt-1 pb-2">
      <p className="mb-2 text-sm font-medium">Nível de entrada</p>
      <div role="meter" aria-label="Nível de entrada" aria-valuenow={Math.round(level * 100)} className="flex gap-0.5">
        {Array.from({ length: bars }, (_, i) => (
          <span
            key={i}
            className={`h-4 flex-1 rounded-sm transition-colors duration-75 ${i < lit ? (speaking ? "bg-online" : "bg-text-muted") : "bg-surface-3"}`}
          />
        ))}
      </div>
    </div>
  );
}
