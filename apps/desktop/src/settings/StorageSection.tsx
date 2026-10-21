import { invoke } from "@tauri-apps/api/core";
import { FileText, Image, Loader2 } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";
import { toast } from "../ui/toast-store";

/** Espelha `storage::StoragePart`. */
interface StoragePart {
  id: string;
  bytes: number;
}

/** O que cada parte é, para a pessoa saber o que vai sumir (as que o app não conhece não aparecem). */
const PARTS: Record<string, { label: string; hint: string; icon: ReactNode }> = {
  web: {
    label: "Imagens e arquivos baixados",
    hint: "Fotos do Discord e o que a interface baixou. O app baixa de novo quando precisar.",
    icon: <Image size={18} />,
  },
  logs: {
    label: "Registros de desempenho",
    hint: "Registros das sessões anteriores. O desta sessão continua sendo gravado.",
    icon: <FileText size={18} />,
  },
};

/** Tamanho para gente ler: 0 B, 12 KB, 3,4 MB, 1,2 GB. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = value < 10 ? 1 : 0;
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: digits })} ${units[unit]}`;
}

/**
 * Armazenamento: quanto cada parte do app ocupa no PC e o botão de limpar cada uma (ou tudo).
 * Só entra o que o app refaz sozinho: o login, as preferências e os amigos nunca são apagados.
 */
export function StorageSection() {
  const [parts, setParts] = useState<StoragePart[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);

  useEffect(() => {
    invoke<StoragePart[]>("storage_usage").then(setParts, () => setParts([]));
  }, []);

  async function clear(part: string) {
    setBusy(part);
    try {
      const before = parts?.reduce((sum, p) => sum + (part === "all" || p.id === part ? p.bytes : 0), 0) ?? 0;
      const after = await invoke<StoragePart[]>("storage_clear", { part });
      setParts(after);
      const left = after.reduce((sum, p) => sum + (part === "all" || p.id === part ? p.bytes : 0), 0);
      toast.success(`${formatBytes(Math.max(0, before - left))} liberados.`, "Armazenamento");
    } catch (e) {
      toast.error(String(e), "Armazenamento");
    } finally {
      setBusy(null);
    }
  }

  const known = (parts ?? []).filter((p) => PARTS[p.id]);
  const total = known.reduce((sum, p) => sum + p.bytes, 0);

  return (
    <section aria-labelledby="armazenamento" className="flex flex-col gap-3">
      <h3 id="armazenamento" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Cache do app
      </h3>
      <p className="-mt-1 text-xs text-text-faint">
        Limpar não apaga seu login, suas preferências nem seus amigos: só o que o app baixa ou refaz sozinho.
      </p>

      {parts === null ? (
        <p className="flex items-center gap-2 text-sm text-text-muted">
          <Loader2 size={14} className="animate-spin" /> Medindo…
        </p>
      ) : (
        <ul aria-label="Partes do app" className="flex flex-col divide-y divide-white/[0.06] rounded-lg bg-surface-1">
          {known.map((p) => (
            <li key={p.id} className="flex items-center gap-3 px-4 py-3">
              <span className="text-text-muted">{PARTS[p.id].icon}</span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">{PARTS[p.id].label}</p>
                <p className="text-xs text-text-faint">{PARTS[p.id].hint}</p>
              </div>
              <span className="shrink-0 font-mono text-sm text-text-muted tabular-nums">{formatBytes(p.bytes)}</span>
              <button
                type="button"
                aria-label={`Limpar ${PARTS[p.id].label.toLowerCase()}`}
                disabled={busy !== null || p.bytes === 0}
                onClick={() => void clear(p.id)}
                className="grid h-8 w-20 shrink-0 place-items-center rounded-md bg-surface-3 text-sm font-medium transition-colors hover:bg-surface-hover disabled:cursor-default disabled:opacity-50 disabled:hover:bg-surface-3"
              >
                {busy === p.id ? <Loader2 size={14} className="animate-spin" /> : "Limpar"}
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center justify-between gap-3">
        <span className="text-sm text-text-muted">
          Total: <span className="font-mono tabular-nums">{formatBytes(total)}</span>
        </span>
        <button
          type="button"
          disabled={busy !== null || total === 0}
          onClick={() => setConfirmAll(true)}
          className="rounded-md bg-live px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-live/85 disabled:cursor-default disabled:opacity-50 disabled:hover:bg-live"
        >
          {busy === "all" ? "Limpando…" : "Limpar tudo"}
        </button>
      </div>

      <Modal
        open={confirmAll}
        onOpenChange={setConfirmAll}
        title="Limpar todo o cache?"
        description={`Libera ${formatBytes(total)}. Seu login, suas preferências e seus amigos continuam.`}
        width={440}
        footer={
          <>
            <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
            <button
              type="button"
              onClick={() => {
                setConfirmAll(false);
                void clear("all");
              }}
              className="rounded-md bg-live px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-live/85"
            >
              Limpar tudo
            </button>
          </>
        }
      >
        {null}
      </Modal>
    </section>
  );
}
