import { Activity, Cpu, Gauge, Timer, Volume2 } from "lucide-react";
import type { ReactNode } from "react";
import { useRoomStore, watches } from "../room/room-store";
import { InfoBadge, LiveBadge } from "../ui/LiveBadge";
import { Modal } from "../ui/Modal";
import { Switch } from "../ui/Switch";
import { qualityLabel } from "./StreamMenu";
import { HISTORY_SIZE, type StreamStats, useStreamStore } from "./stream-store";

/**
 * Detalhes da transmissão (os "..." da live): os números de agora, com o último minuto em gráfico
 * e o uso da GPU e da CPU em barras. Tudo se atualiza sozinho a cada medição (500 ms).
 */
export function StreamDetailsDialog(props: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Modal open={props.open} onOpenChange={props.onOpenChange} width={760} title="Detalhes da transmissão">
      {/* Um componente à parte: fechado, nada disto é calculado a cada medição. */}
      <Details />
    </Modal>
  );
}

function Details() {
  const { stats, history, system, quality, logPath } = useStreamStore();
  const viewers = useRoomStore((s) => {
    const me = s.members[0]?.id;
    return me ? s.members.filter((m) => watches(m, me)).length : 0;
  });

  return (
    <div className="flex flex-col gap-4 pb-6 text-left">
      <div className="flex flex-wrap items-center justify-center gap-2">
        <LiveBadge />
        <InfoBadge className="!bg-surface-3">{qualityLabel(quality, stats?.height)}</InfoBadge>
        {stats && <InfoBadge className="!bg-surface-3">{`${stats.width}×${stats.height}`}</InfoBadge>}
        <InfoBadge className="!bg-surface-3">{`${viewers} assistindo`}</InfoBadge>
      </div>

      {stats === null ? (
        <p className="py-10 text-center text-sm text-text-muted">Medindo…</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-3">
            <Metric
              icon={<Activity size={16} />}
              label="Quadros transmitidos"
              value={stats.fps.toFixed(0)}
              unit="fps"
              detail={`o jogo entrega ${stats.sourceFps.toFixed(0)} fps`}
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
              label="Latência (captura → tela)"
              value={stats.latencyMs.toFixed(0)}
              unit="ms"
              detail={`pior: ${stats.latencyP95Ms.toFixed(0)} ms`}
              values={history.map((h) => h.latencyMs)}
              color={stats.latencyMs > 50 ? "var(--color-warning)" : "#3ba0ff"}
              warn={stats.latencyMs > 50}
            />
            <Metric
              icon={<Cpu size={16} />}
              label="Tempo de encode"
              value={stats.encodeMs.toFixed(1)}
              unit="ms"
              detail={`${stats.dropped} quadros descartados`}
              values={history.map((h) => h.encodeMs)}
              color="#e06c9f"
            />
          </div>

          <section aria-label="Uso do computador" className="rounded-card bg-surface-1 p-4">
            <h3 className="mb-3 text-xs font-semibold tracking-wide text-text-faint uppercase">Uso do computador</h3>
            {system ? (
              <div className="flex flex-col gap-3">
                <Bar label="Encoder da GPU" percent={system.gpuEncode} />
                <Bar label="GPU (3D, o jogo)" percent={system.gpu3d} />
                <Bar label="CPU" percent={system.cpu} />
              </div>
            ) : (
              <p className="text-sm text-text-muted">Medindo…</p>
            )}
          </section>

          <AudioPanel stats={stats} />

          <p className="text-xs text-text-faint">
            Encoder: {stats.encoder}
            {logPath && (
              <>
                {" · "}Registro desta sessão (só no seu PC): <span className="break-all select-text">{logPath}</span>
              </>
            )}
          </p>
        </>
      )}
    </div>
  );
}

