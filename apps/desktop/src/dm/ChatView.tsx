import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  Check,
  Clock,
  CornerUpLeft,
  Download,
  FileText,
  FolderOpen,
  Loader2,
  Paperclip,
  Reply,
  SendHorizontal,
  SmilePlus,
  X,
} from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { avatarUrl, displayName, useAccountStore } from "../account/account-store";
import { useFriendsStore } from "../friends/friends-store";
import { Avatar } from "../ui/Avatar";
import { ScrollArea } from "../ui/ScrollArea";
import { inTauri } from "../ui/tauri-env";
import { Tooltip } from "../ui/Tooltip";
import { CopyDialog } from "./CopyDialog";
import { usePerson } from "./DmSidebar";
import { type DmMessage, fileSize, type Reaction, useDmStore } from "./dm-store";

/** Mensagens seguidas da mesma pessoa em até 5 min ficam juntas, sem repetir foto e nome. */
const GROUP_MS = 5 * 60 * 1000;

function time(at: number): string {
  return new Date(at).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
}

function day(at: number): string {
  return new Date(at).toLocaleDateString("pt-BR", { day: "2-digit", month: "long", year: "numeric" });
}

/**
 * Quando a mensagem foi mandada, como no Discord: só a hora se foi hoje, "Ontem às 23:59" se foi
 * ontem e "19/10/2026 14:45" se foi antes.
 */
export function stamp(at: number, now = Date.now()): string {
  const date = new Date(at);
  const today = new Date(now);
  const yesterday = new Date(now);
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === today.toDateString()) return time(at);
  if (date.toDateString() === yesterday.toDateString()) return `Ontem às ${time(at)}`;
  const numeric = date.toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit", year: "numeric" });
  return `${numeric} ${time(at)}`;
}

/** As três reações de um clique, na barra que aparece com o mouse em cima da mensagem. */
export const QUICK_REACTIONS = ["👍", "❤️", "😂"];
/** O resto, no botão de mais reações. */
export const MORE_REACTIONS = [
  "😀",
  "😁",
  "😂",
  "🤣",
  "😊",
  "😍",
  "🥰",
  "😎",
  "🤩",
  "🥲",
  "😅",
  "😬",
  "🤔",
  "🙃",
  "😏",
  "😮",
  "😴",
  "🤯",
  "😢",
  "😭",
  "😡",
  "💀",
  "👀",
  "🫡",
  "👍",
  "👎",
  "👏",
  "🙌",
  "🙏",
  "💪",
  "🤝",
  "✌️",
  "🔥",
  "❤️",
  "💔",
  "💯",
  "✅",
  "❌",
  "🎉",
  "🎮",
  "🏆",
  "⚡",
];

/**
 * Uma conversa privada, como a do Discord: mensagens agrupadas por pessoa, anexos com prévia e a
 * caixa de escrever embaixo. Tudo vai direto para o PC do amigo (ADR 0017).
 */
