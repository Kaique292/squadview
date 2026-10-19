import { Download, X } from "lucide-react";
import { useUpdateStore } from "./update-store";

/** Faixa no topo da área principal avisando que há uma versão nova. */
export function UpdateBanner() {
  const status = useUpdateStore((s) => s.status);
  const install = useUpdateStore((s) => s.install);
  const dismiss = useUpdateStore((s) => s.dismiss);

  if (status.kind === "idle") return null;

  return (
    <div
      role="status"
      className="flex items-center justify-between gap-3 bg-brand px-4 py-2 text-sm font-medium text-white"
    >
      {status.kind === "available" && (
        <>
          <span>Nova versão disponível: {status.version}</span>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={install}
              className="flex items-center gap-1.5 rounded bg-white/15 px-3 py-1 hover:bg-white/25"
            >
              <Download size={16} /> Atualizar e reiniciar
            </button>
            <button type="button" aria-label="Agora não" onClick={dismiss} className="rounded p-1 hover:bg-white/15">
              <X size={16} />
            </button>
          </div>
        </>
      )}
      {status.kind === "downloading" && (
        <span>Baixando atualização{status.percent !== null ? `… ${status.percent}%` : "…"}</span>
      )}
      {status.kind === "error" && (
        <>
          <span>{status.message}</span>
          <button type="button" aria-label="Fechar aviso" onClick={dismiss} className="rounded p-1 hover:bg-white/15">
            <X size={16} />
          </button>
        </>
      )}
    </div>
  );
}
