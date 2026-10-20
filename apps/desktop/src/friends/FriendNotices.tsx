import { avatarUrl } from "../account/account-store";
import { Avatar } from "../ui/Avatar";
import { type Person, useFriendsStore } from "./friends-store";

/**
 * Pedidos para entrar na nossa sala e convites de amigos: cartões no canto, com os botões ali
 * mesmo (qualquer pessoa da sala pode aceitar; o primeiro que responde decide).
 */
export function FriendNotices() {
  const joinRequests = useFriendsStore((s) => s.joinRequests);
  const invites = useFriendsStore((s) => s.invites);
  const answerJoin = useFriendsStore((s) => s.answerJoin);
  const acceptInvite = useFriendsStore((s) => s.acceptInvite);
  const dismissInvite = useFriendsStore((s) => s.dismissInvite);
  if (joinRequests.length === 0 && invites.length === 0) return null;

  return (
    <aside
      aria-label="Pedidos e convites"
      className="pointer-events-none fixed right-6 bottom-6 z-50 flex w-80 flex-col gap-2"
    >
      {joinRequests.map((r) => (
        <Notice
          key={r.id}
          person={r.from}
          title={`${r.from.name} quer entrar na sala`}
          detail="Amigo de alguém daqui"
          accept="Deixar entrar"
          decline="Recusar"
          onAccept={() => void answerJoin(r.id, true)}
          onDecline={() => void answerJoin(r.id, false)}
        />
      ))}
      {invites.map((i) => (
        <Notice
          key={`${i.from.id}-${i.code}`}
          person={i.from}
          title={`${i.from.name} te chamou para a sala`}
          detail={i.room ?? `Sala ${i.code}`}
          accept="Entrar"
          decline="Agora não"
          onAccept={() => void acceptInvite(i)}
          onDecline={() => dismissInvite(i)}
        />
      ))}
    </aside>
  );
}

function Notice(props: {
  person: Person;
  title: string;
  detail: string;
  accept: string;
  decline: string;
  onAccept: () => void;
  onDecline: () => void;
}) {
  return (
    <div
      role="alertdialog"
      aria-label={props.title}
      className="pointer-events-auto animate-menu-in rounded-lg border border-white/[0.06] bg-surface-float p-3 text-left shadow-2xl shadow-black/50"
    >
      <div className="flex items-center gap-3">
        <Avatar name={props.person.name} size={36} src={avatarUrl(props.person, 64)} />
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold">{props.title}</p>
          <p className="truncate text-xs text-text-muted">{props.detail}</p>
        </div>
      </div>
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          onClick={props.onAccept}
          className="flex-1 rounded-md bg-online px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-online/85"
        >
          {props.accept}
        </button>
        <button
          type="button"
          onClick={props.onDecline}
          className="flex-1 rounded-md bg-surface-3 px-3 py-1.5 text-sm font-medium transition-colors hover:bg-surface-hover"
        >
          {props.decline}
        </button>
      </div>
    </div>
  );
}
