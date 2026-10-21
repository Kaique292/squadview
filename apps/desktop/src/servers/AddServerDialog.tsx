import { type FormEvent, useState } from "react";
import { DiscordLoginButton } from "../account/DiscordLoginButton";
import { displayName, useAccountStore } from "../account/account-store";
import { useFriendsStore } from "../friends/friends-store";
import { Modal, ModalClose } from "../ui/Modal";
import { parseServerInvite, useServersStore } from "./servers-store";

type Mode = "create" | "join";

/**
 * Criar um servidor ou entrar num pelo convite (o "+" da barra de salas). Servidores precisam do
 * Discord: é a conta que diz quem é o dono e quem é membro. Sem servidor, ainda dá para abrir uma
 * sala rápida, como antes.
 */
export function AddServerDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Convite já preenchido (link de convite ou `squadview://servidor/...`). */
  initialInvite?: string;
  /** Abre a sala rápida (sem servidor) no lugar deste modal. */
  onQuickRoom?: () => void;
}) {
  const [mode, setMode] = useState<Mode>(props.initialInvite ? "join" : "create");
  const online = useFriendsStore((s) => s.status === "online");
  const profile = useAccountStore((s) => s.profile);
  return (
    <Modal
      open={props.open}
      onOpenChange={props.onOpenChange}
      width={460}
      title={mode === "create" ? "Criar um servidor" : "Entrar num servidor"}
      description={
        mode === "create"
          ? "Seu cantinho com a galera: salas que ficam sempre lá, mesmo vazias."
          : "Cole o link de convite que mandaram para você."
      }
      footer={
        <>
          <button
            type="button"
            onClick={() => setMode(mode === "create" ? "join" : "create")}
            className="text-sm text-text-muted hover:text-text hover:underline"
          >
            {mode === "create" ? "Tenho um convite" : "Criar um servidor"}
          </button>
          <div className="flex gap-2">
            <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
            {online && (
              <button
                type="submit"
                form="servidor-form"
                className="rounded-md bg-brand px-5 py-2 text-sm font-semibold transition-colors hover:bg-brand-hover"
              >
                {mode === "create" ? "Criar" : "Entrar"}
              </button>
            )}
          </div>
        </>
      }
    >
      {props.open && online ? (
        <ServerForm
          key={mode}
          mode={mode}
          initialName={profile ? `Servidor de ${displayName(profile)}` : ""}
          initialInvite={props.initialInvite ?? ""}
          onDone={() => props.onOpenChange(false)}
        />
      ) : (
        <div className="flex flex-col items-center gap-3 py-2 text-sm text-text-muted">
          <p>Servidores usam sua conta do Discord para saber quem é o dono e quem é membro.</p>
          {!profile ? <DiscordLoginButton /> : <p className="text-text-faint">Conectando aos amigos…</p>}
        </div>
      )}
      {props.onQuickRoom && (
        <button
          type="button"
          onClick={() => {
            props.onOpenChange(false);
            props.onQuickRoom?.();
          }}
          className="mt-4 w-full rounded-md border border-white/[0.06] px-3 py-2 text-left text-sm text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
        >
          <span className="font-medium text-text">Só uma sala rápida</span> — sem servidor, com um código de 6 letras.
        </button>
      )}
    </Modal>
  );
}

function ServerForm(props: { mode: Mode; initialName: string; initialInvite: string; onDone: () => void }) {
  const { create, join } = useServersStore();
  const [value, setValue] = useState(props.mode === "create" ? props.initialName : props.initialInvite);
  const invite = props.mode === "join" ? parseServerInvite(value) : null;
  const invalid = props.mode === "join" && value.trim() !== "" && !invite;

  function submit(e: FormEvent) {
    e.preventDefault();
    const clean = value.trim();
    if (!clean) return;
    if (props.mode === "create") void create(clean);
    else if (invite) void join(invite);
    else return;
    props.onDone();
  }

  return (
    <form id="servidor-form" onSubmit={submit} className="flex flex-col gap-1.5 text-left">
      <label className="flex flex-col gap-1.5">
        <span className="text-xs font-semibold tracking-wide text-text-muted uppercase">
          {props.mode === "create" ? "Nome do servidor" : "Link de convite"}
        </span>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          maxLength={props.mode === "create" ? 32 : 120}
          placeholder={props.mode === "create" ? "Turma do bairro" : "Cole o link do convite"}
          aria-invalid={invalid}
          autoFocus
          required
          className="rounded-md bg-surface-0 px-3 py-2 text-text focus:ring-2 focus:ring-brand focus:outline-none aria-[invalid=true]:ring-2 aria-[invalid=true]:ring-live"
        />
      </label>
      {invalid && <p className="text-xs text-live">Esse não parece um convite de servidor do SquadView.</p>}
    </form>
  );
}
