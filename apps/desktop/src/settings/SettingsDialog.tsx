import { RotateCcw } from "lucide-react";
import { type CSSProperties, type ReactNode, useState } from "react";
import { DiscordLoginButton } from "../account/DiscordLoginButton";
import { avatarUrl, displayName, useAccountStore } from "../account/account-store";
import { Modal, ModalClose } from "../ui/Modal";
import { Switch } from "../ui/Switch";
import { usePresenceSettings } from "./presence-store";
import { Tooltip } from "../ui/Tooltip";
import { DEFAULT_GLASS, useAppearanceStore } from "./appearance-store";

/** Configurações do app. Por enquanto, a aparência (intensidade do vidro). */
export function SettingsDialog(props: { trigger: ReactNode }) {
  const [open, setOpen] = useState(false);
  const glass = useAppearanceStore((s) => s.glass);
  const setGlass = useAppearanceStore((s) => s.setGlass);

  return (
    <Modal
      open={open}
      onOpenChange={setOpen}
      trigger={props.trigger}
      title="Configurações"
      description="Muda na hora e fica salvo neste PC."
      footer={
        <>
          <span />
          <ModalClose className="rounded-md bg-brand px-4 py-2 text-sm font-medium transition-colors hover:bg-brand-hover">
            Pronto
          </ModalClose>
        </>
      }
    >
      <AccountSection />
      <PresenceSection />
      <section aria-labelledby="aparencia" className="mt-5 flex flex-col gap-3 text-left">
        <h3 id="aparencia" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
          Aparência
        </h3>
        <div className="rounded-md bg-surface-1 p-3">
          <div className="flex items-center justify-between gap-3">
            <label htmlFor="vidro" className="text-sm font-medium">
              Efeito de vidro
            </label>
            <span className="flex items-center gap-2">
              <span className="font-mono text-sm text-text-muted tabular-nums">{glass}%</span>
              {glass !== DEFAULT_GLASS && (
                <Tooltip label={`Voltar ao padrão (${DEFAULT_GLASS}%)`} side="top">
                  <button
                    type="button"
                    aria-label="Voltar ao padrão"
                    onClick={() => setGlass(DEFAULT_GLASS)}
                    className="rounded p-1 text-text-faint transition-colors hover:bg-surface-hover hover:text-text"
                  >
                    <RotateCcw size={14} />
                  </button>
                </Tooltip>
              )}
            </span>
          </div>
          <input
            id="vidro"
            type="range"
            min={0}
            max={100}
            step={5}
            value={glass}
            onChange={(e) => setGlass(Number(e.target.value))}
            style={{ "--fill": `${glass}%` } as CSSProperties}
            className="glass-slider mt-3 w-full"
          />
          <p className="mt-2 text-xs text-text-faint">
            Deixa a janela translúcida, com o que está atrás desfocado. Em 0% o app fica sólido. O desfoque é feito pelo
            Windows e precisa dos "Efeitos de transparência" ligados.
          </p>
        </div>
      </section>
    </Modal>
  );
}

/** Conta do Discord: quem está conectado, ou o botão para entrar. */
function AccountSection() {
  const profile = useAccountStore((s) => s.profile);
  const logout = useAccountStore((s) => s.logout);
  return (
    <section aria-labelledby="conta" className="flex flex-col gap-3 text-left">
      <h3 id="conta" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Conta
      </h3>
      {profile ? (
        <div className="flex items-center gap-3 rounded-md bg-surface-1 p-3">
          <img src={avatarUrl(profile)} alt="" className="size-10 rounded-full" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-semibold">{displayName(profile)}</p>
            <p className="truncate text-xs text-text-muted">@{profile.username} · Discord</p>
          </div>
          <button
            type="button"
            onClick={() => void logout()}
            className="rounded-md px-3 py-1.5 text-sm text-text-muted transition-colors hover:bg-surface-hover hover:text-live"
          >
            Sair
          </button>
        </div>
      ) : (
        <div className="flex flex-col gap-3 rounded-md bg-surface-1 p-3">
          <p className="text-sm text-text-muted">
            Entre com o Discord para usar seu nome e sua foto nas salas. O SquadView só lê seu perfil público: não vê
            mensagens, servidores nem amigos.
          </p>
          <DiscordLoginButton className="self-start" />
        </div>
      )}
    </section>
  );
}

/** Rich Presence: o que aparece no perfil do Discord enquanto está numa sala. */
function PresenceSection() {
  const { show, shareInvite, setShow, setShareInvite } = usePresenceSettings();
  return (
    <section aria-labelledby="presenca" className="mt-5 flex flex-col gap-3 text-left">
      <h3 id="presenca" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Status no Discord
      </h3>
      <div className="flex flex-col gap-4 rounded-md bg-surface-1 p-3">
        <Switch
          label="Mostrar a sala no meu perfil"
          hint="Com o Discord aberto no PC, aparece que você está numa sala, transmitindo ou assistindo."
          checked={show}
          onChange={setShow}
        />
        <Switch
          label='Botão "Entrar na sala" no perfil'
          hint="Qualquer pessoa que vir seu perfil pode entrar na sala pelo botão."
          checked={shareInvite}
          onChange={setShareInvite}
          disabled={!show}
        />
      </div>
    </section>
  );
}
