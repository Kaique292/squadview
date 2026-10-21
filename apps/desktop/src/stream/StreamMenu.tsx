import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { ChartNoAxesColumn, ChevronRight, Maximize2, MonitorUp, MonitorX } from "lucide-react";
import type { ReactNode } from "react";
import { CheckMark } from "../ui/Checkbox";
import { type Resolution, type StreamQuality, useStreamStore } from "./stream-store";

/** As peças do Radix são as mesmas no menu do botão direito e no dos "...": usamos as duas. */
type Kit = typeof ContextMenu;
const KITS: Record<"context" | "dropdown", Kit> = {
  context: ContextMenu,
  dropdown: DropdownMenu as unknown as Kit,
};

export const QUALITY_RESOLUTIONS: { value: Resolution; label: string }[] = [
  { value: "720", label: "720p" },
  { value: "1080", label: "1080p" },
  { value: "1440", label: "1440p" },
  { value: "source", label: "Fonte" },
];
export const QUALITY_FPS: StreamQuality["fps"][] = [15, 30, 60];

/** A qualidade como no selo da live: "720P 30FPS" (ou a altura de verdade, na "Fonte"). */
export function qualityLabel(quality: StreamQuality, sourceHeight?: number) {
  const height = quality.resolution === "source" ? (sourceHeight ? String(sourceHeight) : "Fonte") : quality.resolution;
  return `${height}${height === "Fonte" ? "" : "p"} ${quality.fps}fps`;
}

export const MENU_CONTENT =
  "z-50 w-72 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in";
const ITEM =
  "flex cursor-pointer items-center justify-between gap-3 rounded px-2 py-2 text-sm font-medium text-text-muted outline-none data-[disabled]:cursor-default data-[disabled]:opacity-50 data-[highlighted]:bg-brand data-[highlighted]:text-white";

/**
 * O menu da nossa live, como o do Discord: parar, trocar o que transmite, a qualidade, o som que vai
 * junto, os detalhes e a tela cheia. Igual no botão direito do quadro e nos "..." dele.
 */
export function StreamMenuItems(props: { kind: "context" | "dropdown"; onChange: () => void; onDetails: () => void }) {
  const M = KITS[props.kind];
  const { quality, current, fullscreen, changeQuality, setShareAudio, setFullscreen, stop } = useStreamStore();
  const sharing = current?.audioPid != null;
  const canShare = current?.lastAudioPid != null;

  return (
    <>
      <M.Item
        onSelect={() => void stop()}
        className={`${ITEM} !text-live data-[highlighted]:!bg-live data-[highlighted]:!text-white`}
      >
        Parar transmissão
        <MonitorX size={18} />
      </M.Item>
      <M.Item onSelect={props.onChange} className={ITEM}>
        Trocar transmissão
        <MonitorUp size={18} />
      </M.Item>
      <M.Sub>
        <M.SubTrigger className={`${ITEM} data-[state=open]:bg-surface-hover`}>
          Qualidade da transmissão
          <ChevronRight size={16} />
        </M.SubTrigger>
        <M.Portal>
          <M.SubContent sideOffset={6} aria-label="Qualidade da transmissão" className={`${MENU_CONTENT} w-48`}>
            <GroupLabel kit={M}>Resolução</GroupLabel>
            <M.RadioGroup
              value={quality.resolution}
              onValueChange={(v) => void changeQuality({ ...quality, resolution: v as Resolution })}
            >
              {QUALITY_RESOLUTIONS.map((r) => (
                <Radio key={r.value} kit={M} value={r.value}>
                  {r.label}
                </Radio>
              ))}
            </M.RadioGroup>
            <M.Separator className="my-1 h-px bg-surface-3" />
            <GroupLabel kit={M}>Quadros por segundo</GroupLabel>
            <M.RadioGroup
              value={String(quality.fps)}
              onValueChange={(v) => void changeQuality({ ...quality, fps: Number(v) as StreamQuality["fps"] })}
            >
              {QUALITY_FPS.map((fps) => (
                <Radio key={fps} kit={M} value={String(fps)}>
                  {`${fps} fps`}
                </Radio>
              ))}
            </M.RadioGroup>
          </M.SubContent>
        </M.Portal>
      </M.Sub>
      <M.CheckboxItem
        checked={sharing}
        disabled={!canShare}
        onCheckedChange={(on) => void setShareAudio(on)}
        // O menu fica aberto para ver a caixa marcar (como no Discord).
        onSelect={(e) => e.preventDefault()}
        className={ITEM}
      >
        <span>
          Compartilhar áudio da transmissão
          {!canShare && (
            <span className="block text-xs font-normal opacity-80">Escolha um programa no Trocar transmissão</span>
          )}
        </span>
        <CheckMark checked={sharing} />
      </M.CheckboxItem>
      <M.Separator className="my-1 h-px bg-surface-3" />
      <M.Item onSelect={props.onDetails} className={ITEM}>
        Detalhes da transmissão
        <ChartNoAxesColumn size={18} />
      </M.Item>
      <M.Item onSelect={() => void setFullscreen(!fullscreen)} className={ITEM}>
        {fullscreen ? "Sair da tela cheia" : "Tela cheia"}
        <Maximize2 size={18} />
      </M.Item>
    </>
  );
}

function GroupLabel(props: { kit: Kit; children: ReactNode }) {
  const M = props.kit;
  return (
    <M.Label className="px-2 pt-1 pb-0.5 text-[11px] font-semibold tracking-wide text-text-faint uppercase">
      {props.children}
    </M.Label>
  );
}

function Radio(props: { kit: Kit; value: string; children: string }) {
  const M = props.kit;
  return (
    <M.RadioItem
      value={props.value}
      onSelect={(e) => e.preventDefault()}
      className="flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white data-[state=checked]:text-text"
    >
      {props.children}
      <span className="grid size-4 place-items-center rounded-full border-2 border-current">
        <M.ItemIndicator className="size-2 rounded-full bg-current" />
      </span>
    </M.RadioItem>
  );
}
