import { Maximize2, Minimize2, X } from "lucide-react";
import { BarButton } from "../stream/StreamView";
import { useStreamStore } from "../stream/stream-store";
import { useVideoSlot } from "../stream/video-slot";
import type { Member } from "./room-store";
import { useRoomStore } from "./room-store";
import { VolumeControl } from "./VolumeControl";

/** Assistindo um amigo: o vídeo dele dentro do app, com uma barra embaixo. */
export function WatchView(props: { member: Member }) {
  const { watchStats: stats, stopWatching } = useRoomStore();
  const { fullscreen, setFullscreen } = useStreamStore();
  const slot = useVideoSlot();

  return (
    <section aria-label={`Assistindo ${props.member.name}`} className="flex h-full w-full flex-col gap-3">
      <div
        ref={slot}
        data-testid="area-do-video"
        onDoubleClick={() => void setFullscreen(!fullscreen)}
        className="relative min-h-0 flex-1 overflow-hidden rounded-card bg-black"
      >
        <p className="absolute inset-0 grid place-items-center text-sm text-text-faint">
          Conectando com {props.member.name}…
        </p>
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
          <VolumeControl />
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
