import { Eye } from "lucide-react";
import { useAccountStore } from "../account/account-store";
import { Avatar } from "../ui/Avatar";
import { Tooltip } from "../ui/Tooltip";
import { type Member, memberPhoto, useRoomStore, watches } from "./room-store";

/** Quem da sala está assistindo `streamerId` (cada app conta para a sala quem assiste). */
export function viewersOf(members: Member[], streamerId: string): Member[] {
  return members.filter((m) => m.id !== streamerId && watches(m, streamerId));
}

/** "Ana", "Ana e Beto", "Ana, Beto e Caio". */
export function namesList(names: string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} e ${names[names.length - 1]}`;
}

/**
 * Quantos e quem estão assistindo uma transmissão, como no Discord: as fotos empilhadas e o número,
 * com os nomes na dica. `compact`: só o olhinho e o número (na lista da sala).
 */
export function Viewers(props: { streamerId: string; compact?: boolean; side?: "top" | "bottom" }) {
  const members = useRoomStore((s) => s.members);
  const profile = useAccountStore((s) => s.profile);
  const viewers = viewersOf(members, props.streamerId);
  if (props.compact && viewers.length === 0) return null;
  const label =
    viewers.length === 0
      ? "Ninguém assistindo ainda"
      : `Assistindo: ${namesList(viewers.map((m) => (m.id === members[0]?.id ? `${m.name} (você)` : m.name)))}`;
  const count = `${viewers.length} ${viewers.length === 1 ? "pessoa assistindo" : "pessoas assistindo"}`;

  if (props.compact) {
    return (
      <Tooltip label={label} side={props.side ?? "top"}>
        <span
          role="group"
          aria-label={count}
          className="flex shrink-0 items-center gap-0.5 text-[11px] font-semibold text-text-muted tabular-nums"
        >
          <Eye size={12} /> {viewers.length}
        </span>
      </Tooltip>
    );
  }

  const shown = viewers.slice(0, 3);
  return (
    <Tooltip label={label} side={props.side ?? "top"}>
      <span role="group" aria-label={count} className="flex items-center gap-1.5 text-sm text-text-muted">
        <Eye size={16} />
        <span className="font-semibold text-text tabular-nums">{viewers.length}</span>
        {shown.length > 0 && (
          <span className="flex -space-x-1.5">
            {shown.map((m) => (
              <span key={m.id} className="rounded-full ring-2 ring-surface-1">
                <Avatar name={m.name} size={20} src={memberPhoto(m, m.id === members[0]?.id, profile, 64)} />
              </span>
            ))}
            {viewers.length > shown.length && (
              <span className="grid size-5 place-items-center rounded-full bg-surface-3 text-[10px] font-bold text-text ring-2 ring-surface-1">
                +{viewers.length - shown.length}
              </span>
            )}
          </span>
        )}
      </span>
    </Tooltip>
  );
}
