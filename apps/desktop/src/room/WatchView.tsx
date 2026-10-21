import { ImageOff, Loader2, Maximize2, Minimize2, PictureInPicture2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { BarButton } from "../stream/BarButton";
import { useStreamStore } from "../stream/stream-store";
import { useVideoSlot } from "../stream/video-slot";
import type { Member } from "./room-store";
import { useRoomStore } from "./room-store";
import { Viewers } from "./Viewers";
import { VolumeControl } from "./VolumeControl";

/** Sem nenhum quadro por esse tempo, avisamos que a imagem não está chegando. */
export const NO_IMAGE_AFTER_MS = 6_000;

/**
 * Se a imagem está chegando: enquanto chega, o vídeo nativo cobre a área; quando não chega (quem
 * transmite está com o jogo minimizado, ou numa versão antiga com o jogo em tela cheia), avisa em
 * vez de ficar parado em "Conectando…".
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

/** Assistindo um amigo: o vídeo dele dentro do app, com uma barra embaixo. */
export function WatchView(props: { member: Member }) {
  const { watchStats: stats, stopWatching } = useRoomStore();
  const { fullscreen, setFullscreen, pip, setPip } = useStreamStore();
  const slot = useVideoSlot();
  const arriving = useImageArriving(stats?.fps);
  const name = props.member.name;
  // Parou de assistir (o vídeo fechou junto com a janelinha): o próximo começa dentro do app.
  useEffect(() => () => useStreamStore.setState({ pip: false }), []);

  return (
    <section aria-label={`Assistindo ${props.member.name}`} className="flex h-full w-full flex-col gap-3">
      <div
        ref={slot}
        data-testid="area-do-video"
        onDoubleClick={() => void (pip ? setPip(false) : setFullscreen(!fullscreen))}
        className="relative min-h-0 flex-1 overflow-hidden rounded-card bg-black"
      >
        {/* Fica por baixo do vídeo nativo: só aparece enquanto não há imagem (ou ele está solto). */}
        <div role="status" className="absolute inset-0 grid place-items-center p-6 text-center">
          {pip ? (
            <div className="flex max-w-sm flex-col items-center gap-3">
              <PictureInPicture2 size={36} className="text-text-faint" />
              <p className="font-semibold">Assistindo {name} em picture in picture</p>
              <p className="text-sm text-text-muted">
                A janelinha fica por cima de tudo. Arraste para mover, puxe a borda para mudar o tamanho e dê dois
                cliques nela para voltar.
              </p>
              <button
                type="button"
                onClick={() => void setPip(false)}
                className="rounded-md bg-brand px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-brand/85"
              >
                Trazer de volta
              </button>
            </div>
          ) : arriving ? (
            <p className="flex items-center gap-2 text-sm text-text-faint">
              <Loader2 size={16} className="animate-spin" />
              {stats ? `Esperando a imagem de ${name}…` : `Conectado. Esperando a imagem de ${name}…`}
            </p>
          ) : (
            <div className="flex max-w-sm flex-col items-center gap-2">
              <ImageOff size={36} className="text-text-faint" />
              <p className="font-semibold">{name} não está mandando imagem agora</p>
              <p className="text-sm text-text-muted">
                O jogo pode estar minimizado, ou em tela cheia numa versão antiga do SquadView. Peça para {name}{" "}
                conferir a tela dele ou atualizar o app. Assim que a imagem voltar, ela aparece aqui sozinha.
              </p>
            </div>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2 rounded-card bg-surface-1 px-3 py-2">
        <span className="text-sm font-medium">Assistindo {props.member.name}</span>
        {stats && (
          <span className="ml-1 text-xs text-text-muted tabular-nums">
            {stats.fps.toFixed(0)} fps · {stats.latencyMs.toFixed(0)} ms · {(stats.bitrateKbps / 1000).toFixed(1)} Mbps
            {stats.rttMs > 0 && ` · ping ${stats.rttMs.toFixed(0)} ms`}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          <span className="mr-2">
            <Viewers streamerId={props.member.id} side="bottom" />
          </span>
          <VolumeControl />
          <BarButton
            label={pip ? "Trazer o vídeo de volta" : "Picture in picture"}
            active={pip}
            onClick={() => void setPip(!pip)}
          >
            <PictureInPicture2 size={18} />
          </BarButton>
          <BarButton
            label={fullscreen ? "Sair da tela cheia" : "Tela cheia"}
            onClick={() => void setFullscreen(!fullscreen)}
          >
            {fullscreen ? <Minimize2 size={18} /> : <Maximize2 size={18} />}
          </BarButton>
          <button
            type="button"
            onClick={() => {
              void setFullscreen(false);
              void setPip(false);
              void stopWatching();
            }}
            className="ml-1 flex items-center gap-1.5 rounded-md bg-surface-3 px-3 py-1.5 text-sm font-medium hover:bg-surface-hover"
          >
            <X size={14} /> Parar de assistir
          </button>
        </div>
      </div>
    </section>
  );
}
