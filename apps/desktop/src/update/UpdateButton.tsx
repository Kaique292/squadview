import { Download, Loader2 } from "lucide-react";
import { useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";
import { Tooltip } from "../ui/Tooltip";
import { parseNotes, useUpdateStore } from "./update-store";

/**
 * Atualização como no Discord: um ícone verde de download na barra de título quando há versão
 * nova. Ao clicar, um modal mostra as novidades e pede confirmação.
 */
export function UpdateButton() {
  const status = useUpdateStore((s) => s.status);
  const [open, setOpen] = useState(false);
  if (status.kind === "idle") return null;

  const label = status.kind === "downloading" ? "Baixando a atualização…" : `Atualização disponível: ${status.version}`;
  return (
    <UpdateDialog
      open={open}
      onOpenChange={setOpen}
      trigger={
        <Tooltip label={label} side="left">
          <button
            type="button"
            aria-label={label}
            className="grid h-full w-[46px] place-items-center text-online transition-colors hover:bg-white/10"
          >
            {status.kind === "downloading" ? (
              <Loader2 size={17} className="animate-spin" />
            ) : (
              <Download size={17} strokeWidth={2.5} />
            )}
          </button>
        </Tooltip>
      }
    />
  );
}

function UpdateDialog(props: { open: boolean; onOpenChange: (open: boolean) => void; trigger: React.ReactNode }) {
  const status = useUpdateStore((s) => s.status);
  const notes = useUpdateStore((s) => s.notes);
  const install = useUpdateStore((s) => s.install);
  if (status.kind === "idle") return null;
  const sections = parseNotes(notes);
  const downloading = status.kind === "downloading";

  return (
    <Modal
      open={props.open}
      onOpenChange={(open) => !downloading && props.onOpenChange(open)}
      trigger={props.trigger}
      width={460}
      title="Atualizar o SquadView"
      description={`A versão ${status.version} está pronta. O app fecha, instala e abre de novo sozinho, em alguns segundos.`}
      footer={
        <>
          <ModalClose
            disabled={downloading}
            className="rounded-md px-4 py-2 text-sm hover:underline disabled:opacity-50"
          >
            Agora não
          </ModalClose>
          <button
            type="button"
            onClick={() => void install()}
            disabled={downloading}
            className="flex items-center gap-2 rounded-md bg-online px-5 py-2 text-sm font-semibold text-white transition-colors hover:bg-online/85 disabled:opacity-70"
          >
            {downloading ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
            {downloading ? "Baixando…" : status.kind === "error" ? "Tentar de novo" : "Atualizar e reiniciar"}
          </button>
        </>
      }
    >
      <div className="flex flex-col gap-3 pb-1 text-left text-sm">
        {downloading && (
          <div>
            <div
              role="progressbar"
              aria-label="Baixando a atualização"
              aria-valuenow={status.percent ?? undefined}
              aria-valuemin={0}
              aria-valuemax={100}
              className="h-2 overflow-hidden rounded-full bg-surface-0"
            >
              <div
                className={`h-full rounded-full bg-online transition-[width] duration-300 ${status.percent === null ? "w-1/3 animate-pulse" : ""}`}
                style={status.percent === null ? undefined : { width: `${status.percent}%` }}
              />
            </div>
            <p className="mt-1.5 text-xs text-text-muted">
              {status.percent === null ? "Baixando…" : `Baixando… ${status.percent}%`}
            </p>
          </div>
        )}
        {status.kind === "error" && (
          <p role="alert" className="rounded-md bg-live/15 px-3 py-2">
            {status.message}
          </p>
        )}
        {sections.length > 0 ? (
          sections.map((s) => (
            <section key={s.title} className="rounded-md bg-surface-1 p-3">
              <h3 className="text-xs font-semibold tracking-wide text-text-faint uppercase">{s.title}</h3>
              <ul className="mt-2 flex list-disc flex-col gap-1 pl-5 text-text-muted">
                {s.items.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </section>
          ))
        ) : (
          <p className="text-text-muted">Correções e melhorias.</p>
        )}
      </div>
    </Modal>
  );
}
