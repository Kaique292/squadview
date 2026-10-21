import { Volume2 } from "lucide-react";
import { ServerIcon } from "./ServerIcon";
import { type Server, useServersStore } from "./servers-store";

/** O servidor aberto sem estar numa sala dele: o nome e as salas para entrar. */
export function ServerHome(props: { server: Server }) {
  const enterRoom = useServersStore((s) => s.enterRoom);
  const { server } = props;
  return (
    <section aria-label={server.name} className="flex max-w-md flex-col items-center gap-4 px-6">
      <span className="overflow-hidden rounded-3xl shadow-lg shadow-black/30">
        <ServerIcon name={server.name} icon={server.icon} size={96} />
      </span>
      <div>
        <h2 className="text-2xl font-bold">{server.name}</h2>
        <p className="text-sm text-text-muted">
          {server.members.length} {server.members.length === 1 ? "membro" : "membros"}. Escolha uma sala para entrar.
        </p>
      </div>
      <ul aria-label="Entrar numa sala" className="flex w-full flex-col gap-1.5">
        {server.rooms.map((room) => (
          <li key={room.id}>
            <button
              type="button"
              onClick={() => void enterRoom(server, room)}
              className="flex w-full items-center gap-2 rounded-lg bg-surface-1 px-4 py-3 text-left font-medium transition-colors hover:bg-surface-hover"
            >
              <Volume2 size={18} className="text-text-muted" />
              {room.name}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
