import * as ContextMenu from "@radix-ui/react-context-menu";
import { Eye, Pencil, UserPlus } from "lucide-react";
import { type CSSProperties, type ReactNode, useState } from "react";
import { useFriendsStore } from "../friends/friends-store";
import { useOverlayMenu } from "../ui/overlay";
import { MAX_VOICE_VOLUME, useVoiceStore } from "../voice/voice-store";

const DEFAULT_PEER = { volume: 100, muted: false };
import { NameDialog } from "./NameDialog";
import { type Member, useRoomStore } from "./room-store";
import { CheckMark } from "../ui/Checkbox";

/**
 * Menu do botão direito num membro, como o do Discord. No nosso nome: trocar o apelido. Nos
 * outros: assistir (se estiver ao vivo), o volume dele e silenciar, os dois só para nós.
 */
export function MemberMenu(props: { member: Member; isMe: boolean; onWatch?: () => void; children: ReactNode }) {
  const setNickname = useRoomStore((s) => s.setNickname);
  const [renaming, setRenaming] = useState(false);
  const { member } = props;
  // O vídeo nativo fica por cima da interface: some enquanto o menu está aberto.
  const onOpenChange = useOverlayMenu();

  return (
    <>
      <ContextMenu.Root onOpenChange={onOpenChange}>
        <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content
            aria-label={`Opções de ${member.name}`}
            className="z-50 w-60 rounded-lg bg-surface-float p-1.5 shadow-2xl shadow-black/50 data-[state=closed]:animate-overlay-out data-[state=open]:animate-menu-in"
          >
            <ContextMenu.Label className="truncate px-2 py-1.5 text-xs font-semibold tracking-wide text-text-faint uppercase">
              {member.name}
            </ContextMenu.Label>
            {props.isMe ? (
              <Item icon={<Pencil size={16} />} onSelect={() => setRenaming(true)}>
                Alterar apelido
              </Item>
            ) : (
              <>
                {member.live && props.onWatch && (
                  <Item icon={<Eye size={16} />} onSelect={props.onWatch}>
                    Assistir a transmissão
                  </Item>
                )}
                <AddFriend member={member} />
                <PeerVoice member={member} />
              </>
            )}
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      {props.isMe && (
        <NameDialog
          open={renaming}
          onOpenChange={setRenaming}
          title="Alterar apelido"
          description="Como a galera vê você nesta sala."
          label="Apelido"
          initial={member.name}
          onSave={(name) => void setNickname(name)}
        />
      )}
    </>
  );
}

/** "Adicionar amigo", para quem entrou com o Discord e ainda não é amigo nem tem pedido. */
function AddFriend(props: { member: Member }) {
  const id = props.member.discord?.id;
  const status = useFriendsStore((s) => s.status);
  const known = useFriendsStore(
    (s) => !id || s.friends.some((f) => f.id === id) || s.outgoing.some((p) => p.id === id),
  );
  const act = useFriendsStore((s) => s.act);
  if (!id || known) return null;
  // Explica por que não dá, em vez de um erro genérico depois do clique.
  const blocked =
    status !== "online"
      ? "Entre de novo com o Discord (Configurações › Minha conta) para usar os amigos."
      : props.member.verified === false
        ? `${props.member.name} precisa entrar de novo com o Discord no SquadView.`
        : null;
  if (blocked) {
    return (
      <ContextMenu.Item
        disabled
        className="flex flex-col items-start gap-0.5 rounded px-2 py-1.5 text-sm text-text-faint outline-none"
      >
        <span className="flex w-full items-center justify-between font-medium">
          Adicionar amigo <UserPlus size={16} />
        </span>
        <span className="text-xs">{blocked}</span>
      </ContextMenu.Item>
    );
  }
  return (
    <Item icon={<UserPlus size={16} />} onSelect={() => void act({ type: "friend-request", id })}>
      Adicionar amigo
    </Item>
  );
}

/** "Volume do usuário" e "Silenciar" de um amigo, só para nós (ficam salvos pelo nome). */
function PeerVoice(props: { member: Member }) {
  const { member } = props;
  // O seletor devolve só o que está salvo; o padrão fica fora (um objeto novo a cada render
  // faria o componente se redesenhar sem parar).
  const saved = useVoiceStore((s) => s.peers[member.name]);
  const prefs = saved ?? DEFAULT_PEER;
  const setPeerVolume = useVoiceStore((s) => s.setPeerVolume);
  const setPeerMuted = useVoiceStore((s) => s.setPeerMuted);
  return (
    <>
      <ContextMenu.Separator className="my-1 h-px bg-surface-3" />
      {/* As setas mexem no volume, não no menu. */}
      <div className="px-2 py-2" onKeyDown={(e) => e.stopPropagation()}>
        <label className="block text-sm font-medium">
          Volume do usuário
          <input
            type="range"
            aria-label={`Volume de ${member.name}`}
            min={0}
            max={MAX_VOICE_VOLUME}
            step={5}
            value={prefs.volume}
            onChange={(e) => void setPeerVolume(member.id, member.name, Number(e.target.value))}
            style={{ "--fill": `${(prefs.volume / MAX_VOICE_VOLUME) * 100}%` } as CSSProperties}
            className="glass-slider mt-2 block w-full"
          />
        </label>
        <p className="mt-1 text-right text-xs text-text-faint tabular-nums">{prefs.volume}%</p>
      </div>
      <ContextMenu.Separator className="my-1 h-px bg-surface-3" />
      <ContextMenu.CheckboxItem
        checked={prefs.muted}
        onCheckedChange={(muted) => void setPeerMuted(member.id, member.name, muted)}
        onSelect={(e) => e.preventDefault()}
        className="flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white"
      >
        Silenciar
        <CheckMark checked={prefs.muted} />
      </ContextMenu.CheckboxItem>
    </>
  );
}

function Item(props: { icon: ReactNode; onSelect: () => void; children: ReactNode }) {
  return (
    <ContextMenu.Item
      onSelect={props.onSelect}
      className="flex cursor-pointer items-center justify-between rounded px-2 py-1.5 text-sm font-medium text-text-muted outline-none data-[highlighted]:bg-brand data-[highlighted]:text-white"
    >
      {props.children}
      {props.icon}
    </ContextMenu.Item>
  );
}
