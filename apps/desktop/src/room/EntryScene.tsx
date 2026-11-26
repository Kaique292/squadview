import { Check, Loader2 } from "lucide-react";
import { useEffect } from "react";
import { useAccountStore } from "../account/account-store";
import { Avatar } from "../ui/Avatar";
import { useVoiceStore } from "../voice/voice-store";
import { memberPhoto, roomTitle, useRoomStore } from "./room-store";

/** Quanto esperar as conexões diretas antes de dar a entrada por feita (alguém pode demorar). */
export const ENTRY_TIMEOUT_MS = 6000;
/** Quanto a cena fica no "pronto" antes de sumir sozinha. */
export const ENTRY_READY_MS = 1400;
/** Quantos amigos aparecem na cena (o resto vira "+N"). */
const MAX_FACES = 6;

/**
 * A cena de entrada na sala (ADR 0034): os amigos aparecem e a barra mostra as etapas de verdade,
 * da sala encontrada até a conexão direta com cada um (`voice-peers`). Fica por cima da sala sem
 * prender ninguém: some sozinha logo depois de conectar, e um clique ou o Esc a fecham na hora.
 */
export function EntryScene() {
  const entry = useRoomStore((s) => s.entry);
  const code = useRoomStore((s) => s.code);
  const roomName = useRoomStore((s) => s.roomName);
  const members = useRoomStore((s) => s.members);
  const setEntry = useRoomStore((s) => s.setEntry);
  const connected = useVoiceStore((s) => s.connected);
  const profile = useAccountStore((s) => s.profile);

  // Nós vimos primeiro na lista: os outros são com quem a conexão precisa fechar.
  const others = members.slice(1);
  const linked = others.filter((m) => connected.includes(m.id)).length;
  const phase = entry?.phase;
  const everyone = members.length > 0 && linked >= others.length;

  // Conectou com todos (ou está sozinho): pronto.
  useEffect(() => {
    if (phase === "connecting" && everyone) setEntry("ready");
  }, [phase, everyone, setEntry]);

  // Alguém demorando não prende a cena: no tempo limite, fica pronto com quem deu.
  useEffect(() => {
    if (phase !== "connecting" || !entry) return;
    const left = Math.max(0, ENTRY_TIMEOUT_MS - (Date.now() - entry.since));
    const timer = setTimeout(() => setEntry("ready"), left);
    return () => clearTimeout(timer);
  }, [phase, entry, setEntry]);

  // Pronto: some sozinha.
  useEffect(() => {
    if (phase !== "ready") return;
    const timer = setTimeout(() => setEntry(null), ENTRY_READY_MS);
    return () => clearTimeout(timer);
  }, [phase, setEntry]);

  // Esc fecha na hora.
  useEffect(() => {
    if (!phase || phase === "finding") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setEntry(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase, setEntry]);

  // Achando a sala, o modal de entrar mostra o andamento; a cena começa com a sala encontrada.
  if (!entry || phase === "finding" || !code) return null;

  const ready = phase === "ready";
  // Gradiente de objetivo: a barra já começa com a sala encontrada (1/3) e vai até o fim.
  // Sem a lista de membros ainda, não dá para saber com quantos conectar: fica no terço da sala.
  const progress = ready
    ? 1
    : members.length === 0
      ? 1 / 3
      : 1 / 3 + (others.length > 0 ? (linked / others.length) * (1 / 3) : 1 / 3);
  const faces = others.slice(0, MAX_FACES);
  const extra = others.length - faces.length;
  const alone = members.length > 0 && others.length === 0;
  const linkStep = alone
    ? "Ninguém mais na sala ainda"
    : members.length === 0
      ? "Conectando com a galera…"
      : ready
        ? linked === others.length
          ? `Conexão direta com ${others.length === 1 ? "1 pessoa" : `as ${others.length} pessoas`}`
          : `Conectado com ${linked} de ${others.length} (o resto continua tentando)`
        : `Conectando direto com cada um… ${linked} de ${others.length}`;
  const steps = [
    { text: "Sala encontrada", done: true },
    { text: linkStep, done: ready || alone },
    { text: ready ? "Pronto" : "Preparando a sala", done: ready },
  ];

  return (
    <div
      data-entry-scene
      onClick={() => setEntry(null)}
      className="absolute inset-0 z-30 grid animate-entry-fade place-items-center bg-black/55 p-4"
    >
      <div
        role="status"
        aria-live="polite"
        aria-label="Entrando na sala"
        className="glass-live flex w-[min(440px,100%)] animate-entry-rise flex-col items-center gap-6 rounded-3xl px-8 py-7 text-center"
      >
        {faces.length > 0 && (
          <ul aria-label="Quem está na sala" className="flex items-center justify-center -space-x-3">
            {faces.map((m, i) => (
              <li
                key={m.id}
                className="animate-entry-pop rounded-full ring-4 ring-[#16161c]"
                style={{ animationDelay: `${i * 90}ms` }}
              >
                <Avatar name={m.name} size={48} src={memberPhoto(m, false, profile, 96)} />
              </li>
            ))}
            {extra > 0 && (
              <li className="grid size-12 animate-entry-pop place-items-center rounded-full bg-surface-3 text-sm font-semibold ring-4 ring-[#16161c]">
                +{extra}
              </li>
            )}
          </ul>
        )}
        <div className="flex flex-col gap-1">
          <h2 className="text-xl font-bold">{roomTitle(code, roomName)}</h2>
          <p className="text-sm text-text-muted">{ready ? "Você está na sala." : "Entrando na sala…"}</p>
        </div>
        <div
          role="progressbar"
          aria-label="Entrada na sala"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(progress * 100)}
          className="h-2 w-full overflow-hidden rounded-full bg-white/10"
        >
          <div
            className="h-full origin-left rounded-full bg-brand transition-transform duration-(--duration-cena) ease-out"
            style={{ transform: `scaleX(${progress})` }}
          />
        </div>
        <ol aria-label="Etapas" className="flex w-full flex-col gap-2 text-left text-sm">
          {steps.map((step) => (
            <li key={step.text} className={`flex items-center gap-2.5 ${step.done ? "text-text" : "text-text-muted"}`}>
              <span
                className={`grid size-5 shrink-0 place-items-center rounded-full ${step.done ? "bg-online text-black" : "bg-white/10"}`}
              >
                {step.done ? (
                  <Check size={12} strokeWidth={3.5} aria-hidden />
                ) : (
                  <Loader2 size={12} className="animate-spin" aria-hidden />
                )}
              </span>
              {step.text}
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}
