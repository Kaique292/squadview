import { Loader2 } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";
import { displayName, useAccountStore } from "../account/account-store";
import { savedName, useRoomStore } from "./room-store";

type Mode = "create" | "join";

/**
 * Criar uma sala ou entrar com o código que um amigo mandou.
 * `trigger` é o botão que abre (o modal fecha sozinho quando entra na sala).
 */
export function RoomDialog(props: {
  trigger?: ReactNode;
  initialMode?: Mode;
  /** Código já preenchido (convite por link). */
  initialCode?: string;
  /** Aberto por fora (convite), em vez de pelo botão. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const { joining, joinError: error, join, clearError } = useRoomStore();
  const [ownOpen, setOwnOpen] = useState(false);
  const open = props.open ?? ownOpen;
  const setOpen = (next: boolean) => {
    setOwnOpen(next);
    props.onOpenChange?.(next);
  };
  const [mode, setMode] = useState<Mode>(props.initialMode ?? "create");
  const [name, setName] = useState(savedName);
  const [code, setCode] = useState(props.initialCode ?? "");
  const profile = useAccountStore((s) => s.profile);

  function onOpenChange(next: boolean) {
    setOpen(next);
    if (next) {
      setMode(props.initialMode ?? "create");
      setCode(props.initialCode ?? "");
      clearError();
      // Sem nome salvo: sugere o do Discord.
      if (!name.trim() && profile) setName(displayName(profile));
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    const ok = await join(mode === "join" ? code : null, name);
    if (ok) setOpen(false);
  }

  const canSubmit = name.trim().length > 0 && (mode === "create" || code.replace(/[\s-]/g, "").length === 6);

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      width={440}
      trigger={props.trigger}
      title={mode === "create" ? "Criar uma sala" : "Entrar numa sala"}
      description={
        mode === "create"
          ? "Você ganha um código de 6 letras para mandar para a galera."
          : "Digite o código que seu amigo mandou."
      }
      footer={
        <>
          <button
            type="button"
            onClick={() => setMode(mode === "create" ? "join" : "create")}
            className="text-sm text-text-muted hover:text-text hover:underline"
          >
            {mode === "create" ? "Tenho um código" : "Criar uma sala nova"}
          </button>
          <div className="flex gap-2">
            <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
            <button
              type="submit"
              form="room-form"
              disabled={!canSubmit || joining}
              className="flex items-center gap-2 rounded-md bg-brand px-5 py-2 text-sm font-semibold transition-colors hover:bg-brand-hover disabled:opacity-50"
            >
              {joining && <Loader2 size={16} className="animate-spin" />}
              {mode === "create" ? "Criar sala" : "Entrar"}
            </button>
          </div>
        </>
      }
    >
      <form id="room-form" onSubmit={submit} className="flex flex-col gap-4 pb-1 text-left">
        <label className="flex flex-col gap-1.5">
          <span className="text-xs font-semibold tracking-wide text-text-muted uppercase">Seu nome na sala</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={32}
            placeholder="Como a galera te chama"
            autoFocus={mode === "create"}
            className="rounded-md bg-surface-0 px-3 py-2 text-text placeholder:text-text-faint focus:ring-2 focus:ring-brand focus:outline-none"
          />
        </label>
        {mode === "join" && (
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-semibold tracking-wide text-text-muted uppercase">Código da sala</span>
            <input
              value={code}
              onChange={(e) => setCode(e.target.value.toUpperCase())}
              maxLength={8}
              placeholder="B8RJ2Q"
              autoFocus
              spellCheck={false}
              className="rounded-md bg-surface-0 px-3 py-2 font-mono text-lg tracking-[0.3em] text-text uppercase placeholder:tracking-[0.3em] placeholder:text-text-faint focus:ring-2 focus:ring-brand focus:outline-none"
            />
          </label>
        )}
        {error && (
          <p role="alert" className="rounded-md bg-live/15 px-3 py-2 text-sm">
            {error}
          </p>
        )}
      </form>
    </Modal>
  );
}