export function ChatView(props: { friend: string }) {
  const { friend } = props;
  const person = usePerson(friend);
  const presence = useFriendsStore((s) => s.friends.find((f) => f.id === friend)?.presence);
  const messages = useDmStore((s) => s.messages[friend]);
  const direct = useDmStore((s) => s.online[friend] ?? false);
  const profile = useAccountStore((s) => s.profile);
  const myName = profile ? displayName(profile) : "Você";
  const bottom = useRef<HTMLDivElement>(null);
  // A mensagem que acabou de ser levada à vista (clique numa resposta): pisca por um instante.
  const [flash, setFlash] = useState<string | null>(null);
  useEffect(() => {
    if (!flash) return;
    const t = setTimeout(() => setFlash(null), 1600);
    return () => clearTimeout(t);
  }, [flash]);
  const goTo = (id: string) => {
    document.getElementById(`mensagem-${id}`)?.scrollIntoView?.({ block: "center", behavior: "smooth" });
    setFlash(id);
  };
  // Mensagem nova (ou a prévia de uma foto que acabou de carregar e cresceu a conversa): rola até o fim.
  // Sem devolver nada: no Chromium novo o `scrollIntoView` devolve uma Promise, e o React a
  // chamaria como limpeza do efeito ao fechar a conversa.
  const toEnd = () => {
    bottom.current?.scrollIntoView?.({ block: "end" });
  };
  useEffect(toEnd, [messages?.length]);

  const status = direct
    ? "Conectado direto"
    : presence?.online
      ? "Online"
      : "Offline: o que você mandar fica esperando aqui até ele ficar online";

  return (
    <section aria-label={`Conversa com ${person.name}`} className="flex h-full w-full flex-col text-left">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-white/[0.06] px-4">
        <Avatar
          name={person.name}
          size={24}
          src={person.avatar ? avatarUrl(person, 64) : undefined}
          status={presence?.online ? "online" : "offline"}
          statusBorder="border-surface-2"
        />
        <span className="font-semibold">{person.name}</span>
        <span className="truncate text-xs text-text-muted">{status}</span>
      </header>

      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-4 pt-4">
        {messages === undefined ? (
          <p className="flex items-center gap-2 text-sm text-text-muted">
            <Loader2 size={14} className="animate-spin" /> Abrindo a conversa…
          </p>
        ) : (
          <>
            <div className="mb-6 flex flex-col gap-2">
              <Avatar name={person.name} size={72} src={person.avatar ? avatarUrl(person, 128) : undefined} />
              <h2 className="text-2xl font-bold">{person.name}</h2>
              <p className="text-sm text-text-muted">
                Esta conversa vai direto entre o seu PC e o de {person.name}: nada passa pelo servidor, e o histórico
                fica só aqui.
              </p>
            </div>
            <ol aria-label="Mensagens" className="flex flex-col pb-4">
              {messages.map((m, i) => {
                const prev = messages[i - 1];
                const newDay = !prev || day(prev.at) !== day(m.at);
                // Uma resposta sempre mostra quem mandou (como no Discord), mesmo logo depois de outra.
                const grouped = !newDay && !m.replyTo && prev.mine === m.mine && m.at - prev.at < GROUP_MS;
                const original = m.replyTo ? messages.find((o) => o.id === m.replyTo) : undefined;
                return (
                  <li key={m.id}>
                    {newDay && (
                      <div className="my-3 flex items-center gap-3 text-xs font-semibold text-text-faint">
                        <span className="h-px flex-1 bg-white/[0.06]" />
                        {day(m.at)}
                        <span className="h-px flex-1 bg-white/[0.06]" />
                      </div>
                    )}
                    <MessageRow
                      friend={friend}
                      message={m}
                      grouped={grouped}
                      author={m.mine ? myName : person.name}
                      photo={
                        m.mine
                          ? profile
                            ? avatarUrl(profile, 64)
                            : undefined
                          : person.avatar
                            ? avatarUrl(person, 64)
                            : undefined
                      }
                      friendName={person.name}
                      friendOnline={!!presence?.online}
                      onMediaLoad={toEnd}
                      original={original}
                      originalAuthor={original ? (original.mine ? myName : person.name) : undefined}
                      onGoTo={goTo}
                      flash={flash === m.id}
                    />
                  </li>
                );
              })}
            </ol>
            <div ref={bottom} />
          </>
        )}
      </ScrollArea>

      <Composer friend={friend} name={person.name} />
      <CopyDialog />
    </section>
  );
}

