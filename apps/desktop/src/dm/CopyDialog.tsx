import { Archive, Send } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Modal } from "../ui/Modal";
import { useDmStore } from "./dm-store";

/**
 * Primeira vez que se manda um anexo: guardar uma cópia no SquadView, ou só mandar (o arquivo
 * fica onde está e a conversa aponta para ele, sem cópia nem cache). "Lembrar minha escolha" não
 * pergunta de novo; dá para mudar em Configurações → Mensagens.
 */
export function CopyDialog() {
  const pending = useDmStore((s) => s.pendingFiles);
  const resolveCopy = useDmStore((s) => s.resolveCopy);
  const [remember, setRemember] = useState(false);
  const count = pending?.paths.length ?? 0;
  return (
    <Modal
      open={pending !== null}
      onOpenChange={(open) => !open && void resolveCopy(null, false)}
      width={480}
      title={count > 1 ? `Mandar ${count} arquivos` : "Mandar arquivo"}
      description="Quer guardar uma cópia no SquadView também?"
    >
      <div className="flex flex-col gap-2 text-left">
        <Choice
          icon={<Archive size={20} />}
          title="Guardar uma cópia"
          hint="O SquadView guarda uma cópia: a conversa continua mostrando o arquivo mesmo se você mover ou apagar o original."
          onClick={() => void resolveCopy(true, remember)}
        />
        <Choice
          icon={<Send size={20} />}
          title="Só mandar"
          hint="Nenhuma cópia: a conversa aponta para o arquivo onde ele está. Se você apagar o original, ele some daqui também."
          onClick={() => void resolveCopy(false, remember)}
        />
        <label className="mt-2 flex cursor-pointer items-center gap-2 text-sm text-text-muted">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            className="size-4 accent-brand"
          />
          Lembrar minha escolha (dá para mudar em Configurações → Mensagens)
        </label>
      </div>
    </Modal>
  );
}

function Choice(props: { icon: ReactNode; title: string; hint: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      className="flex items-start gap-3 rounded-lg border border-white/[0.06] bg-surface-1 p-3 text-left transition-colors hover:border-brand hover:bg-brand/10"
    >
      <span className="mt-0.5 text-text-muted">{props.icon}</span>
      <span>
        <span className="block font-semibold">{props.title}</span>
        <span className="block text-sm text-text-muted">{props.hint}</span>
      </span>
    </button>
  );
}
