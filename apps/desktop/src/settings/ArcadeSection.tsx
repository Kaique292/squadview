import { invoke } from "@tauri-apps/api/core";
import { CheckCircle2, Loader2, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { DriverMissing } from "../arcade/ArcadeHostDialog";
import type { DriverInfo, KeyMap, PadInput } from "../arcade/arcade-store";
import { useRoomStore } from "../room/room-store";
import { Modal, ModalClose } from "../ui/Modal";
import { toast } from "../ui/toast-store";

/** As entradas do controle, na ordem e com o nome que a pessoa vê. */
const INPUTS: { input: PadInput; label: string }[] = [
  { input: "left-stick-up", label: "Analógico esquerdo: cima" },
  { input: "left-stick-down", label: "Analógico esquerdo: baixo" },
  { input: "left-stick-left", label: "Analógico esquerdo: esquerda" },
  { input: "left-stick-right", label: "Analógico esquerdo: direita" },
  { input: "a", label: "A" },
  { input: "b", label: "B" },
  { input: "x", label: "X" },
  { input: "y", label: "Y" },
  { input: "left-shoulder", label: "LB" },
  { input: "right-shoulder", label: "RB" },
  { input: "left-trigger", label: "LT (gatilho esquerdo)" },
  { input: "right-trigger", label: "RT (gatilho direito)" },
  { input: "start", label: "Start" },
  { input: "back", label: "Back" },
  { input: "dpad-up", label: "Direcional: cima" },
  { input: "dpad-down", label: "Direcional: baixo" },
  { input: "dpad-left", label: "Direcional: esquerda" },
  { input: "dpad-right", label: "Direcional: direita" },
  { input: "left-thumb", label: "Apertar o analógico esquerdo" },
  { input: "right-thumb", label: "Apertar o analógico direito" },
];

/** O nome de uma tecla (`KeyboardEvent.code`) para a pessoa ler: "W", "Espaço", "↑". */
export function keyLabel(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return `Num ${code.slice(6)}`;
  const names: Record<string, string> = {
    Space: "Espaço",
    Enter: "Enter",
    Backspace: "Backspace",
    Tab: "Tab",
    ShiftLeft: "Shift esq.",
    ShiftRight: "Shift dir.",
    ControlLeft: "Ctrl esq.",
    ControlRight: "Ctrl dir.",
    AltLeft: "Alt",
    AltRight: "AltGr",
    ArrowUp: "↑",
    ArrowDown: "↓",
    ArrowLeft: "←",
    ArrowRight: "→",
  };
  return names[code] ?? code;
}

/**
 * Configurações › Fliperama (ADR 0023 e 0024): o driver de controle virtual do host (instalar e
 * remover quando quiser) e o teclado de quem joga (que tecla aperta qual botão).
 */
export function ArcadeSection() {
  return (
    <div className="flex flex-col gap-8">
      <DriverPart />
      <KeysPart />
    </div>
  );
}

function DriverPart() {
  const [driver, setDriver] = useState<DriverInfo | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [removing, setRemoving] = useState(false);
  const hosting = useRoomStore((s) => !!s.members[0]?.arcade);

  const refresh = () =>
    void invoke<DriverInfo>("arcade_driver").then(setDriver, () =>
      setDriver({ status: "unavailable", detail: null, version: null, removable: false }),
    );
  useEffect(refresh, []);

  async function remove() {
    setRemoving(true);
    try {
      const after = await invoke<DriverInfo>("arcade_uninstall_driver");
      setDriver(after);
      toast.success(
        after.status === "installed"
          ? "O Windows ainda mostra o driver. Pode ser preciso reiniciar o PC."
          : "Driver de controle removido.",
        "Fliperama",
      );
    } catch (e) {
      toast.error(typeof e === "string" ? e : "Não deu para remover o driver.", "Fliperama");
    } finally {
      setRemoving(false);
      setConfirm(false);
    }
  }

  return (
    <section aria-labelledby="fliperama-driver" className="flex flex-col gap-3">
      <h3 id="fliperama-driver" className="text-xs font-semibold tracking-wide text-text-muted uppercase">
        Driver de controle (quem hospeda)
      </h3>
      {!driver ? (
        <p className="flex items-center gap-2 text-sm text-text-muted">
          <Loader2 size={16} className="animate-spin" /> Conferindo…
        </p>
      ) : driver.status === "installed" ? (
        <div className="flex items-center gap-3 rounded-md bg-surface-0 px-3 py-2.5">
          <CheckCircle2 size={20} className="shrink-0 text-online" />
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium">ViGEmBus instalado{driver.version ? ` (${driver.version})` : ""}</p>
            <p className="text-xs text-text-faint">Cada jogador do seu fliperama vira um controle de Xbox no seu PC.</p>
          </div>
          {driver.removable && (
            <button
              type="button"
              disabled={hosting}
              title={hosting ? "Feche o fliperama antes de remover o driver." : undefined}
              onClick={() => setConfirm(true)}
              className="flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium text-live transition-colors hover:bg-live hover:text-white disabled:opacity-50"
            >
              <Trash2 size={16} />
              Remover o driver de controle
            </button>
          )}
        </div>
      ) : (
        <div className="rounded-md bg-surface-0 px-3 py-3">
          <DriverMissing driver={driver} />
          <button type="button" onClick={refresh} className="mt-2 text-xs font-medium text-brand hover:underline">
            Já instalei: conferir de novo
          </button>
        </div>
      )}
      <Modal
        open={confirm}
        onOpenChange={setConfirm}
        title="Remover o driver de controle?"
        description="O Windows vai pedir permissão de administrador. Para abrir um fliperama de novo, é só instalar outra vez."
        width={440}
        footer={
          <>
            <ModalClose asChild>
              <button
                type="button"
                className="rounded-md px-4 py-2 text-sm font-medium text-text-muted hover:text-text"
              >
                Cancelar
              </button>
            </ModalClose>
            <button
              type="button"
              disabled={removing}
              onClick={() => void remove()}
              className="flex items-center gap-2 rounded-md bg-live px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
            >
              {removing && <Loader2 size={16} className="animate-spin" />}
              Remover
            </button>
          </>
        }
      >
        <span />
      </Modal>
    </section>
  );
}

function KeysPart() {
  const [map, setMap] = useState<KeyMap | null>(null);
  const [binding, setBinding] = useState<PadInput | null>(null);

  useEffect(() => {
    void invoke<KeyMap>("arcade_keymap").then(setMap, () => setMap({ keys: {} }));
  }, []);

  // Esperando a tecla nova: a próxima apertada vira essa entrada (Esc cancela).
  useEffect(() => {
    if (!binding || !map) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setBinding(null);
      if (e.code === "Escape") return;
      // A tecla passa a fazer só isto; a entrada perde as teclas antigas.
      const keys = Object.fromEntries(
        Object.entries(map.keys).filter(([code, input]) => code !== e.code && input !== binding),
      ) as Record<string, PadInput>;
      keys[e.code] = binding;
      const next = { keys };
      setMap(next);
      void invoke("arcade_set_keymap", { keymap: next }).catch(() => toast.error("Não deu para salvar.", "Fliperama"));
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [binding, map]);

  async function reset() {
    try {
      setMap(await invoke<KeyMap>("arcade_reset_keymap"));
    } catch {
      toast.error("Não deu para voltar ao padrão.", "Fliperama");
    }
  }

  return (
    <section aria-labelledby="fliperama-teclado" className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <h3 id="fliperama-teclado" className="text-xs font-semibold tracking-wide text-text-muted uppercase">
          Teclado de quem joga
        </h3>
        <button
          type="button"
          onClick={() => void reset()}
          className="flex items-center gap-1.5 text-xs font-medium text-text-muted hover:text-text"
        >
          <RotateCcw size={14} />
          Voltar ao padrão
        </button>
      </div>
      <p className="text-xs text-text-faint">
        Jogando no fliperama de alguém sem controle, suas teclas viram estes botões. Com o ponteiro preso no jogo, o
        mouse vira o analógico direito, o botão esquerdo o RT e o direito o LT.
      </p>
      {!map ? (
        <Loader2 size={16} className="animate-spin text-text-muted" />
      ) : (
        <ul aria-label="Teclas do controle" className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
          {INPUTS.map(({ input, label }) => {
            const keys = Object.entries(map.keys)
              .filter(([, v]) => v === input)
              .map(([code]) => keyLabel(code));
            const waiting = binding === input;
            return (
              <li key={input} className="flex items-center justify-between gap-2 rounded-md bg-surface-0 px-3 py-1.5">
                <span className="truncate text-sm">{label}</span>
                <button
                  type="button"
                  aria-label={`Trocar a tecla de ${label}`}
                  onClick={() => setBinding(waiting ? null : input)}
                  className={`min-w-16 rounded px-2 py-1 font-mono text-xs transition-colors ${
                    waiting ? "bg-brand text-white" : "bg-surface-2 text-text hover:bg-surface-hover"
                  }`}
                >
                  {waiting ? "Aperte…" : keys.length ? keys.join(" / ") : "—"}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
