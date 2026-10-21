import * as Dialog from "@radix-ui/react-dialog";
import {
  Activity,
  Bell,
  Gamepad2,
  Loader2,
  LogOut,
  Monitor,
  Mic,
  Palette,
  Pencil,
  RotateCcw,
  Search,
  User,
  X,
} from "lucide-react";
import { type CSSProperties, type ReactNode, useEffect, useState } from "react";
import { DiscordLoginButton } from "../account/DiscordLoginButton";
import { avatarUrl, displayName, useAccountStore } from "../account/account-store";
import { useFriendsStore } from "../friends/friends-store";
import { Avatar } from "../ui/Avatar";
import { holdOverlay } from "../ui/overlay";
import { SoundsSection } from "../sounds/SoundsSection";
import { ScrollArea } from "../ui/ScrollArea";
import { Switch } from "../ui/Switch";
import { Tooltip } from "../ui/Tooltip";
import { DEFAULT_GLASS, useAppearanceStore } from "./appearance-store";
import { DiagnosticsSection } from "./DiagnosticsSection";
import { usePresenceSettings } from "./presence-store";
import { VoiceSection } from "./VoiceSection";
import { WindowsSection } from "./WindowsSection";

type Page = "conta" | "voz" | "sons" | "discord" | "aparencia" | "windows" | "desempenho";

interface Item {
  id: Page;
  label: string;
  icon: ReactNode;
  /** Palavras extras para a busca. */
  keywords: string;
  /** Partes da página, como os subitens de "Voz e vídeo" no Discord (id do título na página). */
  parts?: { id: string; label: string }[];
}

/** As seções da esquerda, agrupadas como no Discord. */
const GROUPS: { title: string; items: Item[] }[] = [
  {
    title: "Configurações de usuário",
    items: [{ id: "conta", label: "Minha conta", icon: <User size={18} />, keywords: "discord perfil login entrar" }],
  },
  {
    title: "Experiência",
    items: [
      {
        id: "voz",
        label: "Voz e vídeo",
        icon: <Mic size={18} />,
        keywords: "microfone saída fone volume ruído isolamento áudio",
        parts: [
          { id: "voz-dispositivos", label: "Voz" },
          { id: "voz-teste", label: "Teste do microfone" },
          { id: "voz-processamento", label: "Processamento de voz" },
        ],
      },
      {
        id: "sons",
        label: "Sons",
        icon: <Bell size={18} />,
        keywords: "aviso notificação mudo surdo entrar sair alerta",
      },
      { id: "aparencia", label: "Aparência", icon: <Palette size={18} />, keywords: "vidro transparência tema" },
      {
        id: "windows",
        label: "Windows",
        icon: <Monitor size={18} />,
        keywords: "bandeja fechar minimizar relógio segundo plano iniciar",
      },
      { id: "desempenho", label: "Desempenho", icon: <Activity size={18} />, keywords: "registro fps gpu diagnóstico" },
    ],
  },
  {
    title: "Jogos e apps",
    items: [
      {
        id: "discord",
        label: "Status no Discord",
        icon: <Gamepad2 size={18} />,
        keywords: "rich presence perfil convite atividade",
      },
    ],
  },
];

const TITLES: Record<Page, string> = {
  conta: "Minha conta",
  voz: "Voz e vídeo",
  sons: "Sons",
  discord: "Status no Discord",
  aparencia: "Aparência",
  windows: "Configurações do Windows",
  desempenho: "Desempenho",
};

/**
 * Configurações como as do Discord: janela grande, o perfil e as seções à esquerda e a
 * página escolhida à direita. Tudo muda na hora e fica salvo neste PC.
 */
