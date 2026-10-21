import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Switch } from "../ui/Switch";

/** Espelha `tray::WindowSettings` no Rust. */
interface WindowSettings {
  closeToTray: boolean;
}

/**
 * Windows: fechar a janela manda para a bandeja (perto do relógio), como no Discord. Quem decide é o
 * Rust, na hora de fechar; a escolha fica salva no PC.
 */
export function WindowsSection() {
  // Ligado até o Rust responder (é o padrão).
  const [settings, setSettings] = useState<WindowSettings>({ closeToTray: true });
  useEffect(() => {
    invoke<WindowSettings>("window_settings")
      .then((s) => s && setSettings(s))
      .catch(() => {});
  }, []);

  function setCloseToTray(enabled: boolean) {
    setSettings({ closeToTray: enabled });
    invoke<WindowSettings>("set_close_to_tray", { enabled })
      .then((s) => s && setSettings(s))
      .catch(() => {});
  }

  return (
    <section aria-labelledby="windows" className="flex flex-col gap-3">
      <h3 id="windows" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Configurações do Windows
      </h3>
      <Switch
        label="Minimizar para a bandeja ao fechar"
        hint="Fechar a janela deixa o SquadView aberto perto do relógio, e você continua na sala. Para sair de vez, clique com o botão direito no ícone e escolha Sair."
        checked={settings.closeToTray}
        onChange={setCloseToTray}
      />
    </section>
  );
}
