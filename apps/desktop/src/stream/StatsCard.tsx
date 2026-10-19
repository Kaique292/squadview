import { Switch } from "../ui/Switch";
import { useStreamStore } from "./stream-store";

/** Detalhes da transmissão em andamento (a gaveta embaixo do vídeo). */
export function StatsCard() {
  const stats = useStreamStore((s) => s.stats);
  const system = useStreamStore((s) => s.system);
  const logPath = useStreamStore((s) => s.logPath);
  const listening = useStreamStore((s) => s.listening);
  const setListening = useStreamStore((s) => s.setListening);

  const latencyHigh = stats !== null && stats.latencyMs > 50;

  return (
    <section aria-label="Estatísticas da transmissão" className="w-full rounded-card bg-surface-1 p-4 text-left">
      {stats === null ? (
        <p className="text-sm text-text-muted">Medindo…</p>
      ) : (
        <>
          <dl className="grid grid-cols-3 gap-3 xl:grid-cols-5">
            <Stat
              label="Quadros do jogo"
              value={stats.sourceFps.toFixed(0)}
              unit="fps"
              detail="limitado ao refresh do monitor"
            />
            <Stat label="Quadros transmitidos" value={stats.fps.toFixed(0)} unit="fps" />
            <Stat label="Bitrate" value={(stats.bitrateKbps / 1000).toFixed(1)} unit="Mbps" />
            <Stat label="Tempo de encode" value={stats.encodeMs.toFixed(1)} unit="ms" />
            <Stat
              label="Latência (captura → tela)"
              value={stats.latencyMs.toFixed(0)}
              unit="ms"
              detail={`pior: ${stats.latencyP95Ms.toFixed(0)} ms`}
              warn={latencyHigh}
            />
            <Stat label="Quadros descartados" value={String(stats.dropped)} unit="" />
            <Stat label="Resolução" value={`${stats.width}×${stats.height}`} unit="" />
            {system && (
              <>
                <Stat label="GPU (3D)" value={system.gpu3d.toFixed(0)} unit="%" />
                <Stat label="Encoder da GPU" value={system.gpuEncode.toFixed(0)} unit="%" />
                <Stat label="CPU" value={system.cpu.toFixed(0)} unit="%" />
              </>
            )}
          </dl>
          <AudioMeter
            active={stats.audioActive}
            error={stats.audioError}
            level={stats.audioLevel}
            kbps={stats.audioKbps}
            lost={stats.audioLost}
          />
          {stats.audioActive && (
            <div className="mt-3 rounded-md bg-surface-2 p-3">
              <Switch
                label="Ouvir o som da prévia"
                hint="O som do jogo sai duas vezes (o original e o da prévia). Use fone para conferir."
                checked={listening}
                onChange={(on) => void setListening(on)}
              />
            </div>
          )}
          <p className="mt-4 text-xs text-text-faint">Encoder: {stats.encoder}</p>
        </>
      )}
      <p className="mt-2 text-xs text-text-faint">Dois cliques ou F11 no vídeo alternam a tela cheia; Esc sai.</p>
      {logPath && (
        <p className="mt-1 text-xs break-all text-text-faint">
          Registro desta sessão (fica só no seu PC): <span className="select-text">{logPath}</span>
        </p>
      )}
    </section>
  );
}

/** Medidor do áudio: barra com o pico do último meio segundo, bitrate e pacotes perdidos. */
function AudioMeter(props: { active: boolean; error: string | null; level: number; kbps: number; lost: number }) {
  if (!props.active) {
    return (
      <p className="mt-3 rounded-md bg-surface-2 px-3 py-2 text-xs text-text-muted">
        {props.error ?? "Sem som: escolha um aplicativo na hora de transmitir para o áudio ir junto."}
      </p>
    );
  }
  const percent = Math.round(Math.min(1, props.level) * 100);
  return (
    <div className="mt-3 rounded-md bg-surface-2 p-3">
      <div className="mb-1.5 flex justify-between text-xs text-text-muted">
        <span>Áudio do jogo</span>
        <span className="tabular-nums">
          {props.kbps} kbps{props.lost > 0 ? ` · ${props.lost} perdidos` : ""}
        </span>
      </div>
      <div
        role="meter"
        aria-label="Nível do áudio"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="h-2 overflow-hidden rounded-full bg-surface-0"
      >
        <div
          className={`h-full rounded-full transition-[width] duration-150 ease-out ${percent > 95 ? "bg-live" : "bg-online"}`}
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

function Stat(props: { label: string; value: string; unit: string; detail?: string; warn?: boolean }) {
  return (
    <div className="rounded-md bg-surface-2 p-3">
      <dt className="text-xs text-text-muted">{props.label}</dt>
      <dd className={`mt-1 text-xl font-semibold tabular-nums ${props.warn ? "text-warning" : ""}`}>
        {props.value} <span className="text-sm font-normal text-text-muted">{props.unit}</span>
      </dd>
      {props.detail && <dd className="text-xs text-text-faint">{props.detail}</dd>}
    </div>
  );
}