function MessageRow(props: {
  friend: string;
  message: DmMessage;
  grouped: boolean;
  author: string;
  photo?: string;
  friendName: string;
  /** O amigo está online (a mensagem pendente está conectando, não esperando ele aparecer). */
  friendOnline: boolean;
  onMediaLoad: () => void;
  /** A mensagem que esta responde (se ainda está na conversa) e quem a mandou. */
  original?: DmMessage;
  originalAuthor?: string;
  /** Leva até uma mensagem (clique na resposta). */
  onGoTo: (id: string) => void;
  /** Acabou de ser levada à vista: pisca. */
  flash: boolean;
}) {
  const m = props.message;
  const [menuOpen, setMenuOpen] = useState(false);
  return (
    <div
      id={`mensagem-${m.id}`}
      className={`group relative rounded px-1 transition-colors hover:bg-surface-hover/30 ${
        props.grouped ? "py-0.5" : "mt-3 py-0.5"
      } ${props.flash ? "bg-brand/15" : ""} ${menuOpen ? "bg-surface-hover/30" : ""}`}
    >
      {m.replyTo && <ReplyLine original={props.original} author={props.originalAuthor} onGoTo={props.onGoTo} />}
      <div className="flex gap-3">
        <span className="w-10 shrink-0">
          {props.grouped ? (
            // Agrupada: a hora aparece no lugar da foto, com o mouse em cima (como no Discord).
            <span className="block pt-1 text-right text-[10px] text-text-faint opacity-0 group-hover:opacity-100">
              {time(m.at)}
            </span>
          ) : (
            <Avatar name={props.author} size={40} src={props.photo} />
          )}
        </span>
        <div className="min-w-0 flex-1">
          {!props.grouped && (
            <p className="flex items-baseline gap-2">
              <span className="font-semibold">{props.author}</span>
              <span className="text-xs text-text-faint">{stamp(m.at)}</span>
            </p>
          )}
          {m.text && <p className="break-words whitespace-pre-wrap text-text">{m.text}</p>}
          {m.file && <AttachmentView friend={props.friend} message={m} onMediaLoad={props.onMediaLoad} />}
          {!!m.reactions?.length && <Reactions friend={props.friend} message={m} friendName={props.friendName} />}
          {m.mine && (
            <p className="mt-0.5 flex items-center gap-1 text-[11px] text-text-faint">
              {m.status === "pending" ? (
                <>
                  <Clock size={11} />{" "}
                  {props.friendOnline
                    ? `Conectando com ${props.friendName}…`
                    : `Esperando ${props.friendName} ficar online`}
                </>
              ) : (
                <span className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                  <Check size={11} /> Entregue
                </span>
              )}
            </p>
          )}
        </div>
      </div>
      <MessageActions friend={props.friend} message={m} author={props.author} onMenuChange={setMenuOpen} />
    </div>
  );
}

/** A linha em cima de uma resposta: quem e o que foi respondido. Clique leva até a original. */
function ReplyLine(props: { original?: DmMessage; author?: string; onGoTo: (id: string) => void }) {
  const o = props.original;
  if (!o) {
    return (
      <p className="mb-0.5 flex items-center gap-1.5 pl-[52px] text-xs text-text-faint italic">
        <CornerUpLeft size={12} className="shrink-0" /> A mensagem original não está mais aqui
      </p>
    );
  }
  const preview = o.text ?? (o.file ? `📎 ${o.file.name}` : "");
  return (
    <button
      type="button"
      onClick={() => props.onGoTo(o.id)}
      aria-label={`Ir para a mensagem de ${props.author}`}
      className="mb-0.5 flex max-w-full items-center gap-1.5 pl-[52px] text-left text-xs text-text-muted hover:text-text"
    >
      <CornerUpLeft size={12} className="shrink-0" />
      <span className="shrink-0 font-semibold">@{props.author}</span>
      <span className="truncate">{preview}</span>
    </button>
  );
}

/** Quem reagiu, em palavras: "Você e Ana reagiram com 🔥", "Ana reagiu com 🔥". */
function reactedBy(r: Reaction, friendName: string): string {
  if (r.mine && r.theirs) return `Você e ${friendName} reagiram com ${r.emoji}`;
  return `${r.mine ? "Você" : friendName} reagiu com ${r.emoji}`;
}

