import { Activity, Gauge, Radio, Timer, Volume2 } from "lucide-react";
import { useRoomStore } from "../room/room-store";
import { Viewers } from "../room/Viewers";
import { InfoBadge, LiveBadge } from "../ui/LiveBadge";
import { Modal } from "../ui/Modal";
import { Bar, Metric } from "./StreamDetailsDialog";

/**
 * Detalhes da live de quem assistimos (os "..." do quadro dela): o que chega aqui, com o último
 * minuto em gráfico, o ping até quem transmite e o som. Atualiza sozinho a cada medição.
 */
export function WatchDetailsDialog(props: {
  memberId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const name = useRoomStore((s) => (props.memberId ? s.watching[props.memberId]?.member.name : undefined));
  return (
    <Modal
      // A live acabou com os detalhes abertos: eles fecham junto.
      open={props.open && !!name}
      onOpenChange={props.onOpenChange}
      width={760}
      title={name ? `Live de ${name}` : "Detalhes da transmissão"}
    >
      {/* Um componente à parte: fechado, nada disto é calculado a cada medição. */}
      {props.memberId && <Details memberId={props.memberId} />}
    </Modal>
  );
}

function Details(props: { memberId: string }) {
  const watch = useRoomStore((s) => s.watching[props.memberId]);
  if (!watch) return null;
  const { stats, history } = watch;

  return (
    <div className="flex flex-col gap-4 pb-6 text-left">
      <div className="flex flex-wrap items-center justify-center gap-2">
        <LiveBadge />
        {stats && stats.height > 0 && (
          <InfoBadge className="!bg-surface-3">{`${stats.width}×${stats.height}`}</InfoBadge>
        )}
        {/* Quem mais assiste, com as fotos (os nomes na dica). */}
        <span className="ml-2">
          <Viewers streamerId={props.memberId} side="bottom" />
        </span>
      </div>

      {stats === null ? (
        <p className="py-10 text-center text-sm text-text-muted">Medindo…</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-3">
            <Metric
              icon={<Activity size={16} />}
              label="Quadros recebidos"
              value={stats.fps.toFixed(0)}
              unit="fps"
              values={history.map((h) => h.fps)}
              color="var(--color-online)"
            />
            <Metric
              icon={<Gauge size={16} />}
              label="Bitrate"
              value={(stats.bitrateKbps / 1000).toFixed(1)}
              unit="Mbps"
              values={history.map((h) => h.bitrateKbps / 1000)}
              color="var(--color-brand)"
            />
            <Metric
              icon={<Timer size={16} />}
              label="Latência (captura → sua tela)"
              value={stats.latencyMs.toFixed(0)}
              unit="ms"
              detail={`pior: ${stats.latencyP95Ms.toFixed(0)} ms`}
              values={history.map((h) => h.latencyMs)}
              color={stats.latencyMs > 100 ? "var(--color-warning)" : "#3ba0ff"}
              warn={stats.latencyMs > 100}
            />
            <Metric
              icon={<Radio size={16} />}
              label="Ping"
              value={stats.rttMs > 0 ? stats.rttMs.toFixed(0) : "–"}
              unit="ms"
              detail={stats.dropped > 0 ? `${stats.dropped} quadros descartados` : undefined}
              values={history.map((h) => h.rttMs)}
              color="#e06c9f"
            />
          </div>

          <section aria-label="Som da transmissão" className="rounded-card bg-surface-1 p-4">
            <h3 className="mb-3 flex items-center gap-1.5 text-xs font-semibold tracking-wide text-text-faint uppercase">
              <Volume2 size={14} /> Som da transmissão
            </h3>
            {stats.audioActive ? (
              <>
                <Bar label="Nível do som" percent={stats.audioLevel * 100} />
                <p className="mt-2 text-xs text-text-muted tabular-nums">
                  {stats.audioKbps} kbps{stats.audioLost > 0 ? ` · ${stats.audioLost} pacotes perdidos` : ""}
                </p>
              </>
            ) : (
              <p className="text-sm text-text-muted">{stats.audioError ?? "Esta live está sem som."}</p>
            )}
          </section>
        </>
      )}
    </div>
  );
}
