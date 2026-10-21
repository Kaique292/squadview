import { AppWindow, ChevronLeft, Loader2, Monitor, Radio } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";
import { TabContent, Tabs } from "../ui/Tabs";
import { roomTitle, useRoomStore } from "../room/room-store";
import { AudioSection } from "./AudioSection";
import { StreamIllustration } from "./StreamIllustration";
import {
  estimatedMbps,
  type Resolution,
  type Source,
  sourceKey,
  type StreamQuality,
  useStreamStore,
} from "./stream-store";

type Tab = "apps" | "screens";

const TABS = [
  ["apps", "Aplicativos"],
  ["screens", "Telas"],
] as const satisfies readonly (readonly [Tab, string])[];

const RESOLUTIONS: { value: Resolution; label: string }[] = [
  { value: "720", label: "720p" },
  { value: "1080", label: "1080p" },
  { value: "1440", label: "1440p" },
  { value: "source", label: "Fonte" },
];
const FRAME_RATES: StreamQuality["fps"][] = [15, 30, 60];

/**
 * Botão "Transmitir minha tela" + modal em dois passos: escolher a fonte, depois a qualidade.
 * `trigger` troca o botão padrão (ex.: o botão do painel da sala na barra lateral).
 */
export function StreamDialog(props: { trigger?: ReactNode } = {}) {
  const { sources, thumbnails, loadingSources, status, error, quality, loadSources, setQuality, start } =
    useStreamStore();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>("apps");
  const [selected, setSelected] = useState<Source | null>(null);
  const [audioPid, setAudioPid] = useState<number | null>(null);

  /** Janela: o som do próprio programa vai junto por padrão. Tela: sem som até escolher. */
  function select(source: Source) {
    setSelected(source);
    setAudioPid(source.id.kind === "window" ? source.processId : null);
  }

  function onOpenChange(next: boolean) {
    setOpen(next);
    if (next) {
      setSelected(null);
      void loadSources();
    }
  }

  async function goLive() {
    if (!selected) return;
    await start(selected.id, quality, audioPid);
    if (useStreamStore.getState().status === "live") setOpen(false);
  }

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      width={selected ? 480 : 600}
      illustration={<StreamIllustration />}
      title="Transmitir minha tela"
      description={
        selected ? undefined : "Escolha o que transmitir e chame a galera para assistir. Tudo liberado, sem plano pago."
      }
      trigger={
        props.trigger ?? (
          <button
            type="button"
            className="rounded-md bg-brand px-4 py-2 font-medium transition-colors hover:bg-brand-hover"
          >
            Transmitir minha tela
          </button>
        )
      }
      footer={
        selected ? (
          <>
            <button
              type="button"
              onClick={() => setSelected(null)}
              className="flex items-center gap-1 rounded-md px-3 py-2 text-sm hover:underline"
            >
              <ChevronLeft size={16} /> Voltar
            </button>
            <button
              type="button"
              disabled={status === "starting"}
              onClick={goLive}
              className="flex items-center gap-2 rounded-md bg-brand px-5 py-2 text-sm font-semibold transition-colors hover:bg-brand-hover disabled:opacity-50"
            >
              {status === "starting" ? <Loader2 size={16} className="animate-spin" /> : <Radio size={16} />}
              Ficar ao vivo
            </button>
          </>
        ) : (
          <>
            <span />
            <div className="flex gap-2">
              <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
              <button type="button" disabled className="rounded-md bg-brand px-5 py-2 text-sm font-semibold opacity-50">
                Ficar ao vivo
              </button>
            </div>
          </>
        )
      }
    >
      {selected ? (
        <QualityStep
          source={selected}
          thumbnail={thumbnails[sourceKey(selected.id)]}
          quality={quality}
          onQuality={setQuality}
          onChange={() => setSelected(null)}
          error={error}
          audio={<AudioSection source={selected} sources={sources} audioPid={audioPid} onAudioPid={setAudioPid} />}
        />
      ) : (
        <div key="fonte" className="animate-step-in">
          <Tabs label="Tipo de fonte" tabs={TABS} value={tab} onChange={setTab} className="mb-4" />
          <TabContent value={tab} order={TABS.map(([id]) => id)}>
            {(t) => {
              const shown = sources.filter((s) => (t === "apps" ? s.id.kind === "window" : s.id.kind === "monitor"));
              return loadingSources ? (
                <p className="flex items-center justify-center gap-2 py-12 text-text-muted">
                  <Loader2 size={18} className="animate-spin" /> Procurando telas e janelas…
                </p>
              ) : shown.length === 0 ? (
                <p className="py-12 text-center text-sm text-text-faint">
                  {t === "apps" ? "Nenhum aplicativo aberto para transmitir." : "Nenhuma tela encontrada."}
                </p>
              ) : (
                <ul aria-label={t === "apps" ? "Aplicativos" : "Telas"} className="grid grid-cols-2 gap-4 pb-1">
                  {shown.map((s) => (
                    <li key={sourceKey(s.id)}>
                      <SourceCard source={s} thumbnail={thumbnails[sourceKey(s.id)]} onSelect={() => select(s)} />
                    </li>
                  ))}
                </ul>
              );
            }}
          </TabContent>
          {error && <ErrorBox message={error} />}
        </div>
      )}
    </Modal>
  );
}

