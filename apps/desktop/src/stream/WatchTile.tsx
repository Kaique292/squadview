import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { ImageOff, Loader2, Monitor, MoreHorizontal, PictureInPicture2, VolumeX } from "lucide-react";
import { type KeyboardEvent, useEffect, useState } from "react";
import { useRoomStore } from "../room/room-store";
import { InfoBadge, LiveBadge } from "../ui/LiveBadge";
import { stopInsideMenu } from "../ui/overlay";
import { MENU_CONTENT } from "./StreamMenu";
import type { StreamTileVariant } from "./StreamTile";
import { WatchMenuItems } from "./WatchMenu";
import { useStreamStore } from "./stream-store";
import { useVideoSlot } from "./video-slot";

/** Sem nenhum quadro por esse tempo, avisamos que a imagem não está chegando. */
export const NO_IMAGE_AFTER_MS = 6_000;

/**
 * Se a imagem está chegando: enquanto chega, o vídeo nativo cobre a área; quando não chega (quem
 * transmite está com o jogo minimizado, ou numa versão antiga com o jogo em tela cheia), avisa em
 * vez de ficar parado em "Esperando a imagem".
 */
function useImageArriving(fps: number | undefined): boolean {
  const flowing = (fps ?? 0) > 0;
  const [waitedTooLong, setWaitedTooLong] = useState(false);
  useEffect(() => {
    if (flowing) return;
    const timer = setTimeout(() => setWaitedTooLong(true), NO_IMAGE_AFTER_MS);
    // A imagem voltou (ou saímos): a próxima espera começa do zero.
    return () => {
      clearTimeout(timer);
      setWaitedTooLong(false);
    };
  }, [flowing]);
  return flowing || !waitedTooLong;
}

/**
 * A live de alguém que assistimos, como um quadro da sala igual ao da nossa live (ADR 0019): o
 * vídeo (desenhado pelo Rust por cima), a qualidade e o "AO VIVO" no canto, o nome embaixo e os
 * "..." com o menu dela. Clicar põe em foco; dois cliques, tela cheia; botão direito, o menu.
 *
 * `hidden`: o quadro está na faixa recolhida, e o vídeo nativo (que não respeita o CSS) some junto.
 */
export function WatchTile(props: {
  memberId: string;
  variant: StreamTileVariant;
  focused: boolean;
  hidden?: boolean;
  onActivate: () => void;
  onDetails: () => void;
}) {
  const watch = useRoomStore((s) => s.watching[props.memberId]);
  const { fullscreen, fullscreenOf, pip, setFullscreen, setPip } = useStreamStore();
  const inPip = pip === props.memberId;
  // Com imagem chegando (e o vídeo aqui, não no picture in picture), o quadro fica transparente e
  // o vídeo, desenhado por baixo da página, aparece.
  const showing = !inPip && (watch?.stats?.fps ?? 0) > 0;
  const slot = useVideoSlot(props.memberId, props.hidden, showing);
  const arriving = useImageArriving(watch?.stats?.fps);
  const [pointerIn, setPointerIn] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  if (!watch) return null;

  const { member, stats, listening } = watch;
  const name = member.name;
  const strip = props.variant === "strip";
  const showMore = !strip && (pointerIn || menuOpen);
  const shape =
    props.variant === "focus" || props.variant === "fullscreen"
      ? "h-full w-full"
      : strip
        ? "aspect-video h-full"
        : "aspect-video w-full";
  const label = props.focused ? `Voltar para a grade (live de ${name} em foco)` : `Pôr a live de ${name} em foco`;

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
          data-watch-tile={props.memberId}
          onClick={props.onActivate}
          onDoubleClick={() =>
            void (inPip
              ? setPip(null)
              : setFullscreen(!(fullscreen && fullscreenOf === props.memberId), props.memberId))
          }
          onKeyDown={onKeyDown}
          onMouseEnter={() => setPointerIn(true)}
          onMouseLeave={() => setPointerIn(false)}
          className={`group relative ${shape} cursor-pointer overflow-hidden ${
            props.variant === "fullscreen" ? "" : "rounded-lg"
          } ${showing ? "" : "bg-black"} outline-none focus-visible:ring-2 focus-visible:ring-brand`}
        >
          {/* O vídeo é desenhado exatamente nesta área, por baixo da página; o aviso só aparece
              enquanto não há imagem (ou com o vídeo solto no picture in picture). */}
          <div ref={slot} data-testid={`video-de-${props.memberId}`} className="absolute inset-0">
            <div role="status" className="absolute inset-0 grid place-items-center p-4 text-center">
              {showing ? null : inPip ? (
                <div className="flex max-w-sm flex-col items-center gap-2">
                  <PictureInPicture2 size={strip ? 20 : 32} className="text-text-faint" />
                  {!strip && (
                    <>
                      <p className="text-sm font-semibold">Assistindo {name} em picture in picture</p>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          void setPip(null);
                        }}
                        className="rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-brand/85"
                      >
                        Trazer de volta
                      </button>
                    </>
                  )}
                </div>
              ) : arriving ? (
                <p className="flex items-center gap-2 text-sm text-text-faint">
                  <Loader2 size={16} className="animate-spin" />
                  {!strip && `Esperando a imagem de ${name}…`}
                </p>
              ) : (
                <div className="flex max-w-sm flex-col items-center gap-1.5">
                  <ImageOff size={strip ? 20 : 32} className="text-text-faint" />
                  {!strip && (
                    <>
                      <p className="text-sm font-semibold">{name} não está mandando imagem agora</p>
                      <p className="text-xs text-text-muted">
                        O jogo pode estar minimizado, ou em tela cheia numa versão antiga do SquadView. Assim que a
                        imagem voltar, ela aparece aqui sozinha.
                      </p>
                    </>
                  )}
                </div>
              )}
            </div>
          </div>

          <div className={`absolute flex items-center gap-1.5 ${strip ? "top-1.5 right-1.5" : "top-2.5 right-2.5"}`}>
            {!strip && stats && stats.height > 0 && (
              <InfoBadge>{`${stats.height}p ${Math.round(stats.fps)}fps`}</InfoBadge>
            )}
            <LiveBadge size={strip ? "sm" : "md"} />
          </div>

          <span
            className={`absolute flex max-w-[calc(100%-16px)] items-center gap-1.5 rounded-md bg-black/60 font-medium ${
              strip ? "bottom-1.5 left-1.5 px-1.5 py-0.5 text-xs" : "bottom-2.5 left-2.5 px-2 py-1 text-sm"
            }`}
          >
            <Monitor size={strip ? 12 : 15} className="shrink-0" />
            <span className="truncate">{name}</span>
            {!listening && (
              <VolumeX
                size={strip ? 12 : 14}
                aria-label="Som da transmissão desligado"
                className="shrink-0 text-live"
              />
            )}
          </span>

          {showMore && (
            <DropdownMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
              <DropdownMenu.Trigger asChild>
                <button
                  type="button"
                  aria-label={`Mais opções da live de ${name}`}
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
                  aria-label={`Opções da live de ${name}`}
                  className={MENU_CONTENT}
                  onClick={stopInsideMenu}
                  onDoubleClick={stopInsideMenu}
                >
                  <WatchMenuItems kind="dropdown" watch={watch} onDetails={props.onDetails} />
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          )}
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content
          aria-label={`Opções da live de ${name}`}
          className={MENU_CONTENT}
          onClick={stopInsideMenu}
          onDoubleClick={stopInsideMenu}
        >
          <WatchMenuItems kind="context" watch={watch} onDetails={props.onDetails} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
