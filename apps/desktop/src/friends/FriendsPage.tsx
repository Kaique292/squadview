import * as Menu from "@radix-ui/react-dropdown-menu";
import { Check, Loader2, LogIn, MessageCircle, MoreVertical, UserPlus, Users, X } from "lucide-react";
import { type ReactNode, useState } from "react";
import { DiscordLoginButton } from "../account/DiscordLoginButton";
import { avatarUrl, useAccountStore } from "../account/account-store";
import { useDmStore } from "../dm/dm-store";
import { useRoomStore } from "../room/room-store";
import { Avatar } from "../ui/Avatar";
import { ScrollArea } from "../ui/ScrollArea";
import { Tooltip } from "../ui/Tooltip";
import { type Friend, type Person, presenceText, useFriendsStore, useInMyRoom } from "./friends-store";
import { CheckMark } from "../ui/Checkbox";

type Tab = "online" | "todos" | "pendentes" | "recentes";

function photo(p: Person) {
  return avatarUrl(p, 64);
}

/**
 * Amigos, como a página de amigos do Discord: abas no topo e uma linha por pessoa, com as ações
 * aparecendo no hover. Fica no lugar da sala enquanto aberta (a sala continua conectada).
 */
export function FriendsPage() {
  const status = useFriendsStore((s) => s.status);
  const profile = useAccountStore((s) => s.profile);
  const incoming = useFriendsStore((s) => s.incoming);
  const [tab, setTab] = useState<Tab>("online");

  return (
    <section aria-label="Amigos" className="flex h-full w-full flex-col text-left">
      <header className="flex h-12 shrink-0 items-center gap-4 border-b border-white/[0.06] px-4">
        <span className="flex items-center gap-2 font-semibold">
          <Users size={20} className="text-text-muted" /> Amigos
        </span>
        <span className="h-6 w-px bg-white/10" />
        <nav aria-label="Abas dos amigos" className="flex gap-1">
          <TabButton tab="online" current={tab} onSelect={setTab}>
            Online
          </TabButton>
          <TabButton tab="todos" current={tab} onSelect={setTab}>
            Todos
          </TabButton>
          <TabButton tab="pendentes" current={tab} onSelect={setTab} badge={incoming.length}>
            Pendentes
          </TabButton>
          <TabButton tab="recentes" current={tab} onSelect={setTab}>
            Recentes
          </TabButton>
        </nav>
      </header>
      {status === "online" ? (
        <ScrollArea className="min-h-0 flex-1" viewportClassName="px-6 py-4">
          <div key={tab} className="animate-step-in">
            {tab === "online" && <FriendList onlineOnly />}
            {tab === "todos" && <FriendList />}
            {tab === "pendentes" && <Pending />}
            {tab === "recentes" && <Recents />}
          </div>
        </ScrollArea>
      ) : (
        <div className="grid flex-1 place-items-center p-6">
          <div className="flex max-w-sm flex-col items-center gap-3 text-center">
            {status === "connecting" ? (
              <>
                <Loader2 size={40} className="animate-spin text-text-faint" />
                <p className="text-text-muted">Conectando aos amigos…</p>
              </>
            ) : (
              <>
                <LogIn size={48} className="text-text-faint" />
                <h2 className="text-xl font-semibold">{profile ? "Ative os amigos" : "Entre para ver seus amigos"}</h2>
                <p className="text-text-muted">
                  {profile
                    ? "Entre de novo com o Discord para o SquadView confirmar que é você. Aí seus amigos veem quando você está online e podem pedir para entrar na sua sala."
                    : "Com o Discord, você vê quem está online, pede para entrar na sala dos amigos e chama a galera para a sua."}
                </p>
                <DiscordLoginButton />
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

function TabButton(props: { tab: Tab; current: Tab; onSelect: (t: Tab) => void; badge?: number; children: ReactNode }) {
  const active = props.tab === props.current;
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => props.onSelect(props.tab)}
      className={`flex items-center gap-1.5 rounded-md px-2.5 py-1 text-sm font-medium transition-colors ${
        active ? "bg-surface-hover text-text" : "text-text-muted hover:bg-surface-hover/60 hover:text-text"
      }`}
    >
      {props.children}
      {!!props.badge && (
        <span className="grid min-w-4 place-items-center rounded-full bg-live px-1 text-[11px] font-bold text-white">
          {props.badge}
        </span>
      )}
    </button>
  );
}

function SectionTitle(props: { children: ReactNode }) {
  return <h3 className="mb-2 text-xs font-semibold tracking-wide text-text-faint uppercase">{props.children}</h3>;
}

function EmptyNote(props: { children: ReactNode }) {
  return <p className="py-10 text-center text-sm text-text-muted">{props.children}</p>;
}

/** Uma pessoa na lista: foto, nome, uma linha embaixo e as ações à direita. */
function Row(props: { person: Person; online?: boolean; detail: string; children?: ReactNode }) {
  return (
    <li className="group flex items-center gap-3 border-t border-white/[0.06] px-2 py-2.5 first:border-t-0 hover:rounded-md hover:bg-surface-hover/60">
      <Avatar
        name={props.person.name}
        size={36}
        src={photo(props.person)}
        status={props.online === undefined ? undefined : props.online ? "online" : "offline"}
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-semibold">{props.person.name}</span>
        <span className="block truncate text-xs text-text-muted">{props.detail}</span>
      </span>
      <span className="flex shrink-0 items-center gap-2">{props.children}</span>
    </li>
  );
}

/** Botão redondo de ação, como os da lista do Discord. */
function RoundAction(props: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip label={props.label} side="top">
      <button
        type="button"
        aria-label={props.label}
        // `aria-disabled` em vez de `disabled`: o tooltip continua aparecendo e explica o porquê.
        aria-disabled={props.disabled || undefined}
        onClick={() => !props.disabled && props.onClick()}
        className={`grid size-9 place-items-center rounded-full bg-surface-1 text-text-muted transition-colors ${
          props.disabled ? "cursor-default opacity-40" : props.danger ? "hover:text-live" : "hover:text-text"
        }`}
      >
        {props.children}
      </button>
    </Tooltip>
  );
}

function FriendList(props: { onlineOnly?: boolean }) {
  const friends = useFriendsStore((s) => s.friends);
  const list = props.onlineOnly ? friends.filter((f) => f.presence.online) : friends;
  if (friends.length === 0) {
    return (
      <EmptyNote>
        Ainda sem amigos. Quem entra numa sala com você aparece em Recentes, e de lá você manda o pedido.
      </EmptyNote>
    );
  }
  if (list.length === 0) return <EmptyNote>Ninguém online agora.</EmptyNote>;
  return (
    <>
      <SectionTitle>
        {props.onlineOnly ? "Online" : "Todos os amigos"} — {list.length}
      </SectionTitle>
      <ul>
        {list.map((f) => (
          <FriendRow key={f.id} friend={f} />
        ))}
      </ul>
    </>
  );
}

function FriendRow(props: { friend: Friend }) {
  const { friend } = props;
  const myRoom = useRoomStore((s) => s.code);
  const together = useInMyRoom(friend.id);
  const joinFriend = useFriendsStore((s) => s.joinFriend);
  const act = useFriendsStore((s) => s.act);
  const waitingFor = useFriendsStore((s) => s.waitingFor);
  const openChat = useDmStore((s) => s.openChat);
  const p = friend.presence;
  return (
    <Row person={friend} online={p.online} detail={presenceText(p)}>
      {together ? (
        // Já estamos juntos: nada para entrar nem para chamar.
        <span className="rounded-md bg-surface-3 px-3 py-1.5 text-sm font-medium text-text-muted select-none">
          Na sua sala
        </span>
      ) : (
        p.inRoom && (
          <button
            type="button"
            onClick={() => void joinFriend(friend)}
            disabled={waitingFor === friend.name}
            className="rounded-md bg-online px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-online/85 disabled:opacity-60"
          >
            {waitingFor === friend.name ? "Pedido enviado" : "Entrar"}
          </button>
        )
      )}
      <RoundAction label="Mensagem" onClick={() => void openChat(friend.id)}>
        <MessageCircle size={18} />
      </RoundAction>
      {myRoom && p.online && (
        <RoundAction
          label={together ? "Já está na sua sala" : "Chamar para a sala"}
          disabled={together}
          onClick={() => void act({ type: "invite", id: friend.id })}
        >
          <UserPlus size={18} />
        </RoundAction>
      )}
      <Menu.Root>
        <Menu.Trigger asChild>
          <button
            type="button"
            aria-label={`Mais opções de ${friend.name}`}
            className="grid size-9 place-items-center rounded-full bg-surface-1 text-text-muted transition-colors hover:text-text"
          >
            <MoreVertical size={18} />
          </button>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            align="end"
            sideOffset={6}
            className="z-50 w-64 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            <Menu.CheckboxItem
              checked={friend.direct}
              onCheckedChange={(direct) => void act({ type: "set-direct", id: friend.id, direct })}
              className="flex cursor-pointer items-center justify-between gap-2 rounded px-2 py-1.5 text-sm text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white"
            >
              <span>
                <span className="block font-medium">Entra direto nas minhas salas</span>
                <span className="block text-xs opacity-70">Sem precisar que alguém aceite</span>
              </span>
              <CheckMark checked={friend.direct} />
            </Menu.CheckboxItem>
            <Menu.Separator className="my-1 h-px bg-surface-3" />
            <Menu.Item
              onSelect={() => void act({ type: "unfriend", id: friend.id })}
              className="cursor-pointer rounded px-2 py-1.5 text-sm text-live outline-none data-[highlighted]:bg-live data-[highlighted]:text-white"
            >
              Desfazer amizade
            </Menu.Item>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
    </Row>
  );
}

function Pending() {
  const incoming = useFriendsStore((s) => s.incoming);
  const outgoing = useFriendsStore((s) => s.outgoing);
  const act = useFriendsStore((s) => s.act);
  if (incoming.length + outgoing.length === 0) return <EmptyNote>Nenhum pedido de amizade pendente.</EmptyNote>;
  return (
    <>
      {incoming.length > 0 && (
        <>
          <SectionTitle>Recebidos — {incoming.length}</SectionTitle>
          <ul className="mb-6">
            {incoming.map((p) => (
              <Row key={p.id} person={p} detail="Quer ser seu amigo">
                <RoundAction label="Aceitar" onClick={() => void act({ type: "friend-accept", id: p.id })}>
                  <Check size={18} />
                </RoundAction>
                <RoundAction label="Recusar" danger onClick={() => void act({ type: "friend-decline", id: p.id })}>
                  <X size={18} />
                </RoundAction>
              </Row>
            ))}
          </ul>
        </>
      )}
      {outgoing.length > 0 && (
        <>
          <SectionTitle>Enviados — {outgoing.length}</SectionTitle>
          <ul>
            {outgoing.map((p) => (
              <Row key={p.id} person={p} detail="Pedido enviado">
                <RoundAction
                  label="Cancelar pedido"
                  danger
                  onClick={() => void act({ type: "friend-cancel", id: p.id })}
                >
                  <X size={18} />
                </RoundAction>
              </Row>
            ))}
          </ul>
        </>
      )}
    </>
  );
}

function Recents() {
  const recents = useFriendsStore((s) => s.recents);
  const outgoing = useFriendsStore((s) => s.outgoing);
  const act = useFriendsStore((s) => s.act);
  if (recents.length === 0) {
    return <EmptyNote>Quem entrar numa sala com você (com o Discord) aparece aqui.</EmptyNote>;
  }
  return (
    <>
      <SectionTitle>Estiveram numa sala com você — {recents.length}</SectionTitle>
      <ul>
        {recents.map((p) => {
          const sent = outgoing.some((o) => o.id === p.id);
          return (
            <Row key={p.id} person={p} detail={sent ? "Pedido enviado" : "Esteve numa sala com você"}>
              {!sent && (
                <button
                  type="button"
                  onClick={() => void act({ type: "friend-request", id: p.id })}
                  className="flex items-center gap-1.5 rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-brand-hover"
                >
                  <UserPlus size={16} /> Adicionar amigo
                </button>
              )}
            </Row>
          );
        })}
      </ul>
    </>
  );
}