/** Card com a miniatura e o nome. `thumbnail` indefinido = carregando; `null` = sem imagem. */
function SourceCard(props: { source: Source; thumbnail: string | null | undefined; onSelect: () => void }) {
  const { source, thumbnail } = props;
  const Icon = source.id.kind === "monitor" ? Monitor : AppWindow;
  return (
    <button
      type="button"
      onClick={props.onSelect}
      aria-label={source.name}
      className="group w-full rounded-lg p-1.5 text-left transition-colors hover:bg-surface-hover focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none"
    >
      <Thumb thumbnail={thumbnail} icon={<Icon size={28} />} />
      <span className="mt-2 block truncate text-center text-sm font-medium">{source.name}</span>
    </button>
  );
}

function Thumb(props: { thumbnail: string | null | undefined; icon: ReactNode; small?: boolean }) {
  const base = `grid aspect-video w-full place-items-center overflow-hidden rounded-md bg-surface-0 ring-1 ring-surface-3 transition-all group-hover:ring-2 group-hover:ring-brand`;
  if (props.thumbnail === undefined)
    return <div className={`${base} animate-pulse`} data-testid="miniatura-carregando" />;
  if (props.thumbnail === null) return <div className={`${base} text-text-faint`}>{props.icon}</div>;
  return (
    <div className={base}>
      <img src={props.thumbnail} alt="" className="h-full w-full object-cover" />
    </div>
  );
}

function QualityStep(props: {
  source: Source;
  thumbnail: string | null | undefined;
  quality: StreamQuality;
  onQuality: (q: StreamQuality) => void;
  onChange: () => void;
  error: string | null;
  audio: ReactNode;
}) {
  const { source, quality, onQuality } = props;
  const Icon = source.id.kind === "monitor" ? Monitor : AppWindow;
  return (
    <div key="qualidade" className="animate-step-in">
      <Section title="O que você está transmitindo">
        <div className="flex items-center gap-3 rounded-md bg-surface-1 p-2">
          <div className="w-20 shrink-0">
            <Thumb thumbnail={props.thumbnail} icon={<Icon size={18} />} />
          </div>
          <span className="min-w-0 flex-1 truncate font-medium">{source.name}</span>
          <button
            type="button"
            onClick={props.onChange}
            className="shrink-0 rounded-md bg-surface-3 px-3 py-1.5 text-sm font-medium hover:bg-surface-hover"
          >
            Trocar
          </button>
        </div>
      </Section>

      <Section title="Transmitindo para">
        <Destination />
      </Section>

      <Section title="Áudio">{props.audio}</Section>

      <Section title="Qualidade da transmissão">
        <div className="rounded-md bg-surface-1 p-3">
          <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2">
            <h4 className="text-xs font-semibold tracking-wide text-text-muted uppercase">Resolução</h4>
            <h4 className="text-xs font-semibold tracking-wide text-text-muted uppercase">Taxa de quadros</h4>
            <Segmented
              label="Resolução"
              options={RESOLUTIONS}
              value={quality.resolution}
              onChange={(resolution) => onQuality({ ...quality, resolution })}
            />
            <Segmented
              label="Taxa de quadros"
              options={FRAME_RATES.map((f) => ({ value: f, label: String(f) }))}
              value={quality.fps}
              onChange={(fps) => onQuality({ ...quality, fps })}
            />
          </div>
          <p className="mt-3 text-xs text-text-faint">
            Até ≈ {estimatedMbps(quality).toFixed(1)} Mbps de upload por pessoa assistindo.
            {quality.resolution === "source" && " Em “Fonte”, sai no tamanho original (até 4K)."}
          </p>
        </div>
      </Section>
      {props.error && <ErrorBox message={props.error} />}
    </div>
  );
}

/** Para quem vai a transmissão: a sala (se estivermos numa) ou só a prévia neste PC. */
function Destination() {
  const { code, roomName, members } = useRoomStore();
  const friends = Math.max(0, members.length - 1);
  return (
    <div className="flex items-center gap-3 rounded-md bg-surface-1 px-3 py-2.5">
      <Monitor size={18} className="shrink-0 text-text-muted" />
      <div className="min-w-0">
        {code ? (
          <>
            <p className="text-sm font-medium">{roomTitle(code, roomName)}</p>
            <p className="text-xs text-text-faint">
              {friends === 0
                ? "Ninguém mais na sala ainda: mande o código para a galera."
                : `${friends} ${friends === 1 ? "amigo pode" : "amigos podem"} assistir.`}
            </p>
          </>
        ) : (
          <>
            <p className="text-sm font-medium">Prévia local</p>
            <p className="text-xs text-text-faint">Só você vê. Entre numa sala para a galera assistir.</p>
          </>
        )}
      </div>
    </div>
  );
}

function Section(props: { title: string; children: ReactNode }) {
  return (
    <section className="mb-4">
      <h3 className="mb-2 text-xs font-semibold tracking-wide text-text-faint uppercase">{props.title}</h3>
      {props.children}
    </section>
  );
}

function Segmented<T extends string | number>(props: {
  label: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div role="radiogroup" aria-label={props.label} className="flex rounded-md bg-surface-0 p-0.5">
      {props.options.map((o) => {
        const active = o.value === props.value;
        return (
          <button
            key={String(o.value)}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => props.onChange(o.value)}
            className={`flex-1 rounded px-3 py-1.5 text-sm font-semibold whitespace-nowrap transition-colors ${
              active ? "bg-brand text-white" : "text-text-muted hover:bg-surface-hover hover:text-text"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

function ErrorBox(props: { message: string }) {
  return (
    <p role="alert" className="mt-3 rounded-md bg-live/15 px-3 py-2 text-sm text-text">
      {props.message}
    </p>
  );
}
