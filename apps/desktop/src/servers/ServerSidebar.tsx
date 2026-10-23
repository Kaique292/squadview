import * as Menu from "@radix-ui/react-dropdown-menu";
import { ChevronDown, HeadphoneOff, MicOff, Plus, UserPlus, Volume2 } from "lucide-react";
import { avatarUrl, useAccountStore } from "../account/account-store";
import { Elapsed, RoomMemberList } from "../room/RoomSidebar";
import { useRoomStore } from "../room/room-store";
import { Avatar } from "../ui/Avatar";
import { LiveBadge } from "../ui/LiveBadge";
import { ScrollArea } from "../ui/ScrollArea";
import { Tooltip } from "../ui/Tooltip";
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

  return (
    <>
      <div className="flex h-12 shrink-0 items-center border-b border-white/[0.06]">
        <Menu.Root>
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
                {here && members > 0 ? (
                  <div className="py-0.5">
                    <RoomMemberList />
                  </div>
                ) : (
                  <RoomOccupants server={server} roomId={room.id} />
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

/**
 * Quem está numa sala do servidor em que **não** estamos (ADR 0018): foto, nome, mudo, silenciado e
 * AO VIVO, vindos da lista de membros. Na sala em que estamos aparece a lista da própria sala, mais completa.
 */
function RoomOccupants(props: { server: Server; roomId: string }) {
  const people = useServersStore((s) => s.presence[props.server.id]?.[props.roomId]);
  if (!people?.length) return null;
  return (
    <ul aria-label="Quem está na sala" className="flex flex-col gap-0.5 py-0.5 pl-6">
      {people.map((p) => {
        const member = props.server.members.find((m) => m.id === p.id);
        if (!member) return null;
        return (
          <li key={p.id} className="flex items-center gap-2 rounded-md px-2 py-1 text-text-muted">
            <Avatar name={member.name} size={24} src={member.avatar ? avatarUrl(member, 64) : undefined} />
            <span className="min-w-0 flex-1 truncate text-sm">{member.name}</span>
            {(p.muted || p.deafened) && (
              <MicOff size={14} aria-label="Microfone silenciado" className="shrink-0 text-text-faint" />
            )}
            {p.deafened && <HeadphoneOff size={14} aria-label="Ensurdecido" className="shrink-0 text-text-faint" />}
            {p.live && <LiveBadge size="sm" />}
          </li>
        );
      })}
    </ul>
  );
}
