import { type FormEvent, useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";

interface NameDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  label: string;
  initial: string;
  onSave: (name: string) => void;
  /** Texto do botão (padrão: "Salvar"). */
  confirm?: string;
  /** Botão vermelho (apagar algo). */
  danger?: boolean;
  /** Só libera o botão quando o texto passa (ex.: digitar o nome para apagar). */
  canSave?: (name: string) => boolean;
}

/** Modal com um campo só, para dar nome a algo (a sala, o nosso apelido). Até 32 caracteres. */
export function NameDialog(props: NameDialogProps) {
  return (
    <Modal
      open={props.open}
      onOpenChange={props.onOpenChange}
      width={420}
      title={props.title}
      description={props.description}
      footer={
        <>
          <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
          <button
            type="submit"
            form="name-form"
            className={`rounded-md px-5 py-2 text-sm font-semibold transition-colors disabled:opacity-50 ${
              props.danger ? "bg-live text-white hover:bg-live/85" : "bg-brand hover:bg-brand-hover"
            }`}
          >
            {props.confirm ?? "Salvar"}
          </button>
        </>
      }
    >
      {/* Remonta a cada abertura: o campo sempre começa do nome atual. */}
      {props.open && <NameForm key={props.initial} {...props} />}
    </Modal>
  );
}

function NameForm(props: NameDialogProps) {
  const [value, setValue] = useState(props.initial);
  const clean = value.trim();

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!clean || (props.canSave && !props.canSave(clean))) return;
    if (clean !== props.initial) props.onSave(clean);
    props.onOpenChange(false);
  }

  return (
    <form id="name-form" onSubmit={submit} className="flex flex-col gap-1.5 pb-1 text-left">
      <label className="flex flex-col gap-1.5">
        <span className="text-xs font-semibold tracking-wide text-text-muted uppercase">{props.label}</span>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          maxLength={32}
          autoFocus
          required
          className="rounded-md bg-surface-0 px-3 py-2 text-text focus:ring-2 focus:ring-brand focus:outline-none"
        />
      </label>
    </form>
  );
}
