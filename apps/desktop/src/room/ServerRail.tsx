import { Plus } from "lucide-react";
import type { ReactNode } from "react";
import logo from "../assets/logo.svg";
import { Tooltip } from "../ui/Tooltip";
import { RoomDialog } from "./RoomDialog";
import { useRoomStore } from "./room-store";

/**
 * Barra de salas (a coluna da esquerda, como a de servidores do Discord): ícones redondos que
 * viram quadrados arredondados com o mouse, e a "pílula" branca na borda indicando onde estamos.
 */
export function ServerRail(props: { hidden: boolean }) {
  const code = useRoomStore((s) => s.code);
  const someoneLive = useRoomStore((s) => s.members.slice(1).some((m) => m.live));

  return (
    <nav
      aria-label="Salas"
      className={`${props.hidden ? "hidden" : "flex"} col-1 row-2 min-h-0 flex-col items-center gap-2 overflow-y-auto pt-1 pb-3`}
    >
      <RailItem label="SquadView" active={!code}>
        <span className="grid size-12 place-items-center bg-brand transition-colors group-hover:bg-brand-hover">
          <img src={logo} alt="SquadView" className="size-8" />
        </span>
      </RailItem>
      <div className="h-0.5 w-8 rounded-full bg-surface-3" />
      {code && (
        <RailItem label={`Sala ${code}`} active badge={someoneLive}>
          <span className="grid size-12 place-items-center bg-surface-3 font-mono text-[11px] font-bold tracking-wider">
            {code.slice(0, 3)}
          </span>
        </RailItem>
      )}
      <RoomDialog
        trigger={
          <button type="button" aria-label="Criar sala" className="group relative">
            <Tooltip label="Criar uma sala" side="right">
              <span className="grid size-12 place-items-center overflow-hidden rounded-[24px] bg-surface-3 text-online transition-all duration-200 ease-out group-hover:rounded-2xl group-hover:bg-online group-hover:text-white">
                <Plus size={22} />
              </span>
            </Tooltip>
          </button>
        }
      />
    </nav>
  );
}

/** Um ícone da barra, com a pílula da esquerda e a dica com o nome. */
function RailItem(props: { label: string; active: boolean; badge?: boolean; children: ReactNode }) {
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
        <span
          aria-current={props.active ? "page" : undefined}
          className={`relative overflow-hidden transition-all duration-200 ease-out ${
            props.active ? "rounded-2xl" : "rounded-[24px] group-hover:rounded-2xl"
          }`}
        >
          {props.children}
        </span>
      </Tooltip>
      {/* Alguém ao vivo na sala: pontinho vermelho, como as notificações do Discord. */}
      {props.badge && (
        <span
          aria-label="Alguém ao vivo"
          className="absolute right-3 bottom-0 size-4 rounded-full border-[3px] border-surface-0 bg-live"
        />
      )}
    </div>
  );
}
