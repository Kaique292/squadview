import { type CSSProperties, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";

/** Quanto o conteúdo da aba que sai leva para sumir antes de a nova entrar (ms). */
export const TAB_OUT_MS = 120;

/**
 * Abas do app: a barra embaixo desliza até a aba escolhida (em vez de pular). Use com `TabContent`
 * para o conteúdo também trocar com animação.
 */
export function Tabs<T extends string>(props: {
  label: string;
  tabs: readonly (readonly [T, ReactNode])[];
  value: T;
  onChange: (tab: T) => void;
  className?: string;
}) {
  const list = useRef<HTMLDivElement>(null);
  const [bar, setBar] = useState<{ left: number; width: number } | null>(null);
  // A primeira posição aparece direto; daí em diante a barra desliza.
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const measure = () => {
      const active = list.current?.querySelector<HTMLElement>('[aria-selected="true"]');
      if (active) setBar({ left: active.offsetLeft, width: active.offsetWidth });
    };
    measure();
    // A largura das abas muda com a fonte carregando ou a janela mudando de tamanho.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    if (list.current) observer?.observe(list.current);
    return () => observer?.disconnect();
  }, [props.value]);

  useEffect(() => {
    if (!bar || ready) return;
    const frame = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(frame);
  }, [bar, ready]);

  return (
    <div
      ref={list}
      role="tablist"
      aria-label={props.label}
      className={`relative flex gap-1 border-b border-white/[0.06] ${props.className ?? ""}`}
    >
      {props.tabs.map(([id, label]) => (
        <button
          key={id}
          type="button"
          role="tab"
          aria-selected={props.value === id}
          onClick={() => props.onChange(id)}
          className={`px-3 py-2 text-sm font-medium transition-colors ${
            props.value === id ? "text-text" : "text-text-muted hover:text-text"
          }`}
        >
          {label}
        </button>
      ))}
      {bar && (
        <span
          aria-hidden
          data-tab-bar
          className={`absolute -bottom-px left-0 h-0.5 rounded-full bg-brand ${
            ready ? "transition-[transform,width] duration-200 ease-out" : ""
          }`}
          style={{ width: bar.width, transform: `translateX(${bar.left}px)` }}
        />
      )}
    </div>
  );
}

/**
 * Conteúdo das abas: o da aba que sai desliza e some, depois o da nova entra pelo lado para onde
 * você foi (para a direita se a aba nova está depois, para a esquerda se está antes).
 */
export function TabContent<T extends string>(props: {
  value: T;
  /** As abas na ordem em que aparecem (para saber o lado da animação). */
  order: readonly T[];
  children: (tab: T) => ReactNode;
  className?: string;
}) {
  const [shown, setShown] = useState(props.value);
  const [leaving, setLeaving] = useState(false);
  const [direction, setDirection] = useState(1);
  if (props.value !== shown && !leaving) {
    setDirection(props.order.indexOf(props.value) >= props.order.indexOf(shown) ? 1 : -1);
    setLeaving(true);
  }

  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => {
      setShown(props.value);
      setLeaving(false);
    }, TAB_OUT_MS);
    return () => clearTimeout(timer);
  }, [leaving, props.value]);

  return (
    <div
      key={shown}
      role="tabpanel"
      data-leaving={leaving || undefined}
      className={`${leaving ? "animate-tab-out" : "animate-tab-in"} ${props.className ?? ""}`}
      style={{ "--tab-direction": direction } as CSSProperties}
    >
      {props.children(shown)}
    </div>
  );
}
