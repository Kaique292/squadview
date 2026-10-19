import { Volume1, Volume2, VolumeX } from "lucide-react";
import type { CSSProperties } from "react";
import { MAX_WATCH_VOLUME, useRoomStore } from "./room-store";

/**
 * Volume de quem estamos assistindo, como no Discord: o alto-falante liga e desliga o som e, com
 * o mouse em cima, abre um controle deslizante de 0 a 200% ao lado.
 */
export function VolumeControl() {
  const listening = useRoomStore((s) => s.watchListening);
  const volume = useRoomStore((s) => s.watchVolume);
  const setListening = useRoomStore((s) => s.setWatchListening);
  const setVolume = useRoomStore((s) => s.setWatchVolume);
  const muted = !listening || volume === 0;
  const Icon = muted ? VolumeX : volume < 50 ? Volume1 : Volume2;

  return (
    <div className="group/volume flex items-center">
      <div className="flex w-0 items-center overflow-hidden opacity-0 transition-all duration-200 ease-out group-hover/volume:w-36 group-hover/volume:opacity-100 focus-within:w-36 focus-within:opacity-100">
        <input
          type="range"
          aria-label="Volume da transmissão"
          aria-valuetext={`${volume}%`}
          min={0}
          max={MAX_WATCH_VOLUME}
          step={5}
          value={volume}
          onChange={(e) => {
            const v = Number(e.target.value);
            void setVolume(v);
            // Mexer no volume com o som desligado liga o som, como no Discord.
            if (!listening && v > 0) void setListening(true);
          }}
          style={{ "--fill": `${(volume / MAX_WATCH_VOLUME) * 100}%` } as CSSProperties}
          className="glass-slider mx-2 w-24"
        />
        <span className="w-10 text-right text-xs text-text-muted tabular-nums">{volume}%</span>
      </div>
      <button
        type="button"
        aria-label={listening ? "Silenciar" : "Ativar som"}
        aria-pressed={listening}
        onClick={() => void setListening(!listening)}
        className={`rounded-md p-2 transition-colors hover:bg-surface-hover ${muted ? "text-live" : "text-text-muted hover:text-text"}`}
      >
        <Icon size={18} />
      </button>
    </div>
  );
}
