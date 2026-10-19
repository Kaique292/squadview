import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/index.css";
import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { useAccountStore } from "./account/account-store";
import { listenToRoomEvents, useRoomStore } from "./room/room-store";
import { applyGlass, useAppearanceStore } from "./settings/appearance-store";
import { SPLASH_EVENT, boot } from "./splash/boot";
import { listenToStreamEvents, useStreamStore } from "./stream/stream-store";
import { toast } from "./ui/toast-store";
import { CHECK_EVERY_MS, useUpdateStore } from "./update/update-store";

// Vidro antes do primeiro desenho, para a janela já abrir com a intensidade salva.
applyGlass(useAppearanceStore.getState().glass);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// Abertura: a splash mostra o andamento e a janela principal só aparece pronta.
if ("__TAURI_INTERNALS__" in window) {
  void boot({
    steps: [
      { text: "Conectando os fios…", run: () => Promise.all([listenToStreamEvents(), listenToRoomEvents()]) },
      { text: "Carregando seu perfil…", run: () => useAccountStore.getState().load() },
      // Só no app instalado (no `yarn dev` não há updater).
      ...(import.meta.env.DEV
        ? []
        : [{ text: "Procurando atualizações…", run: () => useUpdateStore.getState().checkForUpdate() }]),
    ],
    progress: (p) => void emitTo("splash", SPLASH_EVENT, p).catch(() => {}),
    ready: () => invoke("app_ready"),
  });
  // Com o app aberto, procura versão nova de hora em hora (o botão verde aparece na barra de título).
  if (!import.meta.env.DEV) {
    setInterval(() => void useUpdateStore.getState().checkForUpdate(), CHECK_EVERY_MS);
  }
}

// Só em desenvolvimento: cenas de exemplo para as capturas do README (`#demo=sala`).
if (import.meta.env.DEV && location.hash.startsWith("#demo=")) {
  void import("./dev/demo").then((m) => m.runDemo(location.hash.slice("#demo=".length)));
}

// Só em desenvolvimento: mexer no app pelo console (`__sv.toast.error("teste")`, `__sv.room.setState(...)`).
if (import.meta.env.DEV) {
  Object.assign(window, {
    __sv: {
      toast,
      room: useRoomStore,
      stream: useStreamStore,
      appearance: useAppearanceStore,
      account: useAccountStore,
      update: useUpdateStore,
    },
  });
}
