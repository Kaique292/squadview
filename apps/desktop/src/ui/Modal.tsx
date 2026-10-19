import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { type ReactNode, useEffect } from "react";
import { holdOverlay } from "./overlay";
import { ScrollArea } from "./ScrollArea";

interface ModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Botão que abre o modal (opcional: o modal também pode ser aberto pelo estado). */
  trigger?: ReactNode;
  title: string;
  description?: ReactNode;
  /** Ilustração acima do título, saindo um pouco para fora do modal (como no Discord). */
  illustration?: ReactNode;
  children: ReactNode;
  /** Barra de botões no rodapé. */
  footer?: ReactNode;
  /** Largura em px. */
  width?: number;
}

/**
 * Base de todos os modais do app: overlay escuro, conteúdo centralizado, animação de
 * entrada e saída, foco preso dentro dele e fechar com Esc ou clicando fora.
 */
export function Modal(props: ModalProps) {
  // O vídeo nativo fica por cima da interface: some enquanto o modal estiver aberto.
  useEffect(() => (props.open ? holdOverlay() : undefined), [props.open]);
  return (
    <Dialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      {props.trigger && <Dialog.Trigger asChild>{props.trigger}</Dialog.Trigger>}
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70 data-[state=closed]:animate-overlay-out data-[state=open]:animate-overlay-in" />
        <Dialog.Content
          style={{
            width: props.width ?? 520,
            // Com ilustração, desce metade da parte que sai para fora, para ela não cortar no topo.
            top: props.illustration ? "calc(50% + 32px)" : "50%",
            maxHeight: props.illustration ? "calc(100vh - 112px)" : "88vh",
          }}
          className="fixed left-1/2 z-40 flex max-w-[calc(100vw-32px)] transition-[width] duration-200 ease-out -translate-x-1/2 -translate-y-1/2 flex-col rounded-card border border-white/[0.06] bg-glass-modal text-text shadow-2xl backdrop-blur-xl focus:outline-none data-[state=closed]:animate-modal-out data-[state=open]:animate-modal-in"
        >
          {props.illustration && (
            <div aria-hidden className="pointer-events-none -mt-16 flex justify-center">
              {props.illustration}
            </div>
          )}
          <Dialog.Close
            aria-label="Fechar"
            className="absolute top-3 right-3 rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text focus:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            <X size={22} />
          </Dialog.Close>
          <header className={`px-6 text-center ${props.illustration ? "pt-2" : "pt-6"}`}>
            <Dialog.Title className="text-2xl font-bold">{props.title}</Dialog.Title>
            {props.description && (
              <Dialog.Description className="mt-1 text-sm text-text-muted">{props.description}</Dialog.Description>
            )}
          </header>
          {/* flex-auto: cresce com o conteúdo e só rola quando passa da altura máxima do modal. */}
          <ScrollArea className="mt-4 flex min-h-0 flex-auto flex-col" viewportClassName="px-6">
            {props.children}
          </ScrollArea>
          {props.footer && (
            <footer className="mt-5 flex items-center justify-between gap-2 rounded-b-card bg-glass-modal-footer px-6 py-4">
              {props.footer}
            </footer>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** Fecha o modal; use para o botão "Cancelar" do rodapé. */
export const ModalClose = Dialog.Close;
