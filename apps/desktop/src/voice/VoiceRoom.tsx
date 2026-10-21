import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  ChevronDown,
  ChevronUp,
  Eye,
  HeadphoneOff,
  Info,
  Loader2,
  MicOff,
  MonitorUp,
  PhoneOff,
  TriangleAlert,
  UserPlus,
  Volume2,
} from "lucide-react";
import { type HTMLAttributes, type KeyboardEvent, type ReactNode, type Ref, useEffect, useRef, useState } from "react";
import { useAccountStore } from "../account/account-store";
import { inviteUrl } from "../room/invite";
import { MemberMenu } from "../room/MemberMenu";
import { type Member, memberPhoto, roomTitle, useRoomStore } from "../room/room-store";
import { StreamDetailsDialog } from "../stream/StreamDetailsDialog";
import { StreamDialog } from "../stream/StreamDialog";
import { MENU_CONTENT, StreamMenuItems } from "../stream/StreamMenu";
import { StreamTile, type StreamTileVariant } from "../stream/StreamTile";
import { WatchDetailsDialog } from "../stream/WatchDetailsDialog";
import { WatchTile } from "../stream/WatchTile";
import { onVideoEnter, useStreamStore } from "../stream/stream-store";
import { OWN_VIDEO } from "../stream/video-pointer";
import { useOverlayMenu } from "../ui/overlay";
import { Avatar } from "../ui/Avatar";
import { toast } from "../ui/toast-store";
import { Tooltip } from "../ui/Tooltip";
import { useMemberEffect } from "./speaking-effect";
import { DeafenButton, MicButton } from "./VoiceButtons";
import { tileGlows, tileGradient } from "./tile-background";
import { useVoiceStore } from "./voice-store";
import { LiveBadge } from "../ui/LiveBadge";

/** Quantas colunas para `n` pessoas (como a grade de um canal de voz do Discord). */
export function gridColumns(n: number): number {
  if (n <= 1) return 1;
  if (n <= 4) return 2;
  if (n <= 6) return 3;
  return 4;
}

/** Como cada quadro aparece: na grade com todos, grande em foco ou pequeno na faixa de baixo. */
type TileVariant = "grid" | "focus" | "strip";

/**
 * A sala como um canal de voz do Discord: um quadro por pessoa (borda verde quando fala, mudo e
 * surdo no nome, "Ao vivo" para assistir) e a barra de controles embaixo. Clicar num quadro põe a
 * pessoa em foco (grande no centro, os outros numa faixa embaixo); clicar nele de novo volta à grade.
 */
