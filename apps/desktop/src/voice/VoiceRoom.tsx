import { Eye, HeadphoneOff, Loader2, MicOff, MonitorUp, PhoneOff, Radio, UserPlus, Volume2 } from "lucide-react";
import { useAccountStore } from "../account/account-store";
import { inviteUrl } from "../room/invite";
import { MemberMenu } from "../room/MemberMenu";
import { type Member, memberPhoto, roomTitle, useRoomStore } from "../room/room-store";
import { StreamDialog } from "../stream/StreamDialog";
import { Avatar, avatarColor } from "../ui/Avatar";
import { toast } from "../ui/toast-store";
import { Tooltip } from "../ui/Tooltip";
import { useState } from "react";
import { DeafenButton, MicButton } from "./VoiceButtons";
import { useVoiceStore } from "./voice-store";

/** Quantas colunas para `n` pessoas (como a grade de um canal de voz do Discord). */
export function gridColumns(n: number): number {
  if (n <= 1) return 1;
  if (n <= 4) return 2;
  if (n <= 6) return 3;
  return 4;
}

/**
 * A sala como um canal de voz do Discord: um quadro por pessoa (borda verde quando fala, mudo e
 * surdo no nome, "AO VIVO" para assistir) e a barra de controles embaixo.
 */
export function VoiceRoom() {
  const { code, roomName, members, pendingWatch, watch, leave } = useRoomStore();
  const speaking = useVoiceStore((s) => s.speaking);
  const myPrefs = useVoiceStore((s) => s.prefs);
  const profile = useAccountStore((s) => s.profile);
  // O nome no topo e os controles embaixo só aparecem com o mouse na área (ou o foco do teclado),
  // como no Discord. O evento do React conta os menus abertos (em portal) como dentro da área.
  const [pointerIn, setPointerIn] = useState(false);
  const [focusIn, setFocusIn] = useState(false);
  if (!code) return null;
  const chrome = pointerIn || focusIn;
  const chromeClass = `transition-[opacity,translate] duration-200 ease-out ${
    chrome ? "opacity-100 translate-y-0" : "pointer-events-none opacity-0"
  }`;
  const columns = gridColumns(members.length);

  async function invite() {
    try {
      await navigator.clipboard.writeText(inviteUrl(code!));
      toast.success("Link copiado. Cole no Discord para a galera entrar.", "Convite");
    } catch {
      toast.warning(`Não deu para copiar. O link é ${inviteUrl(code!)}`);
    }
  }

  return (
    <section
      aria-label="Canal de voz"
      data-chrome={chrome || undefined}
      onMouseEnter={() => setPointerIn(true)}
      onMouseLeave={() => setPointerIn(false)}
      onFocus={() => setFocusIn(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusIn(false);
      }}
      className="relative flex h-full w-full flex-col overflow-hidden bg-black"
    >
      {/* Topo: nome da sala sobre uma sombra que some para baixo. */}
      <header
        className={`absolute inset-x-0 top-0 z-10 flex h-16 items-start gap-2 bg-gradient-to-b from-black/80 to-transparent px-4 pt-3 text-left font-semibold ${chromeClass} ${
          chrome ? "" : "-translate-y-2"
        }`}
      >
        <Volume2 size={20} className="text-text-muted" />
        <span className="truncate">{roomTitle(code, roomName)}</span>
      </header>

      <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden px-4 py-14">
        <ul
          aria-label="Quem está na voz"
          className="flex w-full flex-wrap content-center justify-center gap-3"
          style={{ maxWidth: columns === 1 ? 900 : undefined }}
        >
          {members.map((m, i) => {
            const isMe = i === 0;
            return (
              <li
                key={m.id}
                className="min-w-0"
                style={{ flexBasis: `calc((100% - ${(columns - 1) * 12}px) / ${columns})` }}
              >
                <MemberMenu member={m} isMe={isMe} onWatch={m.live && !isMe ? () => void watch(m) : undefined}>
                  <Tile
                    member={m}
                    isMe={isMe}
                    speaking={!!speaking[isMe ? "me" : m.id]}
                    muted={isMe ? myPrefs.muted || myPrefs.deafened : !!m.muted}
                    deafened={isMe ? myPrefs.deafened : !!m.deafened}
                    photo={memberPhoto(m, isMe, profile, 128)}
                    pending={pendingWatch === m.id}
                    onWatch={() => void watch(m)}
                  />
                </MemberMenu>
              </li>
            );
          })}
        </ul>
      </div>

      {/* Embaixo: os controles sobre uma sombra que sobe. */}
      <nav
        aria-label="Controles da voz"
        className={`absolute inset-x-0 bottom-0 z-10 flex h-24 items-end justify-center gap-2 bg-gradient-to-t from-black/80 to-transparent pb-4 ${chromeClass} ${
          chrome ? "" : "translate-y-2"
        }`}
      >
        <MicButton large />
        <DeafenButton large />
        <StreamDialog
          trigger={
            <ControlButton label="Transmitir minha tela">
              <MonitorUp size={20} />
            </ControlButton>
          }
        />
        <ControlButton label="Convidar pessoas" onClick={() => void invite()}>
          <UserPlus size={20} />
        </ControlButton>
        <Tooltip label="Sair da sala" side="top">
          <button
            type="button"
            aria-label="Sair da sala"
            onClick={() => void leave()}
            className="grid h-10 w-14 place-items-center rounded-md bg-live text-white transition-colors hover:bg-live/85"
          >
            <PhoneOff size={20} />
          </button>
        </Tooltip>
      </nav>
    </section>
  );
}

