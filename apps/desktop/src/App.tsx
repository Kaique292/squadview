import { FriendNotices } from "./friends/FriendNotices";
import { FriendsPage } from "./friends/FriendsPage";
import { useFriendsStore } from "./friends/friends-store";
import { RoomDialog } from "./room/RoomDialog";
import { RoomSidebar, UserDock } from "./room/RoomSidebar";
import { ServerHome } from "./servers/ServerHome";
import { ServerSidebar } from "./servers/ServerSidebar";
import { useServersStore } from "./servers/servers-store";
import { ServerRail } from "./room/ServerRail";
import { WatchView } from "./room/WatchView";
import { roomTitle, useRoomStore } from "./room/room-store";
import { StreamView } from "./stream/StreamView";
import { usePresenceSync } from "./settings/presence-store";
import { useStreamStore } from "./stream/stream-store";
import { Dissolve } from "./ui/Dissolve";
import { VoiceRoom } from "./voice/VoiceRoom";
import { TitleBar } from "./ui/TitleBar";
import { Toaster } from "./ui/Toaster";

/**
 * Layout principal, na hierarquia do Discord: barra de título, barra de salas, barra lateral com
 * o canto arredondado, área do stream e o card do usuário flutuando no pé das duas colunas.
 */
export function App() {
  const live = useStreamStore((s) => s.status === "live");
  const fullscreen = useStreamStore((s) => s.fullscreen);
  const watching = useRoomStore((s) => s.watching);
  const code = useRoomStore((s) => s.code);
  const roomName = useRoomStore((s) => s.roomName);
  // O servidor aberto na lateral (pela barra de salas ou por entrar numa sala dele).
  const server = useServersStore((s) => s.servers.find((x) => x.id === s.selected) ?? null);
  // Amigos é a página inicial, como no Discord: fora de uma sala (sem assistir nem transmitir) e
  // sem servidor aberto, é ela que aparece; dentro, abre pelo ícone do SquadView na barra de salas.
  const friendsOpen = (useFriendsStore((s) => s.open) || (!code && !watching && !live && !server)) && !fullscreen;
  usePresenceSync();

  return (
    <div
      className={`grid h-full ${fullscreen ? "bg-surface-2" : "bg-glass-0"} ${
        fullscreen
          ? "grid-cols-1 grid-rows-1"
          : "grid-cols-[72px_240px_minmax(0,1fr)] grid-rows-[auto_minmax(0,1fr)_auto]"
      }`}
    >
      <InvitePrompt />
      <Dissolve />
      <FriendNotices />
      <Toaster anchor={(watching || live) && !fullscreen ? "sidebar" : "center"} />
      {!fullscreen && <TitleBar title={code ? roomTitle(code, roomName) : undefined} />}
      <ServerRail hidden={fullscreen} />
      {/* Moldura da lateral: o canto arredondado e a borda fina do Discord. */}
      {!fullscreen && (
        <div aria-hidden className="col-2 row-[2/4] rounded-tl-lg border-t border-l border-white/[0.06] bg-glass-1" />
      )}
      {server && !friendsOpen ? (
        <aside className={`${fullscreen ? "hidden" : "flex"} col-2 row-2 min-h-0 flex-col`}>
          <ServerSidebar server={server} />
        </aside>
      ) : (
        <RoomSidebar hidden={fullscreen} className="col-2 row-2" />
      )}
      <UserDock hidden={fullscreen} className="col-[1/3] row-3" />

      <div
        className={`z-[1] flex min-h-0 min-w-0 flex-col ${fullscreen ? "" : "col-3 row-[2/4] border-t border-white/[0.06]"}`}
      >
        <main
          className={`flex min-h-0 flex-1 flex-col items-center justify-center gap-4 text-center ${
            fullscreen
              ? "bg-surface-2 p-0"
              : friendsOpen
                ? "bg-glass-2 p-0"
                : code && !watching && !live
                  ? // O canal de voz ocupa a área toda, com fundo preto, como no Discord.
                    "bg-black p-0"
                  : `bg-glass-2 px-4 pt-4 ${watching || live ? "pb-11" : "pb-4"}`
          }`}
        >
          {friendsOpen ? (
            <FriendsPage />
          ) : server && !code ? (
            <ServerHome server={server} />
          ) : watching ? (
            <WatchView member={watching} />
          ) : live ? (
            <StreamView />
          ) : (
            <VoiceRoom />
          )}
        </main>
      </div>
    </div>
  );
}

/** Convite por link: abre o modal de entrar com o código preenchido; a pessoa confirma. */
function InvitePrompt() {
  const invite = useRoomStore((s) => s.invite);
  const setInvite = useRoomStore((s) => s.setInvite);
  if (!invite) return null;
  return (
    <RoomDialog
      key={invite}
      initialMode="join"
      initialCode={invite}
      open
      onOpenChange={(open) => {
        if (!open) setInvite(null);
      }}
    />
  );
}
