import { invoke } from "@tauri-apps/api/core";
import { ClipboardCopy, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { Switch } from "../ui/Switch";
import { toast } from "../ui/toast-store";

/** Espelha `diagnostics::DiagnosticsStatus`. */
interface Status {
  enabled: boolean;
  files: number;
}

/**
 * Registro de desempenho: liga e desliga, e copia os arquivos para colar numa conversa do
 * Discord (para quem for analisar). Nada é enviado sozinho.
 */
export function DiagnosticsSection() {
  const [status, setStatus] = useState<Status | null>(null);

  useEffect(() => {
    invoke<Status>("diagnostics_status").then(setStatus, () => setStatus(null));
  }, []);

  async function toggle(enabled: boolean) {
    try {
      setStatus(await invoke<Status>("set_diagnostics_enabled", { enabled }));
    } catch (e) {
      toast.error(String(e), "Registro de desempenho");
    }
  }

  async function copy() {
    try {
      const n = await invoke<number>("copy_diagnostics");
      toast.success(
        `${n === 1 ? "1 registro copiado" : `${n} registros copiados`}. Cole (Ctrl+V) na conversa do Discord.`,
        "Registro de desempenho",
      );
    } catch (e) {
      toast.warning(String(e), "Registro de desempenho");
    }
  }

  async function reveal() {
    try {
      await invoke("reveal_diagnostics");
    } catch (e) {
      toast.warning(String(e), "Registro de desempenho");
    }
  }

  const hasFiles = (status?.files ?? 0) > 0;
  return (
    <section aria-labelledby="desempenho" className="flex flex-col gap-3 text-left">
      <h3 id="desempenho" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Desempenho
      </h3>
      <div className="flex flex-col gap-3">
        <Switch
          label="Gravar registro de desempenho"
          hint="Fica só neste PC: uso de GPU e CPU, FPS, bitrate e latência das transmissões, o nome da placa de vídeo e da janela transmitida. Nada é enviado; você escolhe se manda."
          checked={status?.enabled ?? false}
          onChange={(v) => void toggle(v)}
          disabled={!status}
        />
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => void copy()}
            disabled={!hasFiles}
            className="flex items-center gap-2 rounded-md bg-surface-3 px-3 py-1.5 text-sm font-medium transition-colors hover:bg-surface-hover disabled:opacity-50"
          >
            <ClipboardCopy size={16} /> Copiar registros
          </button>
          <button
            type="button"
            onClick={() => void reveal()}
            disabled={!hasFiles}
            className="flex items-center gap-2 rounded-md px-3 py-1.5 text-sm text-text-muted transition-colors hover:bg-surface-hover hover:text-text disabled:opacity-50"
          >
            <FolderOpen size={16} /> Mostrar na pasta
          </button>
        </div>
        <p className="text-xs text-text-faint">
          Copia os 3 registros mais recentes como arquivos. Para ajudar a melhorar o app, cole na conversa com quem te
          passou o SquadView.
        </p>
      </div>
    </section>
  );
}