/** As reações embaixo da mensagem: clique na sua tira; na do amigo, reage junto. */
function Reactions(props: { friend: string; message: DmMessage; friendName: string }) {
  const react = useDmStore((s) => s.react);
  return (
    <div className="mt-1 flex flex-wrap gap-1">
      {props.message.reactions!.map((r) => {
        const count = Number(r.mine) + Number(r.theirs);
        return (
          <Tooltip key={r.emoji} label={reactedBy(r, props.friendName)} side="top">
            <button
              type="button"
              aria-pressed={r.mine}
              aria-label={`${r.emoji} ${count}`}
              onClick={() => void react(props.friend, props.message, r.emoji)}
              className={`flex h-6 items-center gap-1 rounded-lg border px-1.5 text-sm transition-colors ${
                r.mine
                  ? "border-brand bg-brand/15 text-text"
                  : "border-transparent bg-surface-1 text-text-muted hover:border-white/10"
              }`}
            >
              <span>{r.emoji}</span>
              <span className="text-xs font-semibold tabular-nums">{count}</span>
            </button>
          </Tooltip>
        );
      })}
    </div>
  );
}

const ACTION =
  "grid size-8 place-items-center rounded text-lg text-text-muted transition-colors hover:bg-surface-hover hover:text-text";

/**
 * A barra que aparece com o mouse em cima da mensagem, como a do Discord: três reações de um
 * clique, mais reações e responder.
 */
function MessageActions(props: {
  friend: string;
  message: DmMessage;
  author: string;
  onMenuChange: (open: boolean) => void;
}) {
  const react = useDmStore((s) => s.react);
  const setReplying = useDmStore((s) => s.setReplying);
  const [open, setOpen] = useState(false);
  const change = (next: boolean) => {
    setOpen(next);
    props.onMenuChange(next);
  };
  return (
    <div
      role="toolbar"
      aria-label={`Ações da mensagem de ${props.author}`}
      className={`absolute -top-4 right-4 z-10 items-center gap-0.5 rounded-lg border border-white/[0.06] bg-surface-2 p-0.5 shadow-lg ${
        open ? "flex" : "hidden group-focus-within:flex group-hover:flex"
      }`}
    >
      {QUICK_REACTIONS.map((emoji) => (
        <Tooltip key={emoji} label={`Reagir com ${emoji}`} side="top">
          <button
            type="button"
            aria-label={`Reagir com ${emoji}`}
            onClick={() => void react(props.friend, props.message, emoji)}
            className={ACTION}
          >
            {emoji}
          </button>
        </Tooltip>
      ))}
      <DropdownMenu.Root open={open} onOpenChange={change}>
        <Tooltip label="Adicionar reação" side="top">
          <DropdownMenu.Trigger asChild>
            <button type="button" aria-label="Adicionar reação" className={ACTION}>
              <SmilePlus size={18} />
            </button>
          </DropdownMenu.Trigger>
        </Tooltip>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            side="top"
            align="end"
            sideOffset={6}
            aria-label="Reações"
            className="z-50 grid w-[296px] grid-cols-8 gap-0.5 rounded-lg bg-surface-float p-2 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            {MORE_REACTIONS.map((emoji) => (
              <DropdownMenu.Item
                key={emoji}
                aria-label={`Reagir com ${emoji}`}
                onSelect={() => void react(props.friend, props.message, emoji)}
                className="grid size-8 cursor-pointer place-items-center rounded text-xl outline-none data-[highlighted]:bg-surface-hover"
              >
                {emoji}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <Tooltip label="Responder" side="top">
        <button
          type="button"
          aria-label="Responder"
          onClick={() => setReplying(props.friend, props.message.id)}
          className={ACTION}
        >
          <Reply size={18} />
        </button>
      </Tooltip>
    </div>
  );
}

