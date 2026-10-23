import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Monitor, MoreHorizontal } from "lucide-react";
import { type KeyboardEvent, useState } from "react";
import { useRoomStore } from "../room/room-store";
import { ArcadeBadge, InfoBadge, LiveBadge } from "../ui/LiveBadge";
import { stopInsideMenu } from "../ui/overlay";
import { MENU_CONTENT, StreamMenuItems, qualityLabel } from "./StreamMenu";
import { useStreamStore } from "./stream-store";
import { useVideoSlot } from "./video-slot";

/** Como o quadro aparece: na grade com todos, grande em foco, pequeno na faixa ou em tela cheia. */
export type StreamTileVariant = "grid" | "focus" | "strip" | "fullscreen";

/**
 * A nossa live como um quadro da sala, como no Discord: o vídeo (desenhado pelo Rust por baixo),
 * a qualidade e o "AO VIVO" no canto de cima, o nome embaixo e os "..." com o mouse em cima.
 * Clicar põe em foco; dois cliques, tela cheia; botão direito, o menu da live.
 *
 * O vídeo fica por baixo da página (ADR 0021): selos, nome e botões são desenhados por cima dele,
 * e o mouse cai direto no quadro.
 */
export function StreamTile(props: {
  variant: StreamTileVariant;
  /** Está na faixa recolhida: o vídeo some junto. */
  hidden?: boolean;
  focused: boolean;
  onActivate: () => void;
  onChange: () => void;
  onDetails: () => void;
}) {
  const { quality, stats, fullscreen, setFullscreen } = useStreamStore();
  const myName = useRoomStore((s) => s.members[0]?.name) ?? "Você";
  const arcade = useRoomStore((s) => s.members[0]?.arcade);
  // Com imagem chegando, o quadro fica transparente e o vídeo, desenhado por baixo, aparece.
  const showing = (stats?.fps ?? 0) > 0;
  // Na faixa recolhida, o vídeo some junto com o quadro (antes ficava aparecendo por cima).
  const slot = useVideoSlot(undefined, !!props.hidden, showing);
  const [pointerIn, setPointerIn] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const strip = props.variant === "strip";
  const showMore = !strip && (pointerIn || menuOpen);
  const shape =
    props.variant === "focus" || props.variant === "fullscreen"
      ? "h-full w-full"
      : strip
        ? "aspect-video h-full"
        : "aspect-video w-full";
  const label = props.focused ? "Voltar para a grade (sua transmissão em foco)" : "Pôr sua transmissão em foco";

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
    e.preventDefault();
    props.onActivate();
  }

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          role="button"
          tabIndex={0}
          aria-label={label}
          aria-pressed={props.focused}
          data-stream-tile
          onClick={props.onActivate}
          onDoubleClick={() => void setFullscreen(!fullscreen)}
          onKeyDown={onKeyDown}
          onMouseEnter={() => setPointerIn(true)}
          onMouseLeave={() => setPointerIn(false)}
          className={`group relative ${shape} cursor-pointer overflow-hidden ${
            props.variant === "fullscreen" ? "" : "rounded-lg"
          } ${showing ? "" : "bg-black"} outline-none focus-visible:ring-2 focus-visible:ring-brand`}
        >
          {/* O vídeo é desenhado exatamente nesta área, por baixo da página. */}
          <div ref={slot} data-testid="area-do-video" className="absolute inset-0">
            {!showing && (
              <p className="absolute inset-0 grid place-items-center text-sm text-text-faint">Carregando o vídeo…</p>
            )}
          </div>

          <div className={`absolute flex items-center gap-1.5 ${strip ? "top-1.5 right-1.5" : "top-2.5 right-2.5"}`}>
            {!strip && <InfoBadge>{qualityLabel(quality, stats?.height)}</InfoBadge>}
            {arcade && <ArcadeBadge arcade={arcade} size={strip ? "sm" : "md"} compact={strip} />}
            <LiveBadge size={strip ? "sm" : "md"} />
          </div>

          <span
            className={`absolute flex max-w-[calc(100%-16px)] items-center gap-1.5 rounded-md bg-black/60 font-medium ${
              strip ? "bottom-1.5 left-1.5 px-1.5 py-0.5 text-xs" : "bottom-2.5 left-2.5 px-2 py-1 text-sm"
            }`}
          >
            <Monitor size={strip ? 12 : 15} className="shrink-0" />
            <span className="truncate">{myName}</span>
          </span>

          {showMore && (
            <DropdownMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
              <DropdownMenu.Trigger asChild>
                <button
                  type="button"
                  aria-label="Mais opções da transmissão"
                  onClick={(e) => e.stopPropagation()}
                  onDoubleClick={(e) => e.stopPropagation()}
                  className="absolute right-2.5 bottom-2.5 grid size-8 place-items-center rounded-md bg-black/60 text-white transition-colors hover:bg-black/80 data-[state=open]:bg-black/80"
                >
                  <MoreHorizontal size={18} />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content
                  side="top"
                  align="end"
                  sideOffset={6}
                  aria-label="Opções da transmissão"
                  className={MENU_CONTENT}
                  onClick={stopInsideMenu}
                  onDoubleClick={stopInsideMenu}
                >
                  <StreamMenuItems kind="dropdown" onChange={props.onChange} onDetails={props.onDetails} />
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          )}
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content
          aria-label="Opções da transmissão"
          className={MENU_CONTENT}
          onClick={stopInsideMenu}
          onDoubleClick={stopInsideMenu}
        >
          <StreamMenuItems kind="context" onChange={props.onChange} onDetails={props.onDetails} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
