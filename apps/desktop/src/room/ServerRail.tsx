import { Plus } from "lucide-react";
import { type ReactNode, useState } from "react";
import logo from "../assets/logo.svg";
import { useFriendsStore } from "../friends/friends-store";
import { AddServerDialog } from "../servers/AddServerDialog";
import { ServerIcon } from "../servers/ServerIcon";
import { serverOfRoom, useServersStore } from "../servers/servers-store";
import { Tooltip } from "../ui/Tooltip";
import { RoomDialog } from "./RoomDialog";
import { roomInitials, roomTitle, useRoomStore, useServerInviteStore } from "./room-store";

/**
 * Barra de salas (a coluna da esquerda, como a de servidores do Discord): Amigos, os nossos
 * servidores e a sala avulsa em que estamos. Ícones redondos que viram quadrados arredondados com o
 * mouse, e a "pílula" branca na borda indicando onde estamos.
 */
export function ServerRail(props: { hidden: boolean }) {
  const code = useRoomStore((s) => s.code);
  const roomName = useRoomStore((s) => s.roomName);
  const someoneLive = useRoomStore((s) => s.members.slice(1).some((m) => m.live));
  const friendsOpen = useFriendsStore((s) => s.open);
  const setFriendsOpen = useFriendsStore((s) => s.setOpen);
  const pending = useFriendsStore((s) => s.incoming.length);
  const servers = useServersStore((s) => s.servers);
  const selected = useServersStore((s) => s.selected);
  const select = useServersStore((s) => s.select);
  // A sala em que estamos é de um servidor: ela aparece no servidor, não como sala avulsa.
  const roomServer = serverOfRoom(servers, code);
  const [adding, setAdding] = useState(false);
  const [quickRoom, setQuickRoom] = useState(false);
  // Convite de servidor recebido por link: abre o "Entrar num servidor" já preenchido.
  const linkInvite = useServerInviteStore((s) => s.invite);

  return (
    <nav
      aria-label="Salas"
      className={`${props.hidden ? "hidden" : "flex"} col-1 row-2 min-h-0 flex-col items-center gap-2 overflow-y-auto pt-1 pb-3`}
    >
      <RailItem
        label="Amigos"
        active={friendsOpen || (!code && !selected)}
        count={pending}
        onClick={() => {
          select(null);
          setFriendsOpen(true);
        }}
      >
        <span className="grid size-12 place-items-center bg-brand transition-colors group-hover:bg-brand-hover">
          <img src={logo} alt="SquadView" className="size-8" />
        </span>
      </RailItem>
      <div className="h-0.5 w-8 rounded-full bg-surface-3" />
      {servers.map((server) => (
        <RailItem
          key={server.id}
          label={server.name}
          active={!friendsOpen && selected === server.id}
          badge={roomServer?.id === server.id && someoneLive}
          onClick={() => select(server.id)}
        >
          <ServerIcon name={server.name} icon={server.icon} />
        </RailItem>
      ))}
      {code && !roomServer && (
        <RailItem
          label={roomTitle(code, roomName)}
          active={!friendsOpen && !selected}
          badge={someoneLive}
          onClick={() => {
            select(null);
            setFriendsOpen(false);
          }}
        >
          <span className="grid size-12 place-items-center bg-surface-3 font-mono text-[11px] font-bold tracking-wider">
            {roomInitials(code, roomName)}
          </span>
        </RailItem>
      )}
      <button
        type="button"
        aria-label="Adicionar um servidor"
        onClick={() => setAdding(true)}
        className="group relative"
      >
        <Tooltip label="Adicionar um servidor" side="right">
          <span className="grid size-12 place-items-center overflow-hidden rounded-[24px] bg-surface-3 text-online transition-all duration-200 ease-out group-hover:rounded-2xl group-hover:bg-online group-hover:text-white">
            <Plus size={22} />
          </span>
        </Tooltip>
      </button>
      <AddServerDialog
        key={linkInvite ?? "novo"}
        open={adding || linkInvite !== null}
        onOpenChange={(open) => {
          setAdding(open);
          if (!open) useServerInviteStore.setState({ invite: null });
        }}
        initialInvite={linkInvite ?? undefined}
        onQuickRoom={() => setQuickRoom(true)}
      />
      <RoomDialog open={quickRoom} onOpenChange={setQuickRoom} />
    </nav>
  );
}

/** Um ícone da barra, com a pílula da esquerda e a dica com o nome. */
function RailItem(props: {
  label: string;
  active: boolean;
  badge?: boolean;
  /** Número vermelho no canto (pedidos de amizade). */
  count?: number;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <div className="group relative flex w-full justify-center">
      {/* Pílula: some (inativo), cresce um pouco no hover e fica alta no ativo. */}
      <span
        aria-hidden
        className={`absolute top-1/2 left-0 w-1 -translate-y-1/2 rounded-r-full bg-text transition-all duration-200 ease-out ${
          props.active ? "h-10" : "h-0 group-hover:h-5"
        }`}
      />
      <Tooltip label={props.label} side="right">
        <button
          type="button"
          aria-label={props.label}
          aria-current={props.active ? "page" : undefined}
          onClick={props.onClick}
          className={`relative block overflow-hidden transition-all duration-200 ease-out ${
            props.active ? "rounded-2xl" : "rounded-[24px] group-hover:rounded-2xl"
          }`}
        >
          {props.children}
        </button>
      </Tooltip>
      {/* Alguém ao vivo na sala: pontinho vermelho, como as notificações do Discord. */}
      {!!props.count && (
        <span
          aria-label={`${props.count} pedido(s) de amizade`}
          className="absolute right-2 bottom-0 grid h-5 min-w-5 place-items-center rounded-full border-[3px] border-surface-0 bg-live px-1 text-[11px] font-bold text-white"
        >
          {props.count}
        </span>
      )}
      {props.badge && (
        <span
          aria-label="Alguém ao vivo"
          className="absolute right-3 bottom-0 size-4 rounded-full border-[3px] border-surface-0 bg-live"
        />
      )}
    </div>
  );
}
