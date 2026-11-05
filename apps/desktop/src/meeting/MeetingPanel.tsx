import { Download, Loader2, NotebookPen, Square, Trash2, TriangleAlert, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { useAccountStore } from "../account/account-store";
import { memberPhoto, useRoomStore } from "../room/room-store";
import { Avatar } from "../ui/Avatar";
import { ScrollArea } from "../ui/ScrollArea";
import { Select } from "../ui/Select";
import { Tooltip } from "../ui/Tooltip";
import { useVoiceStore } from "../voice/voice-store";
import { type MeetingLine, MODEL_LABELS, type ModelKind, useMeetingStore } from "./meeting-store";

const LANGUAGES = [
  { value: "pt", label: "Português" },
  { value: "auto", label: "Detectar sozinho" },
];

function megabytes(bytes: number) {
  return `${Math.round(bytes / 1_000_000)} MB`;
}

/** "14:32" (e a data, se não for hoje). */
export function lineTime(atMs: number, now = new Date()) {
  const at = new Date(atMs);
  const time = at.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
  return at.toDateString() === now.toDateString()
    ? time
    : `${at.toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit" })} ${time}`;
}

/**
 * A aba Reunião, ao lado da sala (ADR 0032): o que falamos vira texto, com foto, nome e hora.
 * Neste passo só a nossa voz; as falas dos outros e a exportação vêm depois.
 */
export function MeetingPanel() {
  const { cpuSupported, models, prefs, download, transcribing, starting, lines, setOpen } = useMeetingStore();
  const model = models.find((m) => m.kind === prefs.model);

  return (
    <aside
      aria-label="Reunião"
      className="flex h-full w-[340px] shrink-0 flex-col border-l border-white/[0.06] bg-glass-2 text-left"
    >
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-black/30 px-4 font-semibold">
        <NotebookPen size={18} className="text-text-muted" />
        <span className="flex-1">Reunião</span>
        {transcribing && (
          <span className="flex items-center gap-1.5 text-xs font-medium text-live">
            <span className="size-2 animate-pulse rounded-full bg-live" />
            Transcrevendo
          </span>
        )}
        <Tooltip label="Fechar" side="bottom">
          <button
            type="button"
            aria-label="Fechar a aba Reunião"
            onClick={() => setOpen(false)}
            className="grid size-8 place-items-center rounded-md text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
          >
            <X size={18} />
          </button>
        </Tooltip>
      </header>

      <Transcript lines={lines} transcribing={transcribing} />

      <footer className="flex shrink-0 flex-col gap-3 border-t border-black/30 p-4">
        {transcribing ? (
          <button
            type="button"
            onClick={() => void useMeetingStore.getState().stop()}
            className="flex h-10 items-center justify-center gap-2 rounded-md bg-surface-3 text-sm font-medium transition-colors hover:bg-surface-hover"
          >
            <Square size={14} className="fill-current" />
            Parar de transcrever
          </button>
        ) : cpuSupported === null ? (
          // O status do modelo ainda não chegou do Rust: nada de oferecer download à toa.
          <p className="flex items-center justify-center gap-2 text-sm text-text-faint">
            <Loader2 size={16} className="animate-spin" />
            Conferindo o modelo…
          </p>
        ) : cpuSupported === false ? (
          <p role="alert" className="flex items-start gap-2 text-sm text-warning">
            <TriangleAlert size={16} className="mt-0.5 shrink-0" />O processador deste PC não tem as instruções que a
            transcrição usa (AVX2).
          </p>
        ) : download ? (
          <DownloadProgress />
        ) : !model?.installed ? (
          <ModelSetup />
        ) : (
          <>
            <Select
              label="Idioma da fala"
              value={prefs.language ?? "auto"}
              options={LANGUAGES}
              onChange={(v) => useMeetingStore.getState().setPrefs({ language: v === "auto" ? null : v })}
            />
            <button
              type="button"
              disabled={starting}
              onClick={() => void useMeetingStore.getState().start()}
              className="flex h-10 items-center justify-center gap-2 rounded-md bg-brand text-sm font-medium text-white transition-colors hover:bg-brand-hover disabled:opacity-60"
            >
              {starting && <Loader2 size={16} className="animate-spin" />}
              {starting ? "Carregando o modelo…" : "Transcrever minha voz"}
            </button>
            <button
              type="button"
              onClick={() => void useMeetingStore.getState().deleteModel(prefs.model)}
              className="flex items-center justify-center gap-1.5 text-xs text-text-faint transition-colors hover:text-text-muted"
            >
              <Trash2 size={12} />
              Apagar o modelo baixado ({megabytes(model.size)})
            </button>
          </>
        )}
      </footer>
    </aside>
  );
}

/** As falas, com a rolagem descendo sozinha enquanto estamos no fim. */
function Transcript(props: { lines: MeetingLine[]; transcribing: boolean }) {
  const viewport = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const members = useRoomStore((s) => s.members);
  const profile = useAccountStore((s) => s.profile);
  const speaking = useVoiceStore((s) => !!s.speaking.me);
  const me = members[0];

  useEffect(() => {
    const el = viewport.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [props.lines]);

  if (props.lines.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 px-8 text-center">
        <NotebookPen size={32} className="text-text-faint" />
        <p className="text-sm text-text-muted">
          {props.transcribing
            ? speaking
              ? "Ouvindo…"
              : "Fale alguma coisa: o texto aparece aqui quando você der uma pausa."
            : "O que for dito na reunião vira texto aqui, com o nome e a hora de quem falou."}
        </p>
        <p className="text-xs text-text-faint">A transcrição roda no seu PC, e o áudio não sai dele.</p>
      </div>
    );
  }

  return (
    <ScrollArea
      className="min-h-0 flex-1"
      viewportClassName="py-3"
      viewportRef={viewport}
      onViewportScroll={(el) => {
        atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
    >
      <ol aria-label="Transcrição" className="flex flex-col">
        {props.lines.map((line) => {
          const name = me?.name ?? "Você";
          return (
            <li key={line.id} className="flex gap-3 px-4 py-1.5 hover:bg-black/10">
              <Avatar name={name} size={32} src={me ? memberPhoto(me, true, profile, 64) : undefined} />
              <div className="min-w-0 flex-1">
                <p className="flex items-baseline gap-2">
                  <span className="truncate text-sm font-semibold">{name}</span>
                  <time dateTime={new Date(line.atMs).toISOString()} className="shrink-0 text-xs text-text-faint">
                    {lineTime(line.atMs)}
                  </time>
                </p>
                {line.kind === "text" ? (
                  <p className="text-sm break-words text-text">{line.text}</p>
                ) : line.kind === "skipped" ? (
                  <p className="text-sm text-text-faint italic">[trecho não transcrito]</p>
                ) : (
                  <p className="text-sm text-warning italic" title={line.error}>
                    [não deu para transcrever este trecho]
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </ScrollArea>
  );
}

/** Primeira vez: escolher o modelo e baixar. */
function ModelSetup() {
  const { models, prefs, setPrefs, downloadModel } = useMeetingStore();
  const options = models.map((m) => ({
    value: m.kind,
    label: `${MODEL_LABELS[m.kind]} (${megabytes(m.size)})${m.installed ? " · baixado" : ""}`,
  }));
  const size = models.find((m) => m.kind === prefs.model)?.size ?? 0;
  return (
    <>
      <p className="text-sm text-text-muted">
        Para transcrever, o SquadView baixa uma vez o modelo de voz ({megabytes(size)}). Ele fica só no seu PC.
      </p>
      {options.length > 0 && (
        <Select
          label="Modelo de transcrição"
          value={prefs.model}
          options={options}
          onChange={(v) => setPrefs({ model: v as ModelKind })}
        />
      )}
      <button
        type="button"
        disabled={models.length === 0}
        onClick={() => void downloadModel(prefs.model)}
        className="flex h-10 items-center justify-center gap-2 rounded-md bg-brand text-sm font-medium text-white transition-colors hover:bg-brand-hover disabled:opacity-60"
      >
        <Download size={16} />
        Baixar o modelo
      </button>
    </>
  );
}

function DownloadProgress() {
  const download = useMeetingStore((s) => s.download);
  const cancel = useMeetingStore((s) => s.cancelDownload);
  if (!download) return null;
  const pct = download.total > 0 ? Math.min(100, Math.round((download.received / download.total) * 100)) : 0;
  return (
    <div className="flex flex-col gap-2">
      <p className="flex items-baseline justify-between text-sm">
        <span>Baixando o modelo {MODEL_LABELS[download.kind]}…</span>
        <span className="text-xs text-text-faint">
          {megabytes(download.received)} de {megabytes(download.total)}
        </span>
      </p>
      <div
        role="progressbar"
        aria-label="Download do modelo"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        className="h-2 overflow-hidden rounded-full bg-surface-0"
      >
        <div className="h-full rounded-full bg-brand transition-[width]" style={{ width: `${pct}%` }} />
      </div>
      <button
        type="button"
        onClick={() => void cancel()}
        className="self-end text-xs text-text-faint transition-colors hover:text-text-muted"
      >
        Cancelar
      </button>
    </div>
  );
}
