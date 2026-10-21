import { Play } from "lucide-react";
import type { CSSProperties } from "react";
import { Switch } from "../ui/Switch";
import { Tooltip } from "../ui/Tooltip";
import { SOUND_OPTIONS, sound, useSoundStore } from "./sound-store";

/** Sons: o volume dos avisos e quais tocam (como "Notificações › Sons" do Discord). */
export function SoundsSection() {
  const { volume, off, setVolume, setEnabled } = useSoundStore();
  return (
    <section aria-labelledby="sons" className="flex flex-col gap-3">
      <h3 id="sons" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Sons
      </h3>
      <div>
        <div className="flex items-center justify-between gap-3">
          <label htmlFor="volume-sons" className="text-sm font-medium">
            Volume dos avisos
          </label>
          <span className="font-mono text-sm text-text-muted tabular-nums">{volume}%</span>
        </div>
        <input
          id="volume-sons"
          type="range"
          min={0}
          max={100}
          step={5}
          value={volume}
          onChange={(e) => setVolume(Number(e.target.value))}
          // Solta o controle: toca um som para ouvir como ficou.
          onPointerUp={() => sound("join")}
          style={{ "--fill": `${volume}%` } as CSSProperties}
          className="glass-slider mt-3 w-full"
        />
        <p className="mt-2 text-xs text-text-faint">Em 0% nenhum aviso toca.</p>
      </div>
      <div className="mt-3 flex flex-col gap-4">
        {SOUND_OPTIONS.map((o) => {
          const on = !o.cues.some((c) => off.includes(c));
          return (
            <div key={o.label} className="flex items-center gap-2">
              <Tooltip label="Ouvir" side="top">
                <button
                  type="button"
                  aria-label={`Ouvir: ${o.label}`}
                  disabled={!on || volume === 0}
                  onClick={() => o.cues.forEach((c, i) => setTimeout(() => sound(c), i * 600))}
                  className="rounded p-1.5 text-text-faint transition-colors hover:bg-surface-hover hover:text-text disabled:pointer-events-none disabled:opacity-40"
                >
                  <Play size={14} />
                </button>
              </Tooltip>
              <div className="min-w-0 flex-1">
                <Switch label={o.label} checked={on} onChange={(v) => setEnabled(o.cues, v)} />
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