/** Um anexo: foto e vídeo com prévia; o resto como cartão, com baixar, abrir e mostrar na pasta. */
function AttachmentView(props: { friend: string; message: DmMessage; onMediaLoad: () => void }) {
  const m = props.message;
  const file = m.file!;
  const downloading = useDmStore((s) => s.downloading[m.id] ?? false);
  const download = useDmStore((s) => s.download);
  const cancel = useDmStore((s) => s.cancelDownload);
  const [src, setSrc] = useState<string | null>(null);
  const media = file.mime.startsWith("image/") || file.mime.startsWith("video/");

  // Foto ou vídeo neste PC: libera só este arquivo para a prévia.
  useEffect(() => {
    if (!file.path || !media) return;
    let alive = true;
    invoke<string>("dm_preview", { friend: props.friend, id: m.id }).then(
      (path) => alive && setSrc(convertFileSrc(path)),
      () => alive && setSrc(null),
    );
    return () => {
      alive = false;
    };
  }, [file.path, media, props.friend, m.id]);

  const progress = file.size ? Math.min(100, Math.round((file.done / file.size) * 100)) : 0;
  const inProgress = downloading && !file.path;

  async function saveAs() {
    const dest = await invoke<string | null>("dm_pick_save", { name: file.name }).catch(() => null);
    if (dest) await download(props.friend, m, dest);
  }

  if (src && file.mime.startsWith("image/")) {
    return (
      <button
        type="button"
        aria-label={`Abrir ${file.name}`}
        onClick={() => void invoke("dm_open", { friend: props.friend, id: m.id })}
        className="mt-1 block max-w-sm overflow-hidden rounded-lg"
      >
        <img src={src} alt={file.name} onLoad={props.onMediaLoad} className="max-h-80 w-auto" />
      </button>
    );
  }
  if (src && file.mime.startsWith("video/")) {
    return (
      <video
        src={src}
        controls
        aria-label={file.name}
        onLoadedMetadata={props.onMediaLoad}
        className="mt-1 max-h-80 max-w-md rounded-lg"
      />
    );
  }

  return (
    <div className="mt-1 flex max-w-sm items-center gap-3 rounded-lg border border-white/[0.06] bg-surface-1 p-3">
      <FileText size={28} className="shrink-0 text-text-muted" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">{file.name}</p>
        <p className="text-xs text-text-faint">
          {fileSize(file.size)}
          {inProgress && ` · ${progress}%`}
          {file.gone && !file.path && " · Não está mais disponível"}
        </p>
        {inProgress && (
          <div
            role="progressbar"
            aria-label={`Baixando ${file.name}`}
            aria-valuenow={progress}
            className="mt-1.5 h-1 overflow-hidden rounded-full bg-surface-3"
          >
            <div className="h-full bg-brand transition-[width]" style={{ width: `${progress}%` }} />
          </div>
        )}
      </div>
      {file.path ? (
        <>
          <Tooltip label="Abrir" side="top">
            <button
              type="button"
              aria-label={`Abrir ${file.name}`}
              onClick={() => void invoke("dm_open", { friend: props.friend, id: m.id })}
              className="rounded p-1.5 text-text-muted hover:bg-surface-hover hover:text-text"
            >
              <FileText size={18} />
            </button>
          </Tooltip>
          <Tooltip label="Mostrar na pasta" side="top">
            <button
              type="button"
              aria-label={`Mostrar ${file.name} na pasta`}
              onClick={() => void invoke("dm_reveal", { friend: props.friend, id: m.id })}
              className="rounded p-1.5 text-text-muted hover:bg-surface-hover hover:text-text"
            >
              <FolderOpen size={18} />
            </button>
          </Tooltip>
        </>
      ) : inProgress ? (
        <Tooltip label="Cancelar download" side="top">
          <button
            type="button"
            aria-label={`Cancelar o download de ${file.name}`}
            onClick={() => void cancel(props.friend, m)}
            className="rounded p-1.5 text-text-muted hover:bg-surface-hover hover:text-live"
          >
            <X size={18} />
          </button>
        </Tooltip>
      ) : (
        !m.mine &&
        !file.gone && (
          <button
            type="button"
            onClick={() => void saveAs()}
            className="flex items-center gap-1.5 rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white hover:bg-brand-hover"
          >
            <Download size={14} /> Baixar
          </button>
        )
      )}
    </div>
  );
}

