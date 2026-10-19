import * as ContextMenu from "@radix-ui/react-context-menu";
import { Eye, Pencil } from "lucide-react";
import { type ReactNode, useState } from "react";
import { NameDialog } from "./NameDialog";
import { type Member, useRoomStore } from "./room-store";

/**
 * Menu do botão direito num membro, como o do Discord. No nosso nome: trocar o apelido. Nos
 * outros: assistir (se estiver ao vivo). O volume e o silenciar de cada um entram com a voz.
 */
export function MemberMenu(props: { member: Member; isMe: boolean; onWatch?: () => void; children: ReactNode }) {
  const setNickname = useRoomStore((s) => s.setNickname);
  const [renaming, setRenaming] = useState(false);
  const { member } = props;

  return (
    <>
      <ContextMenu.Root>
        <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content
            aria-label={`Opções de ${member.name}`}
            className="z-50 w-56 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            <ContextMenu.Label className="truncate px-2 py-1.5 text-xs font-semibold tracking-wide text-text-faint uppercase">
              {member.name}
            </ContextMenu.Label>
            {props.isMe ? (
              <Item icon={<Pencil size={16} />} onSelect={() => setRenaming(true)}>
                Alterar apelido
              </Item>
            ) : (
              member.live &&
              props.onWatch && (
                <Item icon={<Eye size={16} />} onSelect={props.onWatch}>
                  Assistir a transmissão
                </Item>
              )
            )}
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      {props.isMe && (
        <NameDialog
          open={renaming}
          onOpenChange={setRenaming}
          title="Alterar apelido"
          description="Como a galera vê você nesta sala."
          label="Apelido"
          initial={member.name}
          onSave={(name) => void setNickname(name)}
        />
      )}
    </>
  );
}

function Item(props: { icon: ReactNode; onSelect: () => void; children: ReactNode }) {
  return (
    <ContextMenu.Item
      onSelect={props.onSelect}
      className="flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white"
    >
      {props.children}
      {props.icon}
    </ContextMenu.Item>
  );
}