export function SettingsDialog(props: { trigger: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [page, setPage] = useState<Page>("voz");
  const [part, setPart] = useState<string | null>(null);
  // O vídeo nativo fica por cima da interface: some enquanto as configurações estão abertas.
  useEffect(() => (open ? holdOverlay() : undefined), [open]);
  // Subitem escolhido: rola até aquela parte da página (depois que ela aparece).
  useEffect(() => {
    if (part) document.getElementById(part)?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  }, [part, page]);

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>{props.trigger}</Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70 data-[state=closed]:animate-overlay-out data-[state=open]:animate-overlay-in" />
        <Dialog.Content
          aria-describedby={undefined}
          // Sem contorno de foco no primeiro botão ao abrir com o mouse (o Tab continua funcionando).
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement | null)?.focus();
          }}
          className="fixed top-1/2 left-1/2 z-40 flex h-[min(860px,calc(100vh-48px))] w-[min(1180px,calc(100vw-48px))] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-card border border-white/[0.06] bg-glass-modal shadow-2xl backdrop-blur-xl focus:outline-none data-[state=closed]:animate-modal-out data-[state=open]:animate-modal-in"
        >
          <Dialog.Title className="sr-only">Configurações</Dialog.Title>
          <Sidebar
            page={page}
            part={part}
            onPage={(p, pt) => {
              setPage(p);
              setPart(pt ?? null);
            }}
          />
          <div className="relative flex min-w-0 flex-1 flex-col bg-surface-2">
            <header className="flex h-12 shrink-0 items-center border-b border-white/[0.06] px-6 font-semibold">
              {TITLES[page]}
            </header>
            <Dialog.Close
              aria-label="Fechar"
              className="absolute top-2 right-3 flex flex-col items-center rounded-full p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
            >
              <X size={22} />
            </Dialog.Close>
            <ScrollArea className="min-h-0 flex-1" viewportClassName="px-10 py-8">
              <div key={page} className="mx-auto max-w-[680px] animate-step-in text-left">
                {page === "conta" && <AccountSection />}
                {page === "voz" && <VoiceSection />}
                {page === "sons" && <SoundsSection />}
                {page === "discord" && <PresenceSection />}
                {page === "aparencia" && <AppearanceSection />}
                {page === "windows" && <WindowsSection />}
                {page === "desempenho" && <DiagnosticsSection />}
              </div>
            </ScrollArea>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Sidebar(props: { page: Page; part: string | null; onPage: (p: Page, part?: string) => void }) {
  const profile = useAccountStore((s) => s.profile);
  const logout = useAccountStore((s) => s.logout);
  const [query, setQuery] = useState("");
  const name = profile ? displayName(profile) : "Você";

  const q = query.trim().toLocaleLowerCase("pt-BR");
  const groups = GROUPS.map((g) => ({
    ...g,
    items: g.items.filter(
      (i) =>
        !q ||
        `${i.label} ${i.keywords} ${i.parts?.map((p) => p.label).join(" ") ?? ""}`
          .toLocaleLowerCase("pt-BR")
          .includes(q),
    ),
  })).filter((g) => g.items.length > 0);

  return (
    <nav aria-label="Categorias" className="flex w-72 shrink-0 flex-col overflow-y-auto bg-surface-1 px-3 py-5">
      <button
        type="button"
        onClick={() => props.onPage("conta")}
        className="mb-3 flex items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-surface-hover"
      >
        <Avatar name={name} size={44} src={profile ? avatarUrl(profile) : undefined} />
        <span className="min-w-0">
          <span className="block truncate font-semibold">{name}</span>
          <span className="flex items-center gap-1 truncate text-xs text-text-muted">
            {profile ? (
              <>
                Editar perfil <Pencil size={12} />
              </>
            ) : (
              "Entrar com o Discord"
            )}
          </span>
        </span>
      </button>

      <label className="mb-4 flex h-9 items-center gap-2 rounded-md border border-white/[0.06] bg-surface-0 px-2.5 text-text-muted focus-within:border-brand">
        <Search size={16} className="shrink-0" />
        <input
          type="search"
          aria-label="Buscar nas configurações"
          placeholder="Buscar"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="min-w-0 flex-1 bg-transparent text-sm text-text outline-none placeholder:text-text-muted"
        />
      </label>

      {groups.length === 0 && <p className="px-2 text-sm text-text-muted">Nada encontrado.</p>}
      {groups.map((group, gi) => (
        <div key={group.title} className={gi > 0 ? "mt-2 border-t border-white/[0.06] pt-3" : ""}>
          <p className="px-2 pb-1 text-xs font-medium text-text-muted">{group.title}</p>
          {group.items.map((item) => {
            const active = props.page === item.id;
            return (
              <div key={item.id} className="mb-0.5">
                <button
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => props.onPage(item.id)}
                  className={`flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left font-medium transition-colors ${
                    active ? "bg-surface-hover text-text" : "text-text-muted hover:bg-surface-hover/60 hover:text-text"
                  }`}
                >
                  {item.icon}
                  {item.label}
                </button>
                {active && item.parts && (
                  <div className="my-1 ml-[17px] flex flex-col border-l border-white/10">
                    {item.parts.map((pt, i) => {
                      const on = props.part ? props.part === pt.id : i === 0;
                      return (
                        <button
                          key={pt.id}
                          type="button"
                          onClick={() => props.onPage(item.id, pt.id)}
                          className={`-ml-px border-l-2 py-1 pl-4 text-left text-sm transition-colors ${
                            on ? "border-text text-text" : "border-transparent text-text-muted hover:text-text"
                          }`}
                        >
                          {pt.label}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ))}

      {profile && !q && (
        <div className="mt-2 border-t border-white/[0.06] pt-3">
          <button
            type="button"
            onClick={() => void logout()}
            className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left font-medium text-live transition-colors hover:bg-live/10"
          >
            <LogOut size={18} />
            Sair
          </button>
        </div>
      )}
      <p className="mt-auto px-2 pt-4 text-xs text-text-faint">SquadView {import.meta.env.APP_VERSION}</p>
    </nav>
  );
}

/** Aparência: a intensidade do vidro. */
function AppearanceSection() {
  const glass = useAppearanceStore((s) => s.glass);
  const setGlass = useAppearanceStore((s) => s.setGlass);
  return (
    <section aria-labelledby="aparencia" className="flex flex-col gap-3">
      <h3 id="aparencia" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Aparência
      </h3>
      <div>
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
  );
}

/**
 * Se os amigos estão ligados para esta conta. O login vale para o app inteiro: quando a sessão
 * ainda não saiu (o app segue tentando sozinho), aparece aqui também, e não só na página de amigos.
 */
function FriendsState() {
  const status = useFriendsStore((s) => s.status);
  if (status === "online") {
    return <p className="text-xs text-text-faint">Amigos ativos: você aparece nos recentes de quem está na sala.</p>;
  }
  if (status === "connecting") {
    return (
      <p className="flex items-center gap-2 text-xs text-text-faint">
        <Loader2 size={12} className="animate-spin" /> Ativando os amigos…
      </p>
    );
  }
  return (
    <div className="flex items-center gap-3 rounded-md bg-surface-1 p-3">
      <p className="flex-1 text-sm text-text-muted">
        Os amigos não estão ativos nesta conta. Entre de novo para o SquadView confirmar que é você.
      </p>
      <DiscordLoginButton />
    </div>
  );
}

/** Conta do Discord: quem está conectado, ou o botão para entrar. */
function AccountSection() {
  const profile = useAccountStore((s) => s.profile);
  const logout = useAccountStore((s) => s.logout);
  return (
    <section aria-labelledby="conta" className="flex flex-col gap-3">
      <h3 id="conta" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Conta do Discord
      </h3>
      {profile ? (
        <div className="flex items-center gap-4">
          <img src={avatarUrl(profile)} alt="" className="size-16 rounded-full" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-lg font-semibold">{displayName(profile)}</p>
            <p className="truncate text-sm text-text-muted">@{profile.username} · Discord</p>
          </div>
          <button
            type="button"
            onClick={() => void logout()}
            className="rounded-md bg-surface-3 px-4 py-2 text-sm font-medium transition-colors hover:bg-live hover:text-white"
          >
            Sair
          </button>
        </div>
      ) : null}
      {profile && <FriendsState />}
      {!profile && (
        <div className="flex flex-col gap-3">
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
    <section aria-labelledby="presenca" className="flex flex-col gap-3">
      <h3 id="presenca" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Perfil do Discord
      </h3>
      <div className="flex flex-col gap-4">
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
