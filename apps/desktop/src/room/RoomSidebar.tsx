import * as Menu from "@radix-ui/react-dropdown-menu";
import {
  ChevronDown,
  Copy,
  Eye,
  HeadphoneOff,
  Loader2,
  LogIn,
  LogOut,
  MicOff,
  MonitorUp,
  Pencil,
  PhoneOff,
  Plus,
  Radio,
  Settings,
  Signal,
  UserPlus,
  Users,
  Volume2,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { DiscordLoginButton } from "../account/DiscordLoginButton";
import { avatarUrl, displayName, useAccountStore } from "../account/account-store";
import { Viewers } from "./Viewers";
import { SettingsDialog } from "../settings/SettingsDialog";
import { StreamDialog } from "../stream/StreamDialog";
import { useStreamStore } from "../stream/stream-store";
import { useOverlayMenu } from "../ui/overlay";
import { ScrollArea } from "../ui/ScrollArea";
import { toast } from "../ui/toast-store";
import { Avatar } from "../ui/Avatar";
import { Tooltip } from "../ui/Tooltip";
import { DeafenButton, MicButton } from "../voice/VoiceButtons";
import { useVoiceStore } from "../voice/voice-store";
import { RoomDialog } from "./RoomDialog";
import { inviteUrl } from "./invite";
import { MemberMenu } from "./MemberMenu";
import { NameDialog } from "./NameDialog";
import { type Member, memberPhoto, roomTitle, useRoomStore } from "./room-store";
import { useMemberEffect, useSpeakingEffectStore } from "../voice/speaking-effect";

/**
 * Barra lateral no estilo da lista de canais do Discord: cabeçalho da sala com menu e a sala como
 * um "canal de voz" com o tempo conectado e os membros embaixo. O painel do usuário fica à parte
 * (`UserDock`), flutuando sobre a barra de salas e a lateral.
 */
export function RoomSidebar(props: { hidden: boolean; className?: string }) {
  const code = useRoomStore((s) => s.code);
  return (
    <aside className={`${props.hidden ? "hidden" : "flex"} min-h-0 flex-col ${props.className ?? ""}`}>
      {code ? <RoomHeader code={code} /> : <Header title="SquadView" />}
      {code ? <RoomChannel /> : <NoRoom />}
    </aside>
  );
}

/**
 * Card flutuante no pé da tela, como o do Discord: "Conectado à sala" (quando há sala) e o painel
 * do usuário, juntos, com cantos arredondados, ocupando a largura da barra de salas e da lateral.
 */
export function UserDock(props: { hidden: boolean; className?: string }) {
  const code = useRoomStore((s) => s.code);
  return (
    <div
      className={`${props.hidden ? "hidden" : "flex"} z-10 m-2 flex-col overflow-hidden rounded-lg border border-white/[0.06] bg-glass-panel shadow-lg backdrop-blur-md shadow-black/30 ${props.className ?? ""}`}
    >
      {code && <ConnectedPanel code={code} />}
      <UserPanel />
    </div>
  );
}

function Header(props: { title: string; children?: ReactNode }) {
  return (
    <header className="flex h-12 shrink-0 items-center border-b border-white/[0.06] px-4 font-semibold">
      {props.title}
      {props.children}
    </header>
  );
}

/** Copia o código e avisa (com notificação). */
async function copyCode(code: string) {
  try {
    await navigator.clipboard.writeText(code);
    toast.success(`Código ${code} copiado.`, "Convite");
  } catch {
    toast.warning(`Não deu para copiar. O código é ${code}.`);
  }
}

/** Copia o link de convite: abre o app de quem tem, ou leva ao download de quem não tem. */
async function copyInvite(code: string) {
  const link = inviteUrl(code);
  try {
    await navigator.clipboard.writeText(link);
    toast.success("Link copiado. Cole no Discord para a galera entrar.", "Convite");
  } catch {
    toast.warning(`Não deu para copiar. O link é ${link}`);
  }
}

function RoomHeader(props: { code: string }) {
  const leave = useRoomStore((s) => s.leave);
  const roomName = useRoomStore((s) => s.roomName);
  const renameRoom = useRoomStore((s) => s.renameRoom);
  const [renaming, setRenaming] = useState(false);
  const title = roomTitle(props.code, roomName);
  const onMenuOpenChange = useOverlayMenu();
  return (
    <div className="flex h-12 shrink-0 items-center border-b border-white/[0.06]">
      <NameDialog
        open={renaming}
        onOpenChange={setRenaming}
        title="Nome da sala"
        description="Todo mundo na sala vê o nome novo na hora."
        label="Nome da sala"
        initial={roomName ?? ""}
        onSave={(name) => void renameRoom(name)}
      />
      <Menu.Root onOpenChange={onMenuOpenChange}>
        <Menu.Trigger asChild>
          <button
            type="button"
            className="group flex h-full min-w-0 flex-1 items-center justify-between gap-2 px-4 text-left font-semibold transition-colors hover:bg-surface-hover data-[state=open]:bg-surface-hover"
          >
            <span className="truncate">{title}</span>
            <ChevronDown
              size={18}
              className="shrink-0 transition-transform duration-200 group-data-[state=open]:rotate-180"
            />
          </button>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            align="start"
            sideOffset={6}
            className="z-50 w-56 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            <MenuItem icon={<UserPlus size={16} />} onSelect={() => void copyInvite(props.code)} accent>
              Convidar pessoas
            </MenuItem>
            <MenuItem icon={<Pencil size={16} />} onSelect={() => setRenaming(true)}>
              Editar nome da sala
            </MenuItem>
            <MenuItem icon={<Copy size={16} />} onSelect={() => void copyCode(props.code)}>
              Copiar código da sala
            </MenuItem>
            <Menu.Separator className="my-1 h-px bg-surface-3" />
            <MenuItem icon={<LogOut size={16} />} onSelect={() => void leave()} danger>
              Sair da sala
            </MenuItem>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
      <Tooltip label="Convidar pessoas" side="bottom">
        <button
          type="button"
          aria-label="Copiar link de convite"
          onClick={() => void copyInvite(props.code)}
          className="mr-2 rounded p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
        >
          <UserPlus size={18} />
        </button>
      </Tooltip>
    </div>
  );
}

function MenuItem(props: {
  icon: ReactNode;
  onSelect: () => void;
  danger?: boolean;
  accent?: boolean;
  children: ReactNode;
}) {
  const color = props.danger
    ? "text-live data-[highlighted]:bg-live data-[highlighted]:text-white"
    : props.accent
      ? "text-[#9a86ff] data-[highlighted]:bg-brand data-[highlighted]:text-white"
      : "text-text-muted data-[highlighted]:bg-brand data-[highlighted]:text-white";
  return (
    <Menu.Item
      onSelect={props.onSelect}
      className={`flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium outline-none ${color}`}
    >
      {props.children}
      {props.icon}
    </Menu.Item>
  );
}

function NoRoom() {
  const profile = useAccountStore((s) => s.profile);
  return (
    <div className="flex flex-1 flex-col gap-2 px-2 py-4 text-sm">
      <p className="flex items-center gap-2 px-2 pb-1 text-xs font-semibold tracking-wide text-text-faint uppercase">
        <Users size={14} /> Nenhuma sala ainda
      </p>
      <RoomDialog
        trigger={
          <button
            type="button"
            className="flex items-center justify-center gap-2 rounded-md bg-brand py-2 font-medium transition-colors hover:bg-brand-hover"
          >
            <Plus size={16} /> Criar sala
          </button>
        }
      />
      <RoomDialog
        initialMode="join"
        trigger={
          <button
            type="button"
            className="flex items-center justify-center gap-2 rounded-md py-1.5 font-medium text-text-muted transition-colors hover:bg-surface-hover/60 hover:text-text"
          >
            <LogIn size={16} /> Entrar com código
          </button>
        }
      />
      {!profile && (
        <div className="mt-3 flex flex-col gap-2 rounded-md bg-surface-0/60 p-3">
          <p className="text-xs text-text-muted">Use seu nome e sua foto do Discord nas salas.</p>
          <DiscordLoginButton />
        </div>
      )}
    </div>
  );
}

/** A sala como um "canal de voz": categoria recolhível, nome com o tempo conectado e os membros. */
function RoomChannel() {
  const { code, roomName, members, pendingWatch, watching, watch, joinedAt } = useRoomStore();
  const [open, setOpen] = useState(true);
  const [me, ...others] = members;

  return (
    <ScrollArea className="min-h-0 flex-1" viewportClassName="px-2 pt-4 pb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="group flex w-full items-center gap-1 px-1 pb-1 text-xs font-semibold tracking-wide text-text-faint uppercase transition-colors hover:text-text-muted"
      >
        <ChevronDown size={12} className={`transition-transform duration-200 ${open ? "" : "-rotate-90"}`} />
        Na sala — {members.length}
      </button>

      <div className="flex items-center gap-1.5 rounded-md px-2 py-1.5 text-text">
        <Volume2 size={18} className="shrink-0 text-online" />
        <span className="min-w-0 flex-1 truncate font-medium">{code && roomTitle(code, roomName)}</span>
        {joinedAt && <Elapsed since={joinedAt} />}
      </div>

      <div
        className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out ${open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"}`}
      >
        <ul aria-label="Membros da sala" className="flex min-h-0 flex-col gap-0.5 overflow-hidden pl-6">
          {me && <MemberRow member={me} isMe />}
          {others.map((m) => (
            <MemberRow
              key={m.id}
              member={m}
              watching={watching?.id === m.id}
              pending={pendingWatch === m.id}
              onWatch={() => void watch(m)}
            />
          ))}
        </ul>
      </div>
    </ScrollArea>
  );
}

/** Tempo desde que entramos na sala, como o "2:15:06" do canal de voz. */
function Elapsed(props: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const s = Math.max(0, Math.floor((now - props.since) / 1000));
  const text = `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  return (
    <span aria-label="Tempo na sala" className="font-mono text-xs text-online tabular-nums">
      {text}
    </span>
  );
}

function MemberRow(props: {
  member: Member;
  isMe?: boolean;
  watching?: boolean;
  pending?: boolean;
  onWatch?: () => void;
}) {
  const { member } = props;
  const profile = useAccountStore((s) => s.profile);
  const speaking = useVoiceStore((s) => !!s.speaking[props.isMe ? "me" : member.id]);
  const effect = useMemberEffect(member, !!props.isMe);
  const myPrefs = useVoiceStore((s) => s.prefs);
  const muted = props.isMe ? myPrefs.muted || myPrefs.deafened : !!member.muted;
  const deafened = props.isMe ? myPrefs.deafened : !!member.deafened;
  const canWatch = member.live && !props.isMe;
  return (
    <MemberMenu member={member} isMe={!!props.isMe} onWatch={props.onWatch}>
      <li
        className={`group flex items-center gap-2 rounded-md px-2 py-1 transition-colors ${
          props.watching ? "bg-surface-hover" : "hover:bg-surface-hover/70"
        }`}
      >
        <Avatar
          name={member.name}
          size={24}
          live={member.live}
          speaking={speaking}
          effect={effect}
          src={memberPhoto(member, !!props.isMe, profile, 64)}
        />
        <span
          className={`min-w-0 flex-1 truncate text-sm ${props.watching || member.live ? "text-text" : "text-text-muted group-hover:text-text"}`}
        >
          {member.name}
          {props.isMe && <span className="text-text-faint"> (você)</span>}
        </span>
        {(muted || deafened) && (
          <MicOff size={14} aria-label="Microfone silenciado" className="shrink-0 text-text-faint" />
        )}
        {deafened && <HeadphoneOff size={14} aria-label="Ensurdecido" className="shrink-0 text-text-faint" />}
        {member.live && (
          <span className="flex shrink-0 items-center gap-1 rounded bg-live px-1.5 py-0.5 text-[10px] font-bold text-white">
            Ao vivo
          </span>
        )}
        {member.live && <Viewers streamerId={member.id} compact />}
        {canWatch && !props.watching && (
          <Tooltip label={`Assistir ${member.name}`} side="top">
            <button
              type="button"
              onClick={props.onWatch}
              disabled={props.pending}
              aria-label={`Assistir ${member.name}`}
              className="shrink-0 rounded p-0.5 text-text-muted opacity-0 transition-opacity group-hover:opacity-100 hover:text-text focus-visible:opacity-100 disabled:opacity-100"
            >
              {props.pending ? <Loader2 size={16} className="animate-spin" /> : <Eye size={16} />}
            </button>
          </Tooltip>
        )}
      </li>
    </MemberMenu>
  );
}

/** Painel "Conectado à sala", como o "Voz conectada" do Discord. */
function ConnectedPanel(props: { code: string }) {
  const leave = useRoomStore((s) => s.leave);
  const roomName = useRoomStore((s) => s.roomName);
  const reconnecting = useRoomStore((s) => s.reconnecting);
  const live = useStreamStore((s) => s.status === "live");
  return (
    <div className="shrink-0 border-b border-white/[0.06] px-2 pt-2 pb-2">
      <div className="flex items-center gap-2 px-1">
        <Signal size={18} className="shrink-0 text-online" />
        <div className="min-w-0 flex-1">
          {reconnecting ? (
            <p className="text-sm font-semibold text-warning">Reconectando…</p>
          ) : (
            <p className="text-sm font-semibold text-online">{live ? "Transmitindo" : "Conectado à sala"}</p>
          )}
          <p className="truncate text-xs text-text-muted">{roomTitle(props.code, roomName)}</p>
        </div>
        <Tooltip label="Sair da sala" side="top">
          <button
            type="button"
            aria-label="Sair da sala"
            onClick={() => void leave()}
            className="rounded p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-live"
          >
            <PhoneOff size={18} />
          </button>
        </Tooltip>
      </div>
      <div className="mt-2 grid grid-cols-2 gap-2">
        {live ? (
          <PanelButton label="Você está ao vivo" icon={<Radio size={18} className="text-live" />} disabled />
        ) : (
          <StreamDialog trigger={<PanelButton label="Transmitir minha tela" icon={<MonitorUp size={18} />} />} />
        )}
        <PanelButton
          label="Convidar pessoas"
          icon={<UserPlus size={18} />}
          onClick={() => void copyInvite(props.code)}
        />
      </div>
    </div>
  );
}

function PanelButton(props: { label: string; icon: ReactNode; onClick?: () => void; disabled?: boolean }) {
  return (
    <Tooltip label={props.label} side="top">
      <button
        type="button"
        aria-label={props.label}
        onClick={props.onClick}
        disabled={props.disabled}
        className="grid h-8 place-items-center rounded-md bg-surface-1 text-text transition-colors hover:bg-surface-hover disabled:cursor-default disabled:hover:bg-surface-1"
      >
        {props.icon}
      </button>
    </Tooltip>
  );
}

/** Painel do usuário: avatar com status, nome, o que está fazendo, som e configurações. */
function UserPanel() {
  const me = useRoomStore((s) => s.members[0] ?? null);
  const profile = useAccountStore((s) => s.profile);
  const name = me?.name ?? (profile ? displayName(profile) : "Você");
  const code = useRoomStore((s) => s.code);
  const watching = useRoomStore((s) => s.watching);
  const live = useStreamStore((s) => s.status === "live");
  const speaking = useVoiceStore((s) => !!s.speaking.me);
  const effect = useSpeakingEffectStore((s) => s.effect);
  const status = live ? "Transmitindo" : watching ? `Assistindo ${watching.name}` : code ? "Na sala" : "Fora de sala";

  return (
    <footer className="flex h-[52px] shrink-0 items-center gap-1 px-2">
      <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-1 py-1 transition-colors hover:bg-surface-hover">
        <Avatar
          name={name}
          size={32}
          live={live}
          speaking={speaking}
          effect={effect}
          status={code ? "online" : "offline"}
          src={profile ? avatarUrl(profile) : undefined}
        />
        <div className="min-w-0">
          <p className="truncate text-sm leading-tight font-semibold">{name}</p>
          <p className="truncate text-xs leading-tight text-text-muted">{status}</p>
        </div>
      </div>
      <MicButton />
      <DeafenButton />
      <SettingsDialog
        trigger={
          <Tooltip label="Configurações" side="top">
            <button
              type="button"
              aria-label="Configurações"
              className="rounded p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
            >
              <Settings size={18} className="transition-transform duration-500 hover:rotate-90" />
            </button>
          </Tooltip>
        }
      />
    </footer>
  );
}
