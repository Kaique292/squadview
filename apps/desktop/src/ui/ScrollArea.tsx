import * as Scroll from "@radix-ui/react-scroll-area";
import type { ReactNode, Ref } from "react";

interface ScrollAreaProps {
  children: ReactNode;
  /** Classes da raiz (tamanho, flex). */
  className?: string;
  /** Classes da área rolável (espaçamento interno). */
  viewportClassName?: string;
  /** A área rolável, para quem precisa rolar por código (ex.: descer até a última linha). */
  viewportRef?: Ref<HTMLDivElement>;
  onViewportScroll?: (viewport: HTMLDivElement) => void;
}

/**
 * Área com rolagem no estilo do Discord: a barra é fina, fica por cima do conteúdo (não ocupa
 * espaço), aparece com o mouse em cima ou ao rolar e some com um fade quando o mouse sai.
 * Use no lugar de `overflow-auto` em qualquer lista ou painel do app.
 */
export function ScrollArea({ children, className, viewportClassName, viewportRef, onViewportScroll }: ScrollAreaProps) {
  return (
    <Scroll.Root type="hover" scrollHideDelay={500} className={`relative overflow-hidden ${className ?? ""}`}>
      {/* O Radix envolve o conteúdo numa caixa "display: table" que cresce com o item mais largo
          (ex.: título longo de janela) e empurra o resto para fora. Forçar "block" mantém a largura. */}
      <Scroll.Viewport
        ref={viewportRef}
        onScroll={onViewportScroll && ((e) => onViewportScroll(e.currentTarget))}
        className={`h-full w-full [&>div]:!block ${viewportClassName ?? ""}`}
      >
        {children}
      </Scroll.Viewport>
      <Scroll.Scrollbar
        orientation="vertical"
        className="flex w-2.5 touch-none p-0.5 select-none data-[state=hidden]:animate-scrollbar-out data-[state=visible]:animate-scrollbar-in"
      >
        <Scroll.Thumb className="relative flex-1 rounded-full bg-surface-0 transition-colors hover:bg-black/80" />
      </Scroll.Scrollbar>
    </Scroll.Root>
  );
}
