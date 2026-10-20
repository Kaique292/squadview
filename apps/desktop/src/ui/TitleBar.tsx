import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import logo from "../assets/logo.svg";
import { UpdateButton } from "../update/UpdateButton";
import { inTauri } from "./tauri-env";

/**
 * Barra de título própria, como a do Discord: na mesma cor da barra de salas, arrastável, com o
 * nome no meio e os botões da janela no estilo do Windows.
 */
export function TitleBar(props: { title?: string }) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!inTauri()) return;
    // Sem janela de verdade (ex.: modo de demonstração no navegador): fica sem o estado maximizado.
    let win: ReturnType<typeof getCurrentWindow>;
    try {
      win = getCurrentWindow();
    } catch {
      return;
    }
    const update = () => void win.isMaximized().then(setMaximized, () => {});
    update();
    const off = win.onResized(update).catch(() => () => {});
    return () => void off.then((fn) => fn());
  }, []);

  const run = (action: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => {
    if (!inTauri()) return;
    try {
      void action(getCurrentWindow()).catch(() => {});
    } catch {
      // Sem janela de verdade.
    }
  };

  return (
    <header data-tauri-drag-region className="col-span-full flex h-8 shrink-0 items-center select-none">
      <div className="pointer-events-none flex w-[72px] justify-center">
        <img src={logo} alt="" className="size-4 opacity-80" />
      </div>
      <p className="pointer-events-none flex-1 truncate text-center text-[13px] font-semibold text-text-muted">
        {props.title ?? "SquadView"}
      </p>
      <div className="flex h-full">
        <UpdateButton />
        <WindowButton label="Minimizar" onClick={() => run((w) => w.minimize())}>
          <Minus size={16} />
        </WindowButton>
        <WindowButton label={maximized ? "Restaurar" : "Maximizar"} onClick={() => run((w) => w.toggleMaximize())}>
          {maximized ? <Copy size={13} className="-scale-x-100" /> : <Square size={12} />}
        </WindowButton>
        <WindowButton label="Fechar" danger onClick={() => run((w) => w.close())}>
          <X size={17} />
        </WindowButton>
      </div>
    </header>
  );
}

function WindowButton(props: { label: string; danger?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      onClick={props.onClick}
      className={`grid h-full w-[46px] place-items-center text-text-muted transition-colors ${
        props.danger ? "hover:bg-[#da373c] hover:text-white" : "hover:bg-white/10 hover:text-text"
      }`}
    >
      {props.children}
    </button>
  );
}
