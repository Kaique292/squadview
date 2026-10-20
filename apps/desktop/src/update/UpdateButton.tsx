import { Download, Loader2 } from "lucide-react";
import { useState } from "react";
import { useRoomStore } from "../room/room-store";
import { Modal, ModalClose } from "../ui/Modal";
import { Tooltip } from "../ui/Tooltip";
import { parseNotes, takeWhatsNew, useUpdateStore, type WhatsNew } from "./update-store";

/**
 * Atualização como no Discord: a setinha verde na barra de título quando há versão nova.
 * - Fora de uma sala, clicar já atualiza (a setinha gira enquanto baixa).
 * - Numa sala, avisa antes que vamos sair dela por alguns segundos; o app volta sozinho para ela.
 * - Depois de atualizar, as novidades da versão aparecem uma vez.
 */
export function UpdateButton() {
  const status = useUpdateStore((s) => s.status);
  const install = useUpdateStore((s) => s.install);
  const inRoom = useRoomStore((s) => s.code !== null);
  const [confirming, setConfirming] = useState(false);
  // As novidades da versão que acabou de ser instalada (uma vez só).
  const [whatsNew, setWhatsNew] = useState<WhatsNew | null>(() => takeWhatsNew());

  const downloading = status.kind === "downloading";
  const label =
    status.kind === "downloading"
      ? `Baixando a atualização${status.percent === null ? "…" : ` (${status.percent}%)`}`
      : status.kind === "error"
        ? "Não deu para atualizar. Clique para tentar de novo."
        : status.kind === "available"
          ? `Atualização disponível: ${status.version}`
          : "";

  function onClick() {
    if (downloading) return;
    if (inRoom) setConfirming(true);
    else void install();
  }

  return (
    <>
      {status.kind !== "idle" && (
        <Tooltip label={label} side="left">
          <button
            type="button"
            aria-label={label}
            onClick={onClick}
            className={`grid h-full w-[46px] place-items-center transition-colors hover:bg-white/10 ${
              status.kind === "error" ? "text-live" : "text-online"
            }`}
          >
            {downloading ? <Loader2 size={17} className="animate-spin" /> : <Download size={17} strokeWidth={2.5} />}
          </button>
        </Tooltip>
      )}
      <LeaveRoomDialog
        open={confirming}
        onOpenChange={setConfirming}
        onConfirm={() => {
          setConfirming(false);
          void install();
        }}
      />
      {whatsNew && <WhatsNewDialog news={whatsNew} onClose={() => setWhatsNew(null)} />}
    </>
  );
}

/** "Sair temporariamente da sala?", como o aviso do Discord ao atualizar num canal de voz. */
function LeaveRoomDialog(props: { open: boolean; onOpenChange: (open: boolean) => void; onConfirm: () => void }) {
  return (
    <Modal
      open={props.open}
      onOpenChange={props.onOpenChange}
      width={400}
      title="Sair temporariamente da sala?"
      description="Ao atualizar o SquadView enquanto está numa sala, você sai dela por alguns segundos. O app volta sozinho para a sala, já atualizado."
      footer={
        <div className="flex w-full flex-col gap-2">
          <ModalClose className="w-full rounded-md bg-surface-3 px-4 py-2.5 text-sm font-semibold transition-colors hover:bg-surface-hover">
            Cancelar
          </ModalClose>
          <button
            type="button"
            onClick={props.onConfirm}
            className="w-full rounded-md bg-live px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-live/85"
          >
            Atualizar mesmo assim!
          </button>
        </div>
      }
    >
      {null}
    </Modal>
  );
}

/** As novidades da versão nova, logo depois de atualizar. */
function WhatsNewDialog(props: { news: WhatsNew; onClose: () => void }) {
  const sections = parseNotes(props.news.notes);
  return (
    <Modal
      open
      onOpenChange={(open) => !open && props.onClose()}
      width={460}
      title={`Novidades da versão ${props.news.version}`}
      description="O SquadView acabou de ser atualizado."
      footer={
        <ModalClose className="rounded-md bg-brand px-5 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-hover">
          Entendi
        </ModalClose>
      }
    >
      <div className="flex flex-col gap-3 pb-1 text-left text-sm">
        {sections.length > 0 ? (
          sections.map((s) => (
            <section key={s.title}>
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
