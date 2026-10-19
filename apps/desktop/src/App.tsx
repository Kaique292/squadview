import { Eye, MonitorUp } from "lucide-react";
import { RoomDialog } from "./room/RoomDialog";
import { RoomSidebar, UserDock } from "./room/RoomSidebar";
import { ServerRail } from "./room/ServerRail";
import { WatchView } from "./room/WatchView";
import { useRoomStore } from "./room/room-store";
import { StreamDialog } from "./stream/StreamDialog";
import { StreamView } from "./stream/StreamView";
import { usePresenceSync } from "./settings/presence-store";
import { useStreamStore } from "./stream/stream-store";
import { Dissolve } from "./ui/Dissolve";
import { TitleBar } from "./ui/TitleBar";
import { Toaster } from "./ui/Toaster";
import { UpdateBanner } from "./update/UpdateBanner";

/**
 * Layout principal, na hierarquia do Discord: barra de título, barra de salas, barra lateral com
 * o canto arredondado, área do stream e o card do usuário flutuando no pé das duas colunas.
 */
export function App() {
  const live = useStreamStore((s) => s.status === "live");
  const fullscreen = useStreamStore((s) => s.fullscreen);
  const watching = useRoomStore((s) => s.watching);
  const code = useRoomStore((s) => s.code);
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
      <Toaster anchor={(watching || live) && !fullscreen ? "sidebar" : "center"} />
      {!fullscreen && <TitleBar title={code ? `Sala ${code}` : undefined} />}
      <ServerRail hidden={fullscreen} />
      {/* Moldura da lateral: o canto arredondado e a borda fina do Discord. */}
      {!fullscreen && (
        <div aria-hidden className="col-2 row-[2/4] rounded-tl-lg border-t border-l border-white/[0.06] bg-glass-1" />
      )}
      <RoomSidebar hidden={fullscreen} className="col-2 row-2" />
      <UserDock hidden={fullscreen} className="col-[1/3] row-3" />

      <div
        className={`z-[1] flex min-h-0 min-w-0 flex-col ${fullscreen ? "" : "col-3 row-[2/4] border-t border-white/[0.06]"}`}
      >
        <UpdateBanner />
        <main
          className={`flex min-h-0 flex-1 flex-col items-center justify-center gap-4 text-center ${
            fullscreen ? "bg-surface-2 p-0" : `bg-glass-2 px-4 pt-4 ${watching || live ? "pb-11" : "pb-4"}`
          }`}
        >
          {watching ? <WatchView member={watching} /> : live ? <StreamView /> : <Empty />}
        </main>
      </div>
    </div>
  );
}

/** Ninguém na tela: convida a transmitir ou a assistir quem já está ao vivo. */
function Empty() {
  const { code, members, pendingWatch, watch } = useRoomStore();
  const liveFriends = members.slice(1).filter((m) => m.live);

  if (liveFriends.length > 0) {
    return (
      <>
        <MonitorUp size={56} className="text-text-faint" />
        <h1 className="text-2xl font-semibold">
          {liveFriends.length === 1 ? `${liveFriends[0].name} está ao vivo` : `${liveFriends.length} amigos ao vivo`}
        </h1>
        <div className="flex flex-wrap justify-center gap-2">
          {liveFriends.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => void watch(m)}
              disabled={pendingWatch === m.id}
              className="flex items-center gap-2 rounded-md bg-brand px-4 py-2 font-medium transition-colors hover:bg-brand-hover disabled:opacity-60"
            >
              <Eye size={16} /> {pendingWatch === m.id ? `Conectando com ${m.name}…` : `Assistir ${m.name}`}
            </button>
          ))}
        </div>
        <StreamDialog />
      </>
    );
  }

  return (
    <>
      <MonitorUp size={56} className="text-text-faint" />
      <h1 className="text-2xl font-semibold">Ninguém está transmitindo ainda</h1>
      <p className="max-w-sm text-text-muted">
        {code
          ? "Mande o código da sala para a galera e comece a transmitir. Quem entrar pode assistir na hora."
          : "Crie uma sala, chame a galera e compartilhe a tela. A própria sala distribui o vídeo entre vocês."}
      </p>
      <StreamDialog />
    </>
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
