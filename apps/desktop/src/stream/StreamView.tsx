import {
  ChartNoAxesColumn,
  Headphones,
  HeadphoneOff,
  Info,
  Maximize2,
  Minimize2,
  Radio,
  Square,
  TriangleAlert,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { useRoomStore } from "../room/room-store";
import { Tooltip } from "../ui/Tooltip";
import { StatsCard } from "./StatsCard";
import { useStreamStore } from "./stream-store";
import { useVideoSlot } from "./video-slot";

/**
 * O stream dentro do app, como no Discord: a área do vídeo (onde o Rust desenha, por cima da
 * interface) e uma barra de controles embaixo. Os detalhes ficam numa gaveta que encolhe o vídeo.
 */
export function StreamView() {
  const { stats, notice, listening, fullscreen, setListening, setFullscreen, stop } = useStreamStore();
  const [details, setDetails] = useState(false);
  const code = useRoomStore((s) => s.code);
  const slot = useVideoSlot();

  return (
    <section aria-label="Prévia ao vivo" className="flex h-full w-full flex-col gap-3">
      {notice && (
        <p
          role="status"
          className={`flex items-start gap-2 rounded-md px-3 py-2 text-sm ${
            notice.kind === "source-changed" ? "bg-brand/15" : "bg-warning/15"
          }`}
        >
          {notice.kind === "source-changed" ? (
            <Info size={16} className="mt-0.5 shrink-0 text-brand" />
          ) : (
            <TriangleAlert size={16} className="mt-0.5 shrink-0 text-warning" />
          )}
          {notice.message}
        </p>
      )}

      {/* O vídeo nativo é desenhado exatamente sobre esta área. */}
      <div
        ref={slot}
        data-testid="area-do-video"
        onDoubleClick={() => void setFullscreen(!fullscreen)}
        className="relative min-h-0 flex-1 overflow-hidden rounded-card bg-black"
      >
        <p className="absolute inset-0 grid place-items-center text-sm text-text-faint">Carregando o vídeo…</p>
      </div>

      <div className="flex items-center gap-2 rounded-card bg-surface-1 px-3 py-2">
        <span className="flex items-center gap-1 rounded bg-live px-2 py-0.5 text-xs font-bold text-white">
          <Radio size={12} /> AO VIVO
        </span>
        <span className="text-sm font-medium">{code ? `Transmitindo na sala ${code}` : "Prévia local"}</span>
        {stats && (
          <span className="ml-1 text-xs text-text-muted tabular-nums">
            {stats.fps.toFixed(0)} fps · {stats.latencyMs.toFixed(0)} ms · {(stats.bitrateKbps / 1000).toFixed(1)} Mbps
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          {stats?.audioActive && (
            <BarButton
              label={listening ? "Parar de ouvir a prévia" : "Ouvir o som da prévia"}
              active={listening}
              onClick={() => void setListening(!listening)}
            >
              {listening ? <Headphones size={18} /> : <HeadphoneOff size={18} />}
            </BarButton>
          )}
          <BarButton label="Detalhes da transmissão" active={details} onClick={() => setDetails(!details)}>
            <ChartNoAxesColumn size={18} />
          </BarButton>
          <BarButton
            label={fullscreen ? "Sair da tela cheia" : "Tela cheia"}
            onClick={() => void setFullscreen(!fullscreen)}
          >
            {fullscreen ? <Minimize2 size={18} /> : <Maximize2 size={18} />}
          </BarButton>
          <button
            type="button"
            onClick={() => void stop()}
            className="ml-1 flex items-center gap-1.5 rounded-md bg-live px-3 py-1.5 text-sm font-medium text-white hover:bg-live/85"
          >
            <Square size={14} /> Parar
          </button>
        </div>
      </div>

      {details && !fullscreen && (
        <div className="max-h-[40%] min-h-0 shrink-0 overflow-y-auto">
          <StatsCard />
        </div>
      )}
    </section>
  );
}

/**
 * Botão da barra do vídeo. A dica abre para baixo: em cima dela está o vídeo nativo, que fica por
 * cima de qualquer coisa da interface e a esconderia. Em tela cheia não sobra espaço embaixo, então
 * vale a dica do próprio Windows.
 */
export function BarButton(props: { label: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  const fullscreen = useStreamStore((s) => s.fullscreen);
  const button = (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={props.active}
      title={fullscreen ? props.label : undefined}
      onClick={props.onClick}
      className={`rounded-md p-2 transition-colors hover:bg-surface-hover hover:text-text ${
        props.active ? "text-text" : "text-text-muted"
      }`}
    >
      {props.children}
    </button>
  );
  return fullscreen ? (
    button
  ) : (
    <Tooltip label={props.label} side="bottom">
      {button}
    </Tooltip>
  );
}