/** Um número grande com o gráfico do último minuto embaixo. */
export function Metric(props: {
  icon: ReactNode;
  label: string;
  value: string;
  unit: string;
  detail?: string;
  values: number[];
  color: string;
  warn?: boolean;
}) {
  return (
    <figure aria-label={props.label} className="overflow-hidden rounded-card bg-surface-1">
      <div className="px-4 pt-3">
        <figcaption className="flex items-center gap-1.5 text-xs text-text-muted">
          {props.icon}
          {props.label}
        </figcaption>
        <p className={`mt-1 text-2xl font-semibold tabular-nums ${props.warn ? "text-warning" : ""}`}>
          {props.value} <span className="text-sm font-normal text-text-muted">{props.unit}</span>
        </p>
        <p className="h-4 text-xs text-text-faint">{props.detail}</p>
      </div>
      <Sparkline values={props.values} color={props.color} />
    </figure>
  );
}

const W = 240;
const H = 56;

/** Os pontos do gráfico: o mais novo na direita, sempre com `HISTORY_SIZE` posições. */
export function sparklinePoints(values: number[]): [number, number][] {
  const max = Math.max(1, ...values) * 1.15;
  const step = W / (HISTORY_SIZE - 1);
  const offset = HISTORY_SIZE - values.length;
  return values.map((v, i) => [(offset + i) * step, H - (Math.max(0, v) / max) * H]);
}

/**
 * Linha do último minuto com a área preenchida. O caminho troca de forma com uma transição (o
 * Chromium anima o `d`), então o gráfico desliza em vez de pular a cada medição.
 */
function Sparkline(props: { values: number[]; color: string }) {
  const points = sparklinePoints(props.values);
  if (points.length < 2) return <div className="h-14" />;
  const line = points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = `${line} L${W},${H} L${points[0][0].toFixed(1)},${H} Z`;
  const id = `sparkline-${props.color.replace(/[^a-z0-9]/gi, "")}`;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden className="block h-14 w-full">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={props.color} stopOpacity="0.35" />
          <stop offset="1" stopColor={props.color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={area} fill={`url(#${id})`} className="sparkline" />
      <path
        d={line}
        fill="none"
        stroke={props.color}
        strokeWidth="2"
        vectorEffect="non-scaling-stroke"
        className="sparkline"
      />
    </svg>
  );
}

/** Barra de uso (0–100%) que cresce e encolhe animando; fica amarela e depois vermelha quando aperta. */
export function Bar(props: { label: string; percent: number }) {
  const percent = Math.round(Math.min(100, Math.max(0, props.percent)));
  const color = percent > 90 ? "bg-live" : percent > 70 ? "bg-warning" : "bg-online";
  return (
    <div>
      <div className="mb-1 flex justify-between text-xs">
        <span className="text-text-muted">{props.label}</span>
        <span className="font-semibold tabular-nums">{percent}%</span>
      </div>
      <div
        role="meter"
        aria-label={props.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="h-2 overflow-hidden rounded-full bg-surface-0"
      >
        <div
          className={`h-full rounded-full transition-[width,background-color] duration-500 ease-out ${color}`}
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

/** O som que vai junto: o nível agora, kbps e perdas (ou por que está sem som). */
function AudioPanel(props: { stats: StreamStats }) {
  const { stats } = props;
  const listening = useStreamStore((s) => s.listening);
  const setListening = useStreamStore((s) => s.setListening);
  return (
    <section aria-label="Som da transmissão" className="rounded-card bg-surface-1 p-4">
      <h3 className="mb-3 flex items-center gap-1.5 text-xs font-semibold tracking-wide text-text-faint uppercase">
        <Volume2 size={14} /> Som da transmissão
      </h3>
      {stats.audioActive ? (
        <>
          <Bar label="Nível do som do jogo" percent={stats.audioLevel * 100} />
          <p className="mt-2 text-xs text-text-muted tabular-nums">
            {stats.audioKbps} kbps{stats.audioLost > 0 ? ` · ${stats.audioLost} pacotes perdidos` : ""}
          </p>
          <div className="mt-3 border-t border-white/[0.06] pt-3">
            <Switch
              label="Ouvir o som da prévia"
              hint="O som do jogo sai duas vezes (o original e o da prévia). Use fone para conferir."
              checked={listening}
              onChange={(on) => void setListening(on)}
            />
          </div>
        </>
      ) : (
        <p className="text-sm text-text-muted">
          {stats.audioError ?? "Sem som: ligue “Compartilhar áudio da transmissão” no menu da live."}
        </p>
      )}
    </section>
  );
}
