import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PictureInPicture2, Volume2, VolumeX } from "lucide-react";
import { type CSSProperties, type MouseEvent, useEffect, useState } from "react";
import { LiveBadge } from "../ui/LiveBadge";
import { PIP_CONTROL_EVENT, PIP_STATE_EVENT, type PipControl, type PipPage, type PipState } from "./pip";

/** O volume máximo da live (o mesmo do menu dela). */
const MAX_VOLUME = 200;

/** Botão redondo dos controles, no tom dos botões em cima da live. */
const CONTROL =
  "grid size-8 shrink-0 place-items-center rounded-md bg-black/60 text-white transition-colors hover:bg-black/80";

/**
 * A janelinha do picture in picture (ADR 0021): o vídeo é desenhado por baixo desta página, que é
 * transparente; com o mouse em cima, aparecem quem transmite, o "AO VIVO" e os controles, como no
 * Discord: silenciar, volume e voltar para o app. Arrastar move a janela; dois cliques, volta.
 */
export function PipView({ page }: { page: PipPage }) {
  const { memberId } = page;
  const [sound, setSound] = useState({ listening: page.listening, volume: page.volume });

  // O som mudou pela janela principal (pelo menu da live): os controles acompanham.
  useEffect(() => {
    const off = listen<PipState>(PIP_STATE_EVENT, (e) => {
      if (e.payload.memberId === memberId) setSound({ listening: e.payload.listening, volume: e.payload.volume });
    });
    return () => void off.then((fn) => fn());
  }, [memberId]);

  function send(control: Omit<PipControl, "memberId">) {
    setSound((s) => ({ ...s, ...control }));
    void emitTo("main", PIP_CONTROL_EVENT, { memberId, ...control }).catch(() => {});
  }

  function back() {
    void invoke("room_set_pip", { memberId, pip: false }).catch(() => {});
  }

  function onMouseDown(e: MouseEvent<HTMLDivElement>) {
    if (e.button !== 0 || (e.target as Element).closest("button, input")) return;
    // Dois cliques voltam para o app; um, arrasta a janelinha.
    if (e.detail === 2) back();
    else
      void getCurrentWindow()
        .startDragging()
        .catch(() => {});
  }

  const { listening, volume } = sound;
  return (
    <div
      role="region"
      aria-label={`Live de ${page.name} em picture in picture`}
      onMouseDown={onMouseDown}
      className="group relative h-full w-full cursor-default overflow-hidden text-white select-none"
    >
      <div className="absolute inset-x-0 top-0 flex items-start gap-2 bg-gradient-to-b from-black/70 to-transparent p-2 pb-6 opacity-0 transition-opacity duration-200 group-hover:opacity-100 focus-within:opacity-100">
        <span className="min-w-0 truncate pt-1 text-sm font-semibold drop-shadow">{page.name}</span>
        <LiveBadge size="sm" className="mt-1.5" />
        <button
          type="button"
          aria-label="Voltar para o app"
          title="Voltar para o app"
          onClick={back}
          className={`${CONTROL} ml-auto`}
        >
          <PictureInPicture2 size={16} />
        </button>
      </div>

      <div className="absolute inset-x-0 bottom-0 flex items-center gap-2 bg-gradient-to-t from-black/70 to-transparent p-2 pt-6 opacity-0 transition-opacity duration-200 group-hover:opacity-100 focus-within:opacity-100">
        <button
          type="button"
          aria-label={listening ? "Silenciar a transmissão" : "Ligar o som da transmissão"}
          aria-pressed={!listening}
          title={listening ? "Silenciar" : "Ligar o som"}
          onClick={() => send({ listening: !listening })}
          className={CONTROL}
        >
          {listening ? <Volume2 size={16} /> : <VolumeX size={16} className="text-live" />}
        </button>
        <input
          type="range"
          aria-label="Volume da transmissão"
          aria-valuetext={`${volume}%`}
          min={0}
          max={MAX_VOLUME}
          step={5}
          value={volume}
          onChange={(e) => {
            const v = Number(e.target.value);
            // Mexer no volume com o som desligado liga o som, como no Discord.
            send(!listening && v > 0 ? { volume: v, listening: true } : { volume: v });
          }}
          style={{ "--fill": `${(volume / MAX_VOLUME) * 100}%` } as CSSProperties}
          className="glass-slider w-28 min-w-0"
        />
        <span className="w-10 text-xs text-white/80 tabular-nums">{volume}%</span>
      </div>
    </div>
  );
}
