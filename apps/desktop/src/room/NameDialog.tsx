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
            className="rounded-md bg-brand px-5 py-2 text-sm font-semibold transition-colors hover:bg-brand-hover disabled:opacity-50"
          >
            Salvar
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
    if (!clean) return;
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
