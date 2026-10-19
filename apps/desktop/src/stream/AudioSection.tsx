import { Volume2, VolumeX } from "lucide-react";
import { Switch } from "../ui/Switch";
import type { Source } from "./stream-store";

interface AudioSectionProps {
  source: Source;
  /** Todas as fontes (para oferecer os aplicativos quando a fonte é uma tela). */
  sources: Source[];
  audioPid: number | null;
  onAudioPid: (pid: number | null) => void;
}

/**
 * Escolha do áudio: só o som de um aplicativo vai junto, nunca o da call.
 * Janela → o áudio do próprio programa (ligado por padrão). Tela → escolher de qual aplicativo.
 */
export function AudioSection(props: AudioSectionProps) {
  const { source, audioPid, onAudioPid } = props;

  if (source.id.kind === "window" && source.processId !== null) {
    const pid = source.processId;
    return (
      <div className="rounded-md bg-surface-1 p-3">
        <Switch
          label={`Transmitir o som de ${source.processName ?? source.name}`}
          hint="Só o som desse jogo ou aplicativo. A call e o resto do PC não vão junto."
          checked={audioPid === pid}
          onChange={(on) => onAudioPid(on ? pid : null)}
        />
      </div>
    );
  }

  // Um aplicativo por processo, sem repetir quem tem várias janelas.
  const apps = props.sources.filter(
    (s, i, all) =>
      s.id.kind === "window" && s.processId !== null && all.findIndex((o) => o.processId === s.processId) === i,
  );
  return (
    <div className="flex items-center gap-3 rounded-md bg-surface-1 p-3">
      {audioPid === null ? (
        <VolumeX size={18} className="shrink-0 text-text-muted" />
      ) : (
        <Volume2 size={18} className="shrink-0 text-online" />
      )}
      <label className="min-w-0 flex-1 text-sm">
        <span className="block font-medium">Som de qual aplicativo?</span>
        <span className="block text-xs text-text-faint">Transmitindo a tela, escolha o jogo para o som ir junto.</span>
      </label>
      <select
        aria-label="Som de qual aplicativo?"
        value={audioPid ?? ""}
        onChange={(e) => onAudioPid(e.target.value === "" ? null : Number(e.target.value))}
        className="max-w-[45%] shrink-0 rounded-md bg-surface-3 px-2 py-1.5 text-sm text-text focus:ring-2 focus:ring-brand focus:outline-none"
      >
        <option value="">Sem som</option>
        {apps.map((a) => (
          <option key={a.processId} value={a.processId!}>
            {a.processName ?? a.name}
          </option>
        ))}
      </select>
    </div>
  );
}