/** A caixa de escrever: Enter manda, Shift+Enter quebra a linha; o clipe (ou arrastar) anexa. */
function Composer(props: { friend: string; name: string }) {
  const [text, setText] = useState("");
  const [dragging, setDragging] = useState(false);
  const sendText = useDmStore((s) => s.sendText);
  const sendFiles = useDmStore((s) => s.sendFiles);
  const replyingId = useDmStore((s) => s.replying[props.friend]);
  const replying = useDmStore((s) =>
    replyingId ? s.messages[props.friend]?.find((m) => m.id === replyingId) : undefined,
  );
  const setReplying = useDmStore((s) => s.setReplying);
  const myName = useAccountStore((s) => (s.profile ? displayName(s.profile) : "Você"));
  const input = useRef<HTMLTextAreaElement>(null);
  // Clicou em Responder: a caixa de escrever já fica pronta.
  useEffect(() => {
    if (replyingId) input.current?.focus();
  }, [replyingId]);

  // Arrastar arquivos do Windows para a janela manda para esta conversa.
  useEffect(() => {
    if (!inTauri()) return;
    let off: (() => void) | undefined;
    let alive = true;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
        else setDragging(false);
        if (e.payload.type === "drop") void sendFiles(props.friend, e.payload.paths);
      })
      .then((unlisten) => (alive ? (off = unlisten) : unlisten()));
    return () => {
      alive = false;
      off?.();
    };
  }, [props.friend, sendFiles]);

  async function submit() {
    if (!text.trim()) return;
    if (await sendText(props.friend, text, replying?.id)) {
      setText("");
      setReplying(props.friend, null);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape" && replying) {
      e.preventDefault();
      setReplying(props.friend, null);
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  }

  async function attach() {
    const paths = await invoke<string[]>("dm_pick_files").catch(() => []);
    await sendFiles(props.friend, paths);
  }

  return (
    <div className="shrink-0 px-4 pb-4">
      {replying && (
        <div className="flex items-center gap-2 rounded-t-lg bg-surface-3/60 px-3 py-1.5 text-sm text-text-muted">
          <span className="min-w-0 flex-1 truncate">
            Respondendo a <b className="text-text">{replying.mine ? myName : props.name}</b>
          </span>
          <Tooltip label="Cancelar resposta" side="top">
            <button
              type="button"
              aria-label="Cancelar resposta"
              onClick={() => setReplying(props.friend, null)}
              className="rounded p-0.5 text-text-muted hover:text-text"
            >
              <X size={16} />
            </button>
          </Tooltip>
        </div>
      )}
      <div
        className={`flex items-end gap-2 bg-surface-1 px-3 py-2 transition-shadow ${
          replying ? "rounded-b-lg" : "rounded-lg"
        } ${dragging ? "ring-2 ring-brand" : ""}`}
      >
        <Tooltip label="Anexar arquivos" side="top">
          <button
            type="button"
            aria-label="Anexar arquivos"
            onClick={() => void attach()}
            className="rounded p-1.5 text-text-muted transition-colors hover:text-text"
          >
            <Paperclip size={20} />
          </button>
        </Tooltip>
        <textarea
          ref={input}
          aria-label={`Mensagem para ${props.name}`}
          placeholder={dragging ? "Solte para mandar" : `Conversar com ${props.name}`}
          value={text}
          rows={1}
          maxLength={4000}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          className="max-h-40 min-h-9 flex-1 resize-none bg-transparent py-1.5 text-text outline-none placeholder:text-text-faint"
        />
        <Tooltip label="Enviar" side="top">
          <button
            type="button"
            aria-label="Enviar"
            disabled={!text.trim()}
            onClick={() => void submit()}
            className="rounded p-1.5 text-text-muted transition-colors hover:text-text disabled:opacity-40"
          >
            <SendHorizontal size={20} />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
