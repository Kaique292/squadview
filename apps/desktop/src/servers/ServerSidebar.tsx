import * as Menu from "@radix-ui/react-dropdown-menu";
import { ChevronDown, Plus, UserPlus, Volume2 } from "lucide-react";
import { useAccountStore } from "../account/account-store";
import { Elapsed, RoomMemberList } from "../room/RoomSidebar";
import { useRoomStore } from "../room/room-store";
import { ScrollArea } from "../ui/ScrollArea";
import { Tooltip } from "../ui/Tooltip";
import { useOverlayMenu } from "../ui/overlay";
import { SERVER_MENU_CONTENT, ServerMenuItems, useServerDialogs } from "./ServerMenu";
import { myRole, type Server, useServersStore } from "./servers-store";

/**
 * A lateral de um servidor, como a lista de canais do Discord: o nome com o menu (convidar,
 * configurações, sair) e as salas como canais de voz. Na sala em que estamos aparece quem está nela.
 */
export function ServerSidebar(props: { server: Server }) {
  const { server } = props;
  const me = useAccountStore((s) => s.profile?.id);
  const role = myRole(server, me);
  const admin = role === "owner" || role === "admin";
  const { act, enterRoom } = useServersStore();
  const code = useRoomStore((s) => s.code);
  const joinedAt = useRoomStore((s) => s.joinedAt);
  const members = useRoomStore((s) => s.members.length);
  const dialogs = useServerDialogs(server);
  const onMenuOpenChange = useOverlayMenu();

  return (
    <>
      <div className="flex h-12 shrink-0 items-center border-b border-white/[0.06]">
        <Menu.Root onOpenChange={onMenuOpenChange}>
          <Menu.Trigger asChild>
            <button
              type="button"
              className="group flex h-full min-w-0 flex-1 items-center justify-between gap-2 px-4 text-left font-semibold transition-colors hover:bg-surface-hover data-[state=open]:bg-surface-hover"
            >
              <span className="truncate">{server.name}</span>
              <ChevronDown
                size={18}
                className="shrink-0 transition-transform duration-200 group-data-[state=open]:rotate-180"
              />
            </button>
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Content align="start" sideOffset={6} className={SERVER_MENU_CONTENT}>
              <ServerMenuItems kind="dropdown" server={server} dialogs={dialogs} />
            </Menu.Content>
          </Menu.Portal>
        </Menu.Root>
        <Tooltip label="Convidar pessoas" side="bottom">
          <button
            type="button"
            aria-label="Copiar link de convite do servidor"
            onClick={() => void act(server.id, { kind: "invite" })}
            className="mr-2 rounded p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
          >
            <UserPlus size={18} />
          </button>
        </Tooltip>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-2 pt-4 pb-2">
        <div className="flex items-center justify-between px-1 pb-1 text-xs font-semibold tracking-wide text-text-faint uppercase">
          Salas de voz
          {admin && (
            <Tooltip label="Criar sala" side="top">
              <button
                type="button"
                aria-label="Criar sala"
                onClick={dialogs.openCreate}
                className="rounded p-0.5 transition-colors hover:text-text"
              >
                <Plus size={14} />
              </button>
            </Tooltip>
          )}
        </div>
        <ul aria-label="Salas do servidor" className="flex flex-col gap-0.5">
          {server.rooms.map((room) => {
            const here = room.code === code;
            return (
              <li key={room.id}>
                <button
                  type="button"
                  aria-current={here ? "location" : undefined}
                  onClick={() => void enterRoom(server, room)}
                  className={`flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left transition-colors ${
                    here ? "bg-surface-hover text-text" : "text-text-muted hover:bg-surface-hover/60 hover:text-text"
                  }`}
                >
                  <Volume2 size={18} className={`shrink-0 ${here ? "text-online" : ""}`} />
                  <span className="min-w-0 flex-1 truncate font-medium">{room.name}</span>
                  {here && joinedAt && <Elapsed since={joinedAt} />}
                </button>
                {here && members > 0 && (
                  <div className="py-0.5">
                    <RoomMemberList />
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </ScrollArea>

      {dialogs.dialogs}
    </>
  );
}
