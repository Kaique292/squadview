import type { ReactNode } from "react";
import { Tooltip } from "../ui/Tooltip";
import { useStreamStore } from "./stream-store";

/**
 * Botão da barra do vídeo. A dica abre para baixo: em cima dela está o vídeo nativo, que fica por
 * cima de qualquer coisa da interface e a esconderia. Em tela cheia não sobra espaço embaixo, então
 * vale a dica do próprio Windows.
 */
export function BarButton(props: { label: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  const fullscreen = useStreamStore((s) => s.fullscreen);
  const button = (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={props.active}
      title={fullscreen ? props.label : undefined}
      onClick={props.onClick}
      className={`rounded-md p-2 transition-colors hover:bg-surface-hover hover:text-text ${
        props.active ? "text-text" : "text-text-muted"
      }`}
    >
      {props.children}
    </button>
  );
  return fullscreen ? (
    button
  ) : (
    <Tooltip label={props.label} side="bottom">
      {button}
    </Tooltip>
  );
}
