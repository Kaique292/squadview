import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";
import { create } from "zustand";
import { inviteUrl } from "../room/invite";
import { useRoomStore } from "../room/room-store";
import { useStreamStore } from "../stream/stream-store";

const SHOW_KEY = "squadview.presenca";
const INVITE_KEY = "squadview.presenca.convite";

function read(key: string, fallback: boolean): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

function write(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // Sem armazenamento: vale só até fechar o app.
  }
}

interface PresenceSettings {
  /** Mostrar no perfil do Discord que está numa sala. */
  show: boolean;
  /** Botão "Entrar na sala" no perfil (qualquer um que vir o perfil pode entrar). */
  shareInvite: boolean;
  setShow: (v: boolean) => void;
  setShareInvite: (v: boolean) => void;
}

export const usePresenceSettings = create<PresenceSettings>((set) => ({
  show: read(SHOW_KEY, true),
  shareInvite: read(INVITE_KEY, false),
  setShow(show) {
    write(SHOW_KEY, show);
    set({ show });
  },
  setShareInvite(shareInvite) {
    write(INVITE_KEY, shareInvite);
    set({ shareInvite });
  },
}));

/** O que vai para o Rust (espelha `presence::PresenceInfo`), ou `null` para limpar. */
export function presenceInfo(s: {
  show: boolean;
  shareInvite: boolean;
  code: string | null;
  joinedAt: number | null;
  members: number;
  live: boolean;
  watching: string | null;
}) {
  if (!s.show || !s.code) return null;
  return {
    members: s.members,
    live: s.live,
    watching: s.watching,
    joinedAtMs: s.joinedAt ?? Date.now(),
    inviteUrl: s.shareInvite ? inviteUrl(s.code) : null,
  };
}

/** Mantém o Rich Presence em dia com a sala (só manda quando muda). */
export function usePresenceSync() {
  const { show, shareInvite } = usePresenceSettings();
  const code = useRoomStore((s) => s.code);
  const joinedAt = useRoomStore((s) => s.joinedAt);
  const members = useRoomStore((s) => s.members.length);
  // A primeira live que assistimos (com várias, o Discord mostra só uma).
  const watching = useRoomStore((s) => Object.values(s.watching)[0]?.member.name ?? null);
  const live = useStreamStore((s) => s.status === "live");

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const info = presenceInfo({ show, shareInvite, code, joinedAt, members, live, watching });
    void invoke("set_presence", { info }).catch(() => {});
  }, [show, shareInvite, code, joinedAt, members, live, watching]);
}