function Tile(props: {
  member: Member;
  isMe: boolean;
  speaking: boolean;
  muted: boolean;
  deafened: boolean;
  photo?: string;
  pending: boolean;
  onWatch: () => void;
}) {
  const { member } = props;
  const color = avatarColor(member.name);
  return (
    <div
      data-speaking={props.speaking || undefined}
      className={`group relative flex aspect-video w-full items-center justify-center overflow-hidden rounded-lg border-2 transition-colors duration-150 ${
        props.speaking ? "border-online" : "border-transparent"
      }`}
      // Fundo na cor do avatar, escurecido, como os quadros do Discord.
      style={{ background: `color-mix(in srgb, ${color} 45%, #111214)` }}
    >
      <Avatar name={member.name} size={80} speaking={props.speaking} src={props.photo} />

      {member.live && (
        <span className="absolute top-2 left-2 flex items-center gap-1 rounded bg-live px-1.5 py-0.5 text-[11px] font-bold text-white">
          <Radio size={11} /> AO VIVO
        </span>
      )}
      {member.live && !props.isMe && (
        <div className="absolute inset-0 grid place-items-center bg-black/50 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <button
            type="button"
            onClick={props.onWatch}
            disabled={props.pending}
            className="flex items-center gap-2 rounded-md bg-surface-0/90 px-4 py-2 text-sm font-semibold transition-colors hover:bg-surface-0"
          >
            {props.pending ? <Loader2 size={16} className="animate-spin" /> : <Eye size={16} />}
            {props.pending ? "Conectando…" : "Assistir a transmissão"}
          </button>
        </div>
      )}

      <span className="absolute bottom-2 left-2 flex max-w-[calc(100%-16px)] items-center gap-1.5 rounded-md bg-black/60 px-2 py-1 text-sm font-medium">
        <span className="truncate">{member.name}</span>
        {(props.muted || props.deafened) && (
          <MicOff size={14} aria-label="Microfone silenciado" className="shrink-0 text-live" />
        )}
        {props.deafened && <HeadphoneOff size={14} aria-label="Ensurdecido" className="shrink-0 text-live" />}
      </span>
    </div>
  );
}

function ControlButton(props: { label: string; onClick?: () => void; children: React.ReactNode }) {
  return (
    <Tooltip label={props.label} side="top">
      <button
        type="button"
        aria-label={props.label}
        onClick={props.onClick}
        className="grid size-10 place-items-center rounded-md bg-surface-3 text-text transition-colors hover:bg-surface-hover"
      >
        {props.children}
      </button>
    </Tooltip>
  );
}
