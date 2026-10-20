import { CircleAlert, CircleCheck, Info, OctagonX, TriangleAlert, X } from "lucide-react";
import { type CSSProperties, useEffect, useRef, useState } from "react";
import { type Toast, type ToastKind, useToastStore } from "./toast-store";

const STYLE: Record<ToastKind, { icon: typeof Info; color: string; label: string }> = {
  info: { icon: Info, color: "var(--color-brand)", label: "Aviso do sistema" },
  success: { icon: CircleCheck, color: "var(--color-online)", label: "Sucesso" },
  warning: { icon: TriangleAlert, color: "var(--color-warning)", label: "Aviso" },
  alert: { icon: CircleAlert, color: "#f47b20", label: "Alerta" },
  error: { icon: OctagonX, color: "var(--color-live)", label: "Erro" },
};

/** Distância entre os cards empilhados (fechado) e entre eles abertos (mouse em cima). */
const STACK_OFFSET = 14;
const STACK_GAP = 8;
/** Quantos aparecem na pilha fechada; os outros ficam escondidos atrás. */
const VISIBLE_STACKED = 3;

/**
 * Notificações no topo do app: cada uma desce como um card. Várias ficam empilhadas (a mais nova
 * na frente); com o mouse em cima a pilha se abre e os tempos pausam.
 */
export function Toaster(props: { anchor?: "center" | "sidebar" }) {
  const toasts = useToastStore((s) => s.toasts);
  const [expanded, setExpanded] = useState(false);
  // Altura de cada card, para abrir a pilha sem sobrepor.
  const [heights, setHeights] = useState<Record<number, number>>({});

  return (
    <section
      aria-label="Notificações"
      onMouseEnter={() => setExpanded(true)}
      onMouseLeave={() => setExpanded(false)}
      // Com vídeo na tela, as notificações ficam sobre a barra lateral: o vídeo é uma janela nativa
      // por cima da interface e esconderia o que caísse em cima dele.
      className={`pointer-events-none fixed top-11 z-50 max-w-[calc(100vw-24px)] transition-[left,width] duration-300 ${
        props.anchor === "sidebar" ? "left-2 w-[296px]" : "left-1/2 w-[380px] -translate-x-1/2"
      }`}
    >
      <ol className="relative">
        {toasts.map((t, index) => {
          // Posição na pilha: só os que não estão saindo contam.
          const order = toasts.filter((x, i) => i < index && !x.leaving).length;
          const before = toasts.slice(0, index).filter((x) => !x.leaving);
          const expandedY = before.reduce((y, x) => y + (heights[x.id] ?? 64) + STACK_GAP, 0);
          const style: CSSProperties = expanded
            ? { transform: `translateY(${expandedY}px)`, opacity: 1, zIndex: 100 - order }
            : {
                transform: `translateY(${order * STACK_OFFSET}px) scale(${1 - order * 0.04})`,
                opacity: order >= VISIBLE_STACKED ? 0 : 1,
                zIndex: 100 - order,
              };
          return (
            <ToastCard
              key={t.id}
              toast={t}
              paused={expanded}
              front={order === 0}
              expanded={expanded}
              style={style}
              onHeight={(h) => setHeights((prev) => (prev[t.id] === h ? prev : { ...prev, [t.id]: h }))}
            />
          );
        })}
      </ol>
    </section>
  );
}

function ToastCard(props: {
  toast: Toast;
  paused: boolean;
  front: boolean;
  expanded: boolean;
  style: CSSProperties;
  onHeight: (h: number) => void;
}) {
  const { toast, paused } = props;
  const dismiss = useToastStore((s) => s.dismiss);
  const ref = useRef<HTMLLIElement>(null);
  const { icon: Icon, color, label } = STYLE[toast.kind];
  const remaining = useRef(toast.duration);
  const onHeight = props.onHeight;

  useEffect(() => {
    if (ref.current) onHeight(ref.current.offsetHeight);
  });

  // Conta o tempo só enquanto não está pausado (mouse em cima); ao acabar, sai.
  useEffect(() => {
    if (toast.duration === 0 || toast.leaving || paused) return;
    const started = Date.now();
    const timer = setTimeout(() => dismiss(toast.id), remaining.current);
    return () => {
      clearTimeout(timer);
      remaining.current = Math.max(0, remaining.current - (Date.now() - started));
    };
  }, [paused, toast.duration, toast.id, toast.leaving, dismiss]);

  const urgent = toast.kind === "error" || toast.kind === "alert";
  return (
    <li
      ref={ref}
      role={urgent ? "alert" : "status"}
      aria-label={label}
      // O vídeo nativo abre um buraco aqui: o aviso aparece por cima da live.
      data-video-hole
      style={{ ...props.style, "--toast-color": color } as CSSProperties}
      className={`pointer-events-auto absolute inset-x-0 top-0 origin-top overflow-hidden rounded-card border border-white/10 ${
        // Os de trás ficam um tom mais claro: a pilha fica visível atrás do da frente.
        props.front || props.expanded ? "bg-surface-1/95" : "bg-surface-3"
      } shadow-2xl shadow-black/50 backdrop-blur-md transition-[transform,opacity] duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] ${
        toast.leaving ? "animate-toast-out" : "animate-toast-in"
      }`}
    >
      {/* Faixa de cor do tipo, na esquerda. */}
      <span aria-hidden className="absolute inset-y-0 left-0 w-1" style={{ background: color }} />
      <div className="flex items-start gap-3 py-3 pr-2 pl-4">
        <span
          aria-hidden
          className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-full"
          style={{ background: `color-mix(in srgb, ${color} 18%, transparent)`, color }}
        >
          <Icon size={16} />
        </span>
        <div className="min-w-0 flex-1 text-left">
          {toast.title && <p className="text-sm font-semibold">{toast.title}</p>}
          <p className={`text-sm ${toast.title ? "text-text-muted" : ""}`}>{toast.message}</p>
        </div>
        <button
          type="button"
          aria-label="Fechar notificação"
          onClick={() => dismiss(toast.id)}
          className="shrink-0 rounded p-1 text-text-faint transition-colors hover:bg-surface-hover hover:text-text"
        >
          <X size={14} />
        </button>
      </div>
      {/* Barra do tempo: esvazia do início ao fim; para enquanto o mouse está em cima. */}
      {toast.duration > 0 && (
        <span
          aria-hidden
          data-testid="tempo-da-notificacao"
          className="absolute bottom-0 left-0 h-0.5 w-full origin-left animate-toast-timer"
          style={{
            background: color,
            animationDuration: `${toast.duration}ms`,
            animationPlayState: paused || !props.front ? "paused" : "running",
          }}
        />
      )}
    </li>
  );
}
