import { type CopyChoice, useDmStore } from "../dm/dm-store";

const CHOICES: { value: CopyChoice; label: string; hint: string }[] = [
  { value: "ask", label: "Perguntar", hint: 'Pergunta a cada envio, até você marcar "lembrar minha escolha".' },
  {
    value: "keep",
    label: "Guardar uma cópia no SquadView",
    hint: "A conversa continua mostrando o arquivo mesmo se você mover ou apagar o original.",
  },
  {
    value: "send",
    label: "Só mandar, sem cópia",
    hint: "Nenhuma cópia: a conversa aponta para o arquivo onde ele está.",
  },
];

/** Mensagens: o que fazer com os anexos que você manda (a escolha do modal da primeira vez). */
export function MessagesSection() {
  const choice = useDmStore((s) => s.copyChoice);
  const setChoice = useDmStore((s) => s.setCopyChoice);
  return (
    <section aria-labelledby="mensagens" className="flex flex-col gap-3">
      <h3 id="mensagens" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Anexos que você manda
      </h3>
      <div role="radiogroup" aria-label="Anexos que você manda" className="flex flex-col gap-2">
        {CHOICES.map((c) => (
          <button
            key={c.value}
            type="button"
            role="radio"
            aria-checked={choice === c.value}
            onClick={() => setChoice(c.value)}
            className={`flex items-start gap-3 rounded-lg border p-3 text-left transition-colors ${
              choice === c.value
                ? "border-brand bg-brand/10"
                : "border-white/[0.06] bg-surface-1 hover:bg-surface-hover"
            }`}
          >
            <span
              aria-hidden
              className={`mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border-2 ${
                choice === c.value ? "border-brand" : "border-text-faint"
              }`}
            >
              {choice === c.value && <span className="size-2 rounded-full bg-brand" />}
            </span>
            <span>
              <span className="block text-sm font-medium">{c.label}</span>
              <span className="block text-xs text-text-muted">{c.hint}</span>
            </span>
          </button>
        ))}
      </div>
      <p className="text-xs text-text-faint">
        As conversas vão direto entre os dois PCs e o histórico fica só neste PC. Fotos pequenas que chegam aparecem
        sozinhas; arquivos grandes e vídeos esperam você clicar em Baixar.
      </p>
    </section>
  );
}