export function VoiceRoom() {
  const { code, roomName, members, pendingWatch, watching, watch, leave } = useRoomStore();
  const speaking = useVoiceStore((s) => s.speaking);
  const myPrefs = useVoiceStore((s) => s.prefs);
  const profile = useAccountStore((s) => s.profile);
  // O nome no topo e os controles embaixo só aparecem com o mouse na área (ou o foco do teclado),
  // como no Discord. O evento do React conta os menus abertos (em portal) como dentro da área.
  const [pointerIn, setPointerIn] = useState(false);
  const [focusIn, setFocusIn] = useState(false);
  // Foco que veio de um clique (não do teclado) não segura os controles depois que o mouse sai.
  const clicked = useRef(false);
  // Sobre o vídeo nativo o mouse sai da página sem ela saber: quem avisa é o Rust. Ao entrar no
  // vídeo, o "mouse em cima" da página deixa de valer (senão ficava preso ligado ao sair da janela).
  const videoHover = useStreamStore((s) => s.videoHover !== null);
  useEffect(() => onVideoEnter(() => setPointerIn(false)), []);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [stripOpen, setStripOpen] = useState(true);
  // A nossa live é um quadro a mais, antes das pessoas (como no Discord).
  const live = useStreamStore((s) => s.status === "live");
  const fullscreen = useStreamStore((s) => s.fullscreen);
  const fullscreenOf = useStreamStore((s) => s.fullscreenOf);
  const notice = useStreamStore((s) => s.notice);
  const [changing, setChanging] = useState(false);
  const [details, setDetails] = useState(false);
  // De qual live são os detalhes abertos (continua lá enquanto o modal fecha, animando).
  const [watchDetails, setWatchDetails] = useState<{ memberId: string; open: boolean } | null>(null);
  // As lives que assistimos são quadros também, logo depois da nossa (várias ao mesmo tempo, ADR 0019).
  const watched = Object.keys(watching);
  const ids = [...(live ? [STREAM_TILE] : []), ...watched.map(watchTileId), ...members.map((m) => m.id)];
  const focusIndex = focusId ? ids.indexOf(focusId) : -1;
  // Quem estava em foco saiu da sala (ou a live parou): volta para a grade.
  if (focusId && focusIndex < 0) setFocusId(null);
  // Começou a assistir alguém: a live dele vem para o foco, como no Discord.
  useEffect(
    () =>
      useRoomStore.subscribe((s, prev) => {
        const fresh = Object.keys(s.watching).filter((id) => !(id in prev.watching));
        if (fresh.length > 0) setFocusId(watchTileId(fresh[fresh.length - 1]));
      }),
    [],
  );

  const dialogs = (
    <>
      <StreamDialog open={changing} onOpenChange={setChanging} />
      <StreamDetailsDialog open={details} onOpenChange={setDetails} />
      <WatchDetailsDialog
        memberId={watchDetails?.memberId ?? null}
        open={!!watchDetails?.open}
        onOpenChange={(open) => setWatchDetails((d) => d && { ...d, open })}
      />
    </>
  );
  const streamTile = (variant: StreamTileVariant) => (
    <StreamTile
      variant={variant}
      focused={focusId === STREAM_TILE}
      onActivate={() => setFocusId(focusId === STREAM_TILE ? null : STREAM_TILE)}
      onChange={() => setChanging(true)}
      onDetails={() => setDetails(true)}
    />
  );
  const watchTile = (memberId: string, variant: StreamTileVariant, hidden = false) => (
    <WatchTile
      memberId={memberId}
      variant={variant}
      hidden={hidden}
      focused={focusId === watchTileId(memberId)}
      onActivate={() => setFocusId(focusId === watchTileId(memberId) ? null : watchTileId(memberId))}
      onDetails={() => setWatchDetails({ memberId, open: true })}
    />
  );
  // Tela cheia numa live que assistimos: só ela, ocupando tudo.
  if (fullscreen && fullscreenOf && watching[fullscreenOf]) {
    return (
      <section aria-label={`Live de ${watching[fullscreenOf].member.name}`} className="h-full w-full bg-black">
        {watchTile(fullscreenOf, "fullscreen")}
        {dialogs}
      </section>
    );
  }
  // Tela cheia (ou ao vivo fora de uma sala): só a nossa live, ocupando tudo.
  if (live && (fullscreen || !code)) {
    return (
      <section aria-label="Sua transmissão" className={`h-full w-full bg-black ${fullscreen ? "" : "p-4"}`}>
        {streamTile(fullscreen ? "fullscreen" : "focus")}
        {dialogs}
      </section>
    );
  }
  if (!code) return null;
  const chrome = pointerIn || videoHover || focusIn;
  const chromeClass = `transition-[opacity,translate] duration-200 ease-out ${
    chrome ? "opacity-100 translate-y-0" : "pointer-events-none opacity-0"
  }`;
  const columns = gridColumns(ids.length);
  // Cada pessoa com o seu fundo em degradê, sem repetir o de ninguém da sala.
  const glows = tileGlows(members.map((m) => m.id));
  const others = ids.map((_, i) => i).filter((i) => i !== focusIndex);
  const stripLabel = stripOpen ? "Esconder participantes" : "Mostrar participantes";

  async function invite() {
    try {
      await navigator.clipboard.writeText(inviteUrl(code!));
      toast.success("Link copiado. Cole no Discord para a galera entrar.", "Convite");
    } catch {
      toast.warning(`Não deu para copiar. O link é ${inviteUrl(code!)}`);
    }
  }

  /** Assistir quem já assistimos só traz a live dele para o foco. */
  function watchOrFocus(m: Member) {
    if (watching[m.id]) setFocusId(watchTileId(m.id));
    else void watch(m);
  }

  /**
   * O quadro em `index` de `ids`: a nossa live, uma live que assistimos ou uma pessoa (a primeira
   * pessoa somos nós). `hidden`: está na faixa recolhida.
   */
  function tile(index: number, variant: TileVariant, hidden = false) {
    if (ids[index] === STREAM_TILE) return streamTile(variant);
    if (index - (live ? 1 : 0) < watched.length) return watchTile(watched[index - (live ? 1 : 0)], variant, hidden);
    const memberIndex = index - (live ? 1 : 0) - watched.length;
    const m = members[memberIndex];
    const isMe = memberIndex === 0;
    const focused = m.id === focusId;
    return (
      <MemberMenu member={m} isMe={isMe} onWatch={m.live && !isMe ? () => watchOrFocus(m) : undefined}>
        <Tile
          glow={glows[m.id]}
          member={m}
          isMe={isMe}
          variant={variant}
          focused={focused}
          speaking={!!speaking[isMe ? "me" : m.id]}
          muted={isMe ? myPrefs.muted || myPrefs.deafened : !!m.muted}
          deafened={isMe ? myPrefs.deafened : !!m.deafened}
          photo={memberPhoto(m, isMe, profile, 128)}
          pending={pendingWatch.includes(m.id)}
          watching={!!watching[m.id]}
          onWatch={() => watchOrFocus(m)}
          onActivate={() => setFocusId(focused ? null : m.id)}
        />
      </MemberMenu>
    );
  }

  return (
    <section
      aria-label="Canal de voz"
      data-chrome={chrome || undefined}
      onMouseEnter={() => setPointerIn(true)}
      onMouseLeave={() => setPointerIn(false)}
      onPointerDown={() => {
        clicked.current = true;
      }}
      onFocus={() => {
        setFocusIn(!clicked.current);
        clicked.current = false;
      }}
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

      {focusIndex < 0 ? (
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden px-4 py-14">
          <ul
            aria-label="Quem está na voz"
            className="flex w-full animate-focus-in flex-wrap content-center justify-center gap-3"
            style={{ maxWidth: columns === 1 ? 900 : undefined }}
          >
            {ids.map((id, i) => (
              <li
                key={id}
                className="min-w-0"
                style={{ flexBasis: `calc((100% - ${(columns - 1) * 12}px) / ${columns})` }}
              >
                {tile(i, "grid")}
              </li>
            ))}
          </ul>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col px-4 pt-14 pb-14">
          {/* Quem está em foco, grande no centro (a chave refaz a animação ao trocar de pessoa). */}
          <div key={focusId} role="group" aria-label="Em foco" className="min-h-0 flex-1 animate-focus-in">
            {tile(focusIndex, "focus")}
          </div>
          {others.length > 0 && (
            <>
              <div className={`flex justify-center py-1.5 ${chromeClass}`}>
                <Tooltip label={stripLabel} side="top">
                  <button
                    type="button"
                    aria-label={stripLabel}
                    aria-expanded={stripOpen}
                    aria-controls="faixa-da-voz"
                    onClick={() => setStripOpen((v) => !v)}
                    className="grid h-6 w-10 place-items-center rounded-full bg-surface-3/80 text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
                  >
                    {stripOpen ? <ChevronDown size={16} /> : <ChevronUp size={16} />}
                  </button>
                </Tooltip>
              </div>
              {/* A faixa recolhe animando a altura da linha da grade (0fr ↔ 1fr). */}
              <div
                id="faixa-da-voz"
                className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out ${
                  stripOpen ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"
                }`}
              >
                <div className="min-h-0 overflow-hidden">
                  <ul
                    aria-label="Outros na voz"
                    inert={!stripOpen}
                    className="mx-auto flex w-max max-w-full gap-2 overflow-x-auto pb-1"
                  >
                    {others.map((i) => (
                      <li key={ids[i]} className="h-28 shrink-0 animate-focus-in">
                        {tile(i, "strip", !stripOpen)}
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            </>
          )}
        </div>
      )}

      {live && notice && (
        <p
          role="status"
          data-video-hole
          className={`absolute top-14 left-1/2 z-20 flex max-w-[min(560px,calc(100%-32px))] -translate-x-1/2 items-start gap-2 rounded-lg px-3 py-2 text-left text-sm shadow-lg ${
            notice.kind === "source-changed" ? "bg-brand" : "bg-warning text-black"
          }`}
        >
          {notice.kind === "source-changed" ? (
            <Info size={16} className="mt-0.5 shrink-0" />
          ) : (
            <TriangleAlert size={16} className="mt-0.5 shrink-0" />
          )}
          {notice.message}
        </p>
      )}
      {dialogs}

      {/* Embaixo: os controles sobre uma sombra que sobe. A sombra não pega o mouse (cobria a
          setinha da faixa de participantes): só os botões, e só quando aparecem. */}
      <nav
        aria-label="Controles da voz"
        className={`pointer-events-none absolute inset-x-0 bottom-0 z-10 flex h-24 items-end justify-center gap-3 bg-gradient-to-t from-black/80 to-transparent pb-4 ${chromeClass} ${
          chrome ? "[&>*]:pointer-events-auto" : "translate-y-2"
        }`}
      >
        {/* Grupos como a barra de chamada do Discord: áudio, depois tela e convite, e o desligar. */}
        <ControlGroup label="Áudio">
          <MicButton large />
          <DeafenButton large />
        </ControlGroup>
        <ControlGroup label="Tela e convite">
          {live ? (
            <StreamControl onChange={() => setChanging(true)} onDetails={() => setDetails(true)} />
          ) : (
            <StreamDialog
              trigger={
                <ControlButton label="Transmitir minha tela">
                  <MonitorUp size={20} />
                </ControlButton>
              }
            />
          )}
          <ControlButton label="Convidar pessoas" onClick={() => void invite()}>
            <UserPlus size={20} />
          </ControlButton>
        </ControlGroup>
        <Tooltip label="Sair da sala" side="top">
          <button
            type="button"
            aria-label="Sair da sala"
            onClick={() => void leave()}
            className="grid h-12 w-16 place-items-center rounded-lg bg-live text-white transition-colors hover:bg-live/85"
          >
            <PhoneOff size={22} />
          </button>
        </Tooltip>
      </nav>
    </section>
  );
}

const AVATAR_SIZE: Record<TileVariant, number> = { grid: 80, focus: 112, strip: 48 };

/**
 * Um quadro da voz. Recebe as props do gatilho do menu do botão direito (o `MemberMenu` usa
 * `asChild`), por isso repassa o resto para a `div`.
 */
function Tile({
  glow,
  member,
  isMe,
  variant,
  focused,
  speaking,
  muted,
  deafened,
  photo,
  pending,
  watching,
  onWatch,
  onActivate,
  ref,
  style,
  ...rest
}: {
  /** A cor do brilho do fundo (ver `tileGlows`). */
  glow: string;
  member: Member;
  isMe: boolean;
  variant: TileVariant;
  focused: boolean;
  speaking: boolean;
  muted: boolean;
  deafened: boolean;
  photo?: string;
  pending: boolean;
  /** Já assistimos a live desta pessoa (o botão só traz o quadro dela para o foco). */
  watching: boolean;
  onWatch: () => void;
  onActivate: () => void;
  ref?: Ref<HTMLDivElement>;
} & HTMLAttributes<HTMLDivElement>) {
  const effect = useMemberEffect(member, isMe);
  const strip = variant === "strip";
  const shape = variant === "focus" ? "h-full w-full" : strip ? "aspect-video h-full" : "aspect-video w-full";
  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    rest.onKeyDown?.(e);
    if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
    e.preventDefault();
    onActivate();
  }
  return (
    <div
      {...rest}
      ref={ref}
      role="button"
      tabIndex={0}
      aria-label={focused ? `Voltar para a grade (${member.name} em foco)` : `Pôr ${member.name} em foco`}
      aria-pressed={focused}
      data-speaking={speaking || undefined}
      onClick={(e) => {
        rest.onClick?.(e);
        onActivate();
      }}
      onKeyDown={onKeyDown}
      className={`group relative flex ${shape} cursor-pointer items-center justify-center overflow-hidden rounded-lg border-2 transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-brand ${
        speaking ? "border-online" : strip ? "border-transparent hover:border-surface-hover" : "border-transparent"
      }`}
      style={{
        ...style,
        // Fundo em degradê, um brilho subindo de baixo, de uma cor só desta pessoa na sala.
        background: tileGradient(glow),
        // Falando com um efeito escolhido: a borda do quadro fica na cor dele.
        ...(speaking && effect ? { borderColor: effect.color } : null),
      }}
    >
      <Avatar name={member.name} size={AVATAR_SIZE[variant]} speaking={speaking} effect={effect} src={photo} />

      {member.live && <LiveBadge size={strip ? "sm" : "md"} className="absolute top-2 right-2" />}
      {member.live && !isMe && !strip && (
        <div className="pointer-events-none absolute inset-0 grid place-items-center bg-black/50 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <button
            type="button"
            onClick={(e) => {
              // Assistir não mexe no foco.
              e.stopPropagation();
              onWatch();
            }}
            disabled={pending}
            className="pointer-events-auto flex items-center gap-2 rounded-md bg-surface-0/90 px-4 py-2 text-sm font-semibold transition-colors hover:bg-surface-0"
          >
            {pending ? <Loader2 size={16} className="animate-spin" /> : <Eye size={16} />}
            {pending ? "Conectando…" : watching ? "Ver a transmissão" : "Assistir a transmissão"}
          </button>
        </div>
      )}

      <span
        className={`absolute bottom-2 left-2 flex max-w-[calc(100%-16px)] items-center gap-1.5 rounded-md bg-black/60 font-medium ${
          strip ? "px-1.5 py-0.5 text-xs" : "px-2 py-1 text-sm"
        }`}
      >
        <span className="truncate">{member.name}</span>
        {(muted || deafened) && <MicOff size={14} aria-label="Microfone silenciado" className="shrink-0 text-live" />}
        {deafened && <HeadphoneOff size={14} aria-label="Ensurdecido" className="shrink-0 text-live" />}
      </span>
    </div>
  );
}

/** O quadro da nossa live (não é uma pessoa: não colide com nenhum id). */
const STREAM_TILE = OWN_VIDEO;

/** O quadro da live de quem assistimos (o da pessoa continua com o id dela). */
function watchTileId(memberId: string) {
  return `assistindo:${memberId}`;
}

/** Transmitindo, o botão de tela da barra fica verde (como no Discord) e abre o menu da live. */
function StreamControl(props: { onChange: () => void; onDetails: () => void }) {
  return (
    <DropdownMenu.Root onOpenChange={useOverlayMenu()}>
      <Tooltip label="Opções da transmissão" side="top">
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            aria-label="Opções da transmissão"
            className="grid size-10 place-items-center rounded-md bg-online/15 text-online transition-colors hover:bg-online/25 data-[state=open]:bg-online/25"
          >
            <MonitorUp size={20} />
          </button>
        </DropdownMenu.Trigger>
      </Tooltip>
      <DropdownMenu.Portal>
        <DropdownMenu.Content side="top" sideOffset={8} aria-label="Opções da transmissão" className={MENU_CONTENT}>
          <StreamMenuItems kind="dropdown" onChange={props.onChange} onDetails={props.onDetails} />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Uma caixa escura com bordas suaves que junta botões da barra da chamada. */
function ControlGroup(props: { label: string; children: ReactNode }) {
  return (
    <div
      role="group"
      aria-label={props.label}
      className="flex items-center gap-1 rounded-lg border border-white/[0.06] bg-surface-0 p-1 shadow-lg shadow-black/30"
    >
      {props.children}
    </div>
  );
}

function ControlButton(props: { label: string; onClick?: () => void; children: ReactNode }) {
  return (
    <Tooltip label={props.label} side="top">
      <button
        type="button"
        aria-label={props.label}
        onClick={props.onClick}
        className="grid size-10 place-items-center rounded-md text-text transition-colors hover:bg-surface-hover"
      >
        {props.children}
      </button>
    </Tooltip>
  );
}
