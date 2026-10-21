import { LogIn, Paperclip, Plus, Users } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { avatarUrl } from "../account/account-store";
import { type Person, useFriendsStore } from "../friends/friends-store";
import { RoomDialog } from "../room/RoomDialog";
import { Avatar } from "../ui/Avatar";
import { ScrollArea } from "../ui/ScrollArea";
import { type Conversation, useDmStore } from "./dm-store";

/** Quem é o amigo da conversa (pela lista de amigos; quem saiu dela fica só com o id). */
export function usePerson(id: string): Person {
  const friend = useFriendsStore((s) => s.friends.find((f) => f.id === id));
  return friend ?? { id, name: "Alguém", avatar: null };
}

/**
 * A lateral na página de amigos, como as mensagens diretas do Discord: "Amigos" no topo e as
 * conversas embaixo, a mais recente primeiro.
 */
export function DmSidebar(props: { className: string }) {
  const conversations = useDmStore((s) => s.conversations);
  const open = useDmStore((s) => s.open);
  const openChat = useDmStore((s) => s.openChat);
  const incoming = useFriendsStore((s) => s.incoming.length);
  return (
    <aside aria-label="Mensagens diretas" className={`flex min-h-0 flex-col ${props.className}`}>
      <header className="flex h-12 shrink-0 items-center border-b border-white/[0.06] px-4 font-semibold">
        SquadView
      </header>
      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-2 py-2">
        <button
          type="button"
          aria-current={open === null ? "page" : undefined}
          onClick={() => void openChat(null)}
          className={`mb-0.5 flex w-full items-center gap-3 rounded-md px-2 py-2 text-left font-medium transition-colors ${
            open === null ? "bg-surface-hover text-text" : "text-text-muted hover:bg-surface-hover/60 hover:text-text"
          }`}
        >
          <Users size={20} />
          <span className="flex-1">Amigos</span>
          {incoming > 0 && (
            <span className="grid min-w-5 place-items-center rounded-full bg-live px-1 text-[11px] font-bold text-white">
              {incoming}
            </span>
          )}
        </button>
        {/* As salas rápidas continuam à mão na página inicial. */}
        <RoomDialog trigger={<SideButton icon={<Plus size={20} />}>Criar sala</SideButton>} />
        <RoomDialog
          initialMode="join"
          trigger={<SideButton icon={<LogIn size={20} />}>Entrar com código</SideButton>}
        />
        <p className="mt-3 px-2 pb-1 text-xs font-semibold tracking-wide text-text-faint uppercase">
          Mensagens diretas
        </p>
        {conversations.length === 0 ? (
          <p className="px-2 py-3 text-sm text-text-muted">Nenhuma conversa ainda.</p>
        ) : (
          <ul aria-label="Conversas" className="flex flex-col gap-0.5">
            {conversations.map((c) => (
              <ConversationRow key={c.friend} conversation={c} active={open === c.friend} />
            ))}
          </ul>
        )}
      </ScrollArea>
    </aside>
  );
}

/** Linha de ação da lateral (o `RoomDialog` usa como gatilho, então repassa as props do botão). */
function SideButton(props: { icon: ReactNode; children: ReactNode } & ButtonHTMLAttributes<HTMLButtonElement>) {
  const { icon, children, ...rest } = props;
  return (
    <button
      type="button"
      {...rest}
      className="mb-0.5 flex w-full items-center gap-3 rounded-md px-2 py-2 text-left font-medium text-text-muted transition-colors hover:bg-surface-hover/60 hover:text-text"
    >
      {icon}
      {children}
    </button>
  );
}

function preview(c: Conversation): string {
  const m = c.last;
  if (!m) return "";
  const body = m.text ?? `Anexo: ${m.file!.name}`;
  return m.mine ? `Você: ${body}` : body;
}

function ConversationRow(props: { conversation: Conversation; active: boolean }) {
  const { conversation: c } = props;
  const person = usePerson(c.friend);
  const online = useFriendsStore((s) => s.friends.find((f) => f.id === c.friend)?.presence.online ?? false);
  const unread = useDmStore((s) => s.unread[c.friend] ?? 0);
  const openChat = useDmStore((s) => s.openChat);
  return (
    <li>
      <button
        type="button"
        aria-current={props.active ? "page" : undefined}
        aria-label={`Conversa com ${person.name}`}
        onClick={() => void openChat(c.friend)}
        className={`group flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors ${
          props.active ? "bg-surface-hover text-text" : "text-text-muted hover:bg-surface-hover/60 hover:text-text"
        }`}
      >
        <Avatar
          name={person.name}
          size={32}
          src={person.avatar ? avatarUrl(person, 64) : undefined}
          status={online ? "online" : "offline"}
          statusBorder="border-surface-1"
        />
        <span className="min-w-0 flex-1">
          <span className={`block truncate text-sm ${unread ? "font-semibold text-text" : "font-medium"}`}>
            {person.name}
          </span>
          <span className="flex items-center gap-1 truncate text-xs text-text-faint">
            {c.last?.file && <Paperclip size={11} className="shrink-0" />}
            <span className="truncate">{preview(c)}</span>
          </span>
        </span>
        {unread > 0 && (
          <span
            aria-label={`${unread} não lida(s)`}
            className="grid min-w-5 place-items-center rounded-full bg-live px-1 text-[11px] font-bold text-white"
          >
            {unread}
          </span>
        )}
      </button>
    </li>
  );
}
