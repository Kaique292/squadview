import { useAccountStore } from "../account/account-store";
import { type Member, memberPhoto, useRoomStore, watches } from "../room/room-store";
import { useToastStore } from "../ui/toast-store";
import { useStreamStore } from "./stream-store";

/** Mais gente que isso começando a assistir ao mesmo tempo vira um aviso só. */
const MAX_NOTICES = 3;

/** Quem assiste a live de `me` agora. */
function viewersOf(members: Member[], me: string | undefined): Member[] {
  return me ? members.filter((m) => m.id !== me && watches(m, me)) : [];
}

/**
 * Avisa, com a foto, quem começou a assistir a nossa live (ADR 0034). Vem da lista de verdade de
 * quem assiste (cada membro informa o que assiste): quem entra na lista ganha um aviso; quem sai,
 * nada. Fora do ar, a lista zera. Devolve como parar.
 */
export function watchViewerNotices(): () => void {
  let seen = new Set<string>();
  const check = () => {
    const live = useStreamStore.getState().status === "live";
    const { members } = useRoomStore.getState();
    if (!live) {
      seen = new Set();
      return;
    }
    // Nós somos o primeiro da lista.
    const viewers = viewersOf(members, members[0]?.id);
    const fresh = viewers.filter((m) => !seen.has(m.id));
    seen = new Set(viewers.map((m) => m.id));
    if (fresh.length === 0) return;
    const push = useToastStore.getState().push;
    if (fresh.length > MAX_NOTICES) {
      push({ kind: "info", title: "Sua live", message: `${fresh.length} pessoas começaram a assistir` });
      return;
    }
    const profile = useAccountStore.getState().profile;
    for (const m of fresh) {
      push({
        kind: "info",
        title: "Sua live",
        message: `${m.name} começou a assistir`,
        avatar: { name: m.name, src: memberPhoto(m, false, profile, 64) },
      });
    }
  };
  const offRoom = useRoomStore.subscribe((s, prev) => {
    if (s.members !== prev.members) check();
  });
  const offStream = useStreamStore.subscribe((s, prev) => {
    if (s.status !== prev.status) check();
  });
  check();
  return () => {
    offRoom();
    offStream();
  };
}
