import { Crown, ImagePlus, Pencil, Shield, Trash2, UserMinus } from "lucide-react";
import { useRef, useState } from "react";
import { avatarUrl, useAccountStore } from "../account/account-store";
import { NameDialog } from "../room/NameDialog";
import { Avatar } from "../ui/Avatar";
import { Modal } from "../ui/Modal";
import { Tooltip } from "../ui/Tooltip";
import { IconCropDialog } from "./IconCropDialog";
import { ServerIcon } from "./ServerIcon";
import { myRole, type Server, type ServerMember, type ServerRoom, useServersStore } from "./servers-store";

type Tab = "geral" | "salas" | "membros";

const ROLE_LABEL: Record<ServerMember["role"], string> = {
  owner: "Dono",
  admin: "Administrador",
  member: "Membro",
};

/** Configurações do servidor (só dono e administradores abrem): nome, salas e membros. */
export function ServerSettingsDialog(props: { server: Server; open: boolean; onOpenChange: (open: boolean) => void }) {
  const [tab, setTab] = useState<Tab>("geral");
  return (
    <Modal
      open={props.open}
      onOpenChange={props.onOpenChange}
      width={560}
      title={`Configurações de ${props.server.name}`}
    >
      <div role="tablist" aria-label="Partes do servidor" className="mb-4 flex gap-1 border-b border-white/[0.06]">
        {(
          [
            ["geral", "Visão geral"],
            ["salas", "Salas"],
            ["membros", "Membros"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition-colors ${
              tab === id ? "border-brand text-text" : "border-transparent text-text-muted hover:text-text"
            }`}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="min-h-64 text-left">
        {tab === "geral" && <Overview server={props.server} />}
        {tab === "salas" && <Rooms server={props.server} />}
        {tab === "membros" && <Members server={props.server} />}
      </div>
    </Modal>
  );
}

/** Imagens que dá para usar como ícone (até 8 MB; o recorte reduz para 96 px). */
const ICON_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const MAX_ICON_FILE = 8 * 1024 * 1024;

function Overview(props: { server: Server }) {
  const act = useServersStore((s) => s.act);
  const [renaming, setRenaming] = useState(false);
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [error, setError] = useState<string | null>(null);
  const file = useRef<HTMLInputElement>(null);

  function pick(chosen: File | undefined) {
    setError(null);
    if (!chosen) return;
    if (!ICON_TYPES.includes(chosen.type)) return setError("Use uma imagem PNG, JPG, WebP ou GIF.");
    if (chosen.size > MAX_ICON_FILE) return setError("Essa imagem é grande demais (até 8 MB).");
    const url = URL.createObjectURL(chosen);
    const img = new Image();
    img.onload = () => setImage(img);
    img.onerror = () => {
      URL.revokeObjectURL(url);
      setError("Não deu para abrir essa imagem.");
    };
    img.src = url;
  }

  function closeCropper() {
    if (image) URL.revokeObjectURL(image.src);
    setImage(null);
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-4">
        <button
          type="button"
          aria-label="Trocar o ícone do servidor"
          onClick={() => file.current?.click()}
          className="group relative overflow-hidden rounded-2xl"
        >
          <ServerIcon name={props.server.name} icon={props.server.icon} size={80} />
          <span className="absolute inset-0 grid place-items-center bg-black/55 text-xs font-semibold opacity-0 transition-opacity group-hover:opacity-100">
            <ImagePlus size={20} />
          </span>
        </button>
        <div className="min-w-0 flex-1">
          <p className="text-xs font-semibold tracking-wide text-text-muted uppercase">Nome do servidor</p>
          <p className="truncate text-lg font-semibold">{props.server.name}</p>
          <p className="text-xs text-text-faint">
            {props.server.members.length} {props.server.members.length === 1 ? "membro" : "membros"} ·{" "}
            {props.server.rooms.length} {props.server.rooms.length === 1 ? "sala" : "salas"}
          </p>
        </div>
        <button
          type="button"
          onClick={() => setRenaming(true)}
          className="flex items-center gap-1.5 rounded-md bg-surface-3 px-3 py-2 text-sm font-medium hover:bg-surface-hover"
        >
          <Pencil size={14} /> Renomear
        </button>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => file.current?.click()}
          className="flex items-center gap-1.5 rounded-md bg-brand px-3 py-2 text-sm font-medium text-white hover:bg-brand-hover"
        >
          <ImagePlus size={14} /> {props.server.icon ? "Trocar ícone" : "Enviar ícone"}
        </button>
        {props.server.icon && (
          <button
            type="button"
            onClick={() => void act(props.server.id, { kind: "update", icon: null })}
            className="rounded-md px-3 py-2 text-sm text-text-muted hover:text-text hover:underline"
          >
            Remover ícone
          </button>
        )}
        <span className="text-xs text-text-faint">Quadrada fica melhor; você ajusta antes de salvar.</span>
      </div>
      {error && (
        <p role="alert" className="text-sm text-live">
          {error}
        </p>
      )}
      <input
        ref={file}
        type="file"
        accept={ICON_TYPES.join(",")}
        aria-label="Imagem do ícone"
        className="hidden"
        onChange={(e) => {
          pick(e.target.files?.[0]);
          // Escolher a mesma imagem de novo também abre o recorte.
          e.target.value = "";
        }}
      />
      <IconCropDialog
        image={image}
        onClose={closeCropper}
        onApply={(icon) => void act(props.server.id, { kind: "update", icon })}
      />
      <NameDialog
        open={renaming}
        onOpenChange={setRenaming}
        title="Nome do servidor"
        description="Todo mundo no servidor vê o nome novo na hora."
        label="Nome do servidor"
        initial={props.server.name}
        onSave={(name) => void act(props.server.id, { kind: "update", name })}
      />
    </div>
  );
}

function Rooms(props: { server: Server }) {
  const act = useServersStore((s) => s.act);
  const [renaming, setRenaming] = useState<ServerRoom | null>(null);
  return (
    <>
      <ul aria-label="Salas" className="flex flex-col divide-y divide-white/[0.06] rounded-lg bg-surface-1">
        {props.server.rooms.map((room) => (
          <li key={room.id} className="flex items-center gap-2 px-3 py-2">
            <span className="min-w-0 flex-1 truncate text-sm font-medium">{room.name}</span>
            <Tooltip label="Renomear" side="top">
              <button
                type="button"
                aria-label={`Renomear ${room.name}`}
                onClick={() => setRenaming(room)}
                className="rounded p-1.5 text-text-muted hover:bg-surface-hover hover:text-text"
              >
                <Pencil size={16} />
              </button>
            </Tooltip>
            <Tooltip label="Apagar sala" side="top">
              <button
                type="button"
                aria-label={`Apagar ${room.name}`}
                disabled={props.server.rooms.length === 1}
                onClick={() => void act(props.server.id, { kind: "room-delete", room: room.id })}
                className="rounded p-1.5 text-text-muted hover:bg-live/15 hover:text-live disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-text-muted"
              >
                <Trash2 size={16} />
              </button>
            </Tooltip>
          </li>
        ))}
      </ul>
      <p className="mt-2 text-xs text-text-faint">O servidor sempre tem ao menos uma sala.</p>
      <NameDialog
        open={renaming !== null}
        onOpenChange={(open) => !open && setRenaming(null)}
        title="Nome da sala"
        label="Nome da sala"
        initial={renaming?.name ?? ""}
        onSave={(name) => renaming && void act(props.server.id, { kind: "room-rename", room: renaming.id, name })}
      />
    </>
  );
}

function Members(props: { server: Server }) {
  const act = useServersStore((s) => s.act);
  const me = useAccountStore((s) => s.profile?.id);
  const role = myRole(props.server, me);
  return (
    <ul aria-label="Membros" className="flex flex-col divide-y divide-white/[0.06] rounded-lg bg-surface-1">
      {props.server.members.map((m) => {
        // Administrador tira membros; só o dono tira administradores e muda papéis.
        const canKick =
          m.id !== me && (m.role === "member" ? role !== "member" : m.role === "admin" && role === "owner");
        return (
          <li key={m.id} className="flex items-center gap-3 px-3 py-2">
            <Avatar name={m.name} size={32} src={m.avatar ? avatarUrl(m, 64) : undefined} />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">
                {m.name}
                {m.id === me && <span className="text-text-faint"> (você)</span>}
              </p>
              <p className="flex items-center gap-1 text-xs text-text-muted">
                {m.role === "owner" && <Crown size={12} className="text-[#f0b232]" />}
                {m.role === "admin" && <Shield size={12} className="text-brand" />}
                {ROLE_LABEL[m.role]}
              </p>
            </div>
            {role === "owner" && m.role !== "owner" && (
              <button
                type="button"
                onClick={() =>
                  void act(props.server.id, {
                    kind: "role",
                    member: m.id,
                    role: m.role === "admin" ? "member" : "admin",
                  })
                }
                className="rounded-md bg-surface-3 px-2.5 py-1 text-xs font-medium hover:bg-surface-hover"
              >
                {m.role === "admin" ? "Tirar administrador" : "Tornar administrador"}
              </button>
            )}
            {canKick && (
              <Tooltip label="Tirar do servidor" side="top">
                <button
                  type="button"
                  aria-label={`Tirar ${m.name} do servidor`}
                  onClick={() => void act(props.server.id, { kind: "kick", member: m.id })}
                  className="rounded p-1.5 text-text-muted hover:bg-live/15 hover:text-live"
                >
                  <UserMinus size={16} />
                </button>
              </Tooltip>
            )}
          </li>
        );
      })}
    </ul>
  );
}
