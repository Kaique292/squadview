import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  ChartNoAxesColumn,
  EyeOff,
  Gamepad2,
  LogOut,
  Maximize2,
  PictureInPicture2,
  Volume2,
  VolumeX,
} from "lucide-react";
import type { CSSProperties } from "react";
import { freeSlots, useArcadeStore } from "../arcade/arcade-store";
import { MAX_WATCH_VOLUME, useRoomStore, type Watch } from "../room/room-store";
import { CheckMark } from "../ui/Checkbox";
import { useStreamStore } from "./stream-store";

/** As peças do Radix são as mesmas no menu do botão direito e no dos "...": usamos as duas. */
type Kit = typeof ContextMenu;
const KITS: Record<"context" | "dropdown", Kit> = {
  context: ContextMenu,
  dropdown: DropdownMenu as unknown as Kit,
};

const ITEM =
  "flex cursor-pointer items-center justify-between gap-3 rounded px-2 py-2 text-sm font-medium text-text-muted outline-none data-[disabled]:cursor-default data-[disabled]:opacity-50 data-[highlighted]:bg-brand data-[highlighted]:text-white";

/**
 * O menu da live de quem assistimos, como o do Discord: o volume dela, silenciar, soltar em picture
 * in picture, tela cheia, os detalhes, entrar no fliperama e parar de assistir. Igual no botão
 * direito e nos "...".
 */
export function WatchMenuItems(props: { kind: "context" | "dropdown"; watch: Watch; onDetails: () => void }) {
  const M = KITS[props.kind];
  const { setWatchVolume, setWatchListening, stopWatching } = useRoomStore();
  const { fullscreen, fullscreenOf, pip, setFullscreen, setPip } = useStreamStore();
  const { volume, listening } = props.watch;
  const memberId = props.watch.member.id;
  const inFullscreen = fullscreen && fullscreenOf === memberId;
  const inPip = pip === memberId;

  return (
    <>
      <M.Label className="px-2 pt-1 text-[11px] font-semibold tracking-wide text-text-faint uppercase">
        Volume da transmissão
      </M.Label>
      <div className="flex items-center gap-2 px-2 pt-1 pb-2">
        <input
          type="range"
          aria-label="Volume da transmissão"
          aria-valuetext={`${volume}%`}
          min={0}
          max={MAX_WATCH_VOLUME}
          step={5}
          value={volume}
          // As setas mexem no volume, não no item destacado do menu.
          onKeyDown={(e) => e.stopPropagation()}
          onChange={(e) => {
            const v = Number(e.target.value);
            void setWatchVolume(memberId, v);
            // Mexer no volume com o som desligado liga o som, como no Discord.
            if (!listening && v > 0) void setWatchListening(memberId, true);
          }}
          style={{ "--fill": `${(volume / MAX_WATCH_VOLUME) * 100}%` } as CSSProperties}
          className="glass-slider min-w-0 flex-1"
        />
        <span className="w-10 text-right text-xs text-text-muted tabular-nums">{volume}%</span>
      </div>
      <M.CheckboxItem
        checked={!listening}
        onCheckedChange={(muted) => void setWatchListening(memberId, !muted)}
        // O menu fica aberto para ver a caixa marcar (como no Discord).
        onSelect={(e) => e.preventDefault()}
        className={ITEM}
      >
        <span className="flex items-center gap-2">
          {listening ? <Volume2 size={16} /> : <VolumeX size={16} />}
          Silenciar transmissão
        </span>
        <CheckMark checked={!listening} />
      </M.CheckboxItem>
      <M.Separator className="my-1 h-px bg-surface-3" />
      <M.Item onSelect={() => void setPip(inPip ? null : memberId)} className={ITEM}>
        {inPip ? "Trazer o vídeo de volta" : "Picture in picture"}
        <PictureInPicture2 size={18} />
      </M.Item>
      <M.Item onSelect={() => void setFullscreen(!inFullscreen, memberId)} className={ITEM}>
        {inFullscreen ? "Sair da tela cheia" : "Tela cheia"}
        <Maximize2 size={18} />
      </M.Item>
      <M.Item onSelect={props.onDetails} className={ITEM}>
        Detalhes da transmissão
        <ChartNoAxesColumn size={18} />
      </M.Item>
      <ArcadeItem kit={M} memberId={memberId} />
      <M.Separator className="my-1 h-px bg-surface-3" />
      <M.Item
        onSelect={() => void stopWatching(memberId)}
        className={`${ITEM} !text-live data-[highlighted]:!bg-live data-[highlighted]:!text-white`}
      >
        Parar de assistir
        <EyeOff size={18} />
      </M.Item>
    </>
  );
}

/**
 * O fliperama aberto nessa live (ADR 0023): entrar como jogador ou sair, como no botão direito no
 * nome de quem transmite. Sem fliperama, não aparece.
 */
function ArcadeItem(props: { kit: Kit; memberId: string }) {
  const M = props.kit;
  // O membro atualizado da sala: o fliperama abre e enche depois de começarmos a assistir.
  const member = useRoomStore((s) => s.members.find((m) => m.id === props.memberId));
  const playing = useArcadeStore((s) => s.playing);
  const { join, leave } = useArcadeStore();
  if (!member?.arcade) return null;
  if (playing?.hostId === member.id) {
    return (
      <M.Item onSelect={() => void leave()} className={ITEM}>
        Sair do fliperama
        <LogOut size={18} />
      </M.Item>
    );
  }
  const free = freeSlots(member);
  return (
    <M.Item disabled={free === 0} onSelect={() => void join(member)} className={ITEM}>
      {free === 0 ? "Fliperama cheio" : `Entrar no fliperama (${free} ${free === 1 ? "vaga" : "vagas"})`}
      <Gamepad2 size={18} />
    </M.Item>
  );
}
