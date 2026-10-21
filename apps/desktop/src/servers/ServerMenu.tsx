import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { LogOut, Plus, Settings, Trash2, UserPlus } from "lucide-react";
import { type ReactNode, useState } from "react";
import { useAccountStore } from "../account/account-store";
import { NameDialog } from "../room/NameDialog";
import { Modal, ModalClose } from "../ui/Modal";
import { useOverlayMenu } from "../ui/overlay";
import { ServerSettingsDialog } from "./ServerSettingsDialog";
import { myRole, type Server, useServersStore } from "./servers-store";

/** As peças do Radix são as mesmas no menu do nome do servidor e no do botão direito. */
type Kit = typeof DropdownMenu;
const KITS: Record<"dropdown" | "context", Kit> = {
  dropdown: DropdownMenu,
  context: ContextMenu as unknown as Kit,
};

export const SERVER_MENU_CONTENT =
  "z-50 w-56 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in";

/**
 * Os modais do menu do servidor (criar sala, configurações, apagar) e quem os abre. Fica com quem
 * mostra o menu, para continuarem abertos depois que o menu fecha.
 */
export function useServerDialogs(server: Server) {
  const act = useServersStore((s) => s.act);
  const [creating, setCreating] = useState(false);
  const [settings, setSettings] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const dialogs = (
    <>
      <NameDialog
        open={creating}
        onOpenChange={setCreating}
        title="Criar sala"
        description={`A sala fica em ${server.name} para todo mundo, mesmo vazia.`}
        label="Nome da sala"
        initial=""
        onSave={(name) => void act(server.id, { kind: "room-create", name })}
      />
      <ServerSettingsDialog server={server} open={settings} onOpenChange={setSettings} />
      <ConfirmDelete server={server} open={deleting} onOpenChange={setDeleting} />
    </>
  );
  return {
    dialogs,
    openCreate: () => setCreating(true),
    openSettings: () => setSettings(true),
    openDelete: () => setDeleting(true),
  };
}

/** O que dá para fazer num servidor, conforme o nosso papel nele (como o menu do Discord). */
export function ServerMenuItems(props: {
  kind: "dropdown" | "context";
  server: Server;
  dialogs: ReturnType<typeof useServerDialogs>;
}) {
  const M = KITS[props.kind];
  const { server, dialogs } = props;
  const act = useServersStore((s) => s.act);
  const me = useAccountStore((s) => s.profile?.id);
  const role = myRole(server, me);
  const admin = role === "owner" || role === "admin";
  const item = (icon: ReactNode, label: string, onSelect: () => void, tone?: "accent" | "danger") => (
    <M.Item
      onSelect={onSelect}
      className={`flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium outline-none ${
        tone === "danger"
          ? "text-live data-[highlighted]:bg-live data-[highlighted]:text-white"
          : tone === "accent"
            ? "text-[#9a86ff] data-[highlighted]:bg-brand data-[highlighted]:text-white"
            : "text-text-muted data-[highlighted]:bg-brand data-[highlighted]:text-white"
      }`}
    >
      {label}
      {icon}
    </M.Item>
  );
  return (
    <>
      {item(<UserPlus size={16} />, "Convidar pessoas", () => void act(server.id, { kind: "invite" }), "accent")}
      {admin && item(<Settings size={16} />, "Configurações do servidor", dialogs.openSettings)}
      {admin && item(<Plus size={16} />, "Criar sala", dialogs.openCreate)}
      <M.Separator className="my-1 h-px bg-surface-3" />
      {role === "owner"
        ? item(<Trash2 size={16} />, "Apagar servidor", dialogs.openDelete, "danger")
        : item(<LogOut size={16} />, "Sair do servidor", () => void act(server.id, { kind: "leave" }), "danger")}
    </>
  );
}

/** Botão direito no ícone do servidor (na barra da esquerda): o mesmo menu do nome dele. */
export function ServerContextMenu(props: { server: Server; children: ReactNode }) {
  const dialogs = useServerDialogs(props.server);
  return (
    <>
      <ContextMenu.Root onOpenChange={useOverlayMenu()}>
        <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content aria-label={`Opções de ${props.server.name}`} className={SERVER_MENU_CONTENT}>
            <ServerMenuItems kind="context" server={props.server} dialogs={dialogs} />
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      {dialogs.dialogs}
    </>
  );
}

/** Apagar o servidor: só confirmar (sem digitar o nome). */
function ConfirmDelete(props: { server: Server; open: boolean; onOpenChange: (open: boolean) => void }) {
  const act = useServersStore((s) => s.act);
  return (
    <Modal
      open={props.open}
      onOpenChange={props.onOpenChange}
      width={440}
      title={`Apagar ${props.server.name}?`}
      description="O servidor some para todos, com as salas e os convites. Não dá para desfazer."
      footer={
        <>
          <span />
          <div className="flex gap-2">
            <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
            <button
              type="button"
              onClick={() => {
                props.onOpenChange(false);
                void act(props.server.id, { kind: "delete" });
              }}
              className="rounded-md bg-live px-5 py-2 text-sm font-semibold text-white transition-colors hover:bg-live/85"
            >
              Apagar servidor
            </button>
          </div>
        </>
      }
    >
      {null}
    </Modal>
  );
}
