import * as RadixTooltip from "@radix-ui/react-tooltip";
import { useAccountStore } from "../account/account-store";
import { Avatar } from "../ui/Avatar";
import { type Member, memberPhoto, useRoomStore, watches } from "./room-store";

/** Quem da sala está assistindo `streamerId` (cada app conta para a sala quem assiste). */
export function viewersOf(members: Member[], streamerId: string): Member[] {
  return members.filter((m) => m.id !== streamerId && watches(m, streamerId));
}

/** Quantas fotos aparecem empilhadas; o resto vira "+N". */
export const STACKED = 3;

/**
 * Quem está assistindo uma live, como no Discord: as fotos empilhadas (até 3, e "+N" com o resto)
 * e, com o mouse em cima, a lista "Espectadores – X" com a foto e o nome de cada um. Sem ninguém
 * assistindo, não mostra nada.
 */
export function Viewers(props: { streamerId: string; side?: "top" | "bottom" }) {
  const members = useRoomStore((s) => s.members);
  const profile = useAccountStore((s) => s.profile);
  const viewers = viewersOf(members, props.streamerId);
  if (viewers.length === 0) return null;
  const me = members[0]?.id;
  const photo = (m: Member, size: number) => memberPhoto(m, m.id === me, profile, size);
  const name = (m: Member) => (m.id === me ? `${m.name} (você)` : m.name);
  const shown = viewers.slice(0, STACKED);
  const count = `${viewers.length} ${viewers.length === 1 ? "pessoa assistindo" : "pessoas assistindo"}`;

  return (
    <RadixTooltip.Provider delayDuration={100} skipDelayDuration={300}>
      <RadixTooltip.Root>
        <RadixTooltip.Trigger asChild>
          <span
            role="group"
            aria-label={count}
            // Com o teclado também abre a lista.
            tabIndex={0}
            className="flex shrink-0 items-center -space-x-2 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {shown.map((m) => (
              <span key={m.id} className="rounded-full ring-2 ring-black">
                <Avatar name={m.name} size={24} src={photo(m, 64)} />
              </span>
            ))}
            {viewers.length > shown.length && (
              <span className="grid size-6 place-items-center rounded-full bg-surface-3 text-[10px] font-bold text-text ring-2 ring-black">
                +{viewers.length - shown.length}
              </span>
            )}
          </span>
        </RadixTooltip.Trigger>
        <RadixTooltip.Portal>
          <RadixTooltip.Content
            side={props.side ?? "bottom"}
            align="start"
            sideOffset={8}
            className="z-50 w-56 rounded-lg bg-surface-float p-3 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=delayed-open]:animate-tooltip-in"
          >
            <p className="mb-2 text-xs font-bold tracking-wide text-text-muted uppercase">
              Espectadores – {viewers.length}
            </p>
            <ul aria-label="Espectadores" className="flex max-h-64 flex-col gap-2 overflow-y-auto">
              {viewers.map((m) => (
                <li key={m.id} className="flex min-w-0 items-center gap-2 text-sm font-medium text-text">
                  <Avatar name={m.name} size={24} src={photo(m, 64)} />
                  <span className="truncate">{name(m)}</span>
                </li>
              ))}
            </ul>
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}
