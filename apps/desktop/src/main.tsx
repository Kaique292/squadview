import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/index.css";
import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { useAccountStore } from "./account/account-store";
import { listenToArcadeEvents } from "./arcade/arcade-store";
import { listenToDmEvents } from "./dm/dm-store";
import { listenToFriendsEvents, useFriendsStore } from "./friends/friends-store";
import { listenToRoomEvents, rejoinAfterUpdate, useRoomStore } from "./room/room-store";
import { applyGlass, useAppearanceStore } from "./settings/appearance-store";
import { listenToSoundEvents } from "./sounds/sound-store";
import { SPLASH_EVENT, SPLASH_UPDATE_EVENT, boot } from "./splash/boot";
import { listenToVoiceEvents, useVoiceStore } from "./voice/voice-store";
import { listenToStreamEvents, useStreamStore } from "./stream/stream-store";
import { parsePipHash } from "./stream/pip";
import { PipView } from "./stream/PipView";
import { blockNativeContextMenu } from "./ui/native-menu";
import { toast } from "./ui/toast-store";
import { CHECK_EVERY_MS, useUpdateStore } from "./update/update-store";

// A janelinha do picture in picture é esta mesma página, com outro endereço (ADR 0021): ela só
// mostra os controles por cima do vídeo, sem nada do resto do app.
const pipPage = parsePipHash(location.hash);
if (pipPage) {
  blockNativeContextMenu();
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <PipView page={pipPage} />
    </StrictMode>,
  );
} else {
  startApp();
}

/** O app inteiro, na janela principal. */
function startApp() {
  // Sons de aviso (entrar, sair, mudo, surdo, transmissão).
  listenToSoundEvents();
  blockNativeContextMenu();

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
        {
          text: "Conectando os fios…",
          run: () =>
            Promise.all([
              listenToStreamEvents(),
              listenToRoomEvents(),
              listenToFriendsEvents(),
              listenToDmEvents(),
              listenToArcadeEvents(),
              listenToVoiceEvents((id) => useRoomStore.getState().members.find((m) => m.id === id)?.name),
            ]),
        },
        { text: "Carregando seu perfil…", run: () => useAccountStore.getState().load() },
        // Atualizou estando numa sala: volta para ela sozinho, como o Discord volta para a call.
        { text: "Voltando para a sala…", run: () => rejoinAfterUpdate() },
        // Só no app instalado (no `yarn dev` não há updater).
        ...(import.meta.env.DEV
          ? []
          : [
              { text: "Procurando atualizações…", run: () => useUpdateStore.getState().checkForUpdate() },
              // Tinha versão nova: atualiza já na abertura, com as barras na splash (como o Discord).
              {
                text: "Atualizando…",
                untimed: true,
                run: () =>
                  useUpdateStore
                    .getState()
                    .installAtStartup((p) => void emitTo("splash", SPLASH_UPDATE_EVENT, p).catch(() => {})),
              },
            ]),
      ],
      progress: (p) => void emitTo("splash", SPLASH_EVENT, p).catch(() => {}),
      ready: () => invoke("app_ready"),
    });
    // Com o app aberto, procura versão nova de hora em hora e quando a janela volta ao foco (no
    // máximo a cada 10 min); o botão verde aparece na barra de título.
    if (!import.meta.env.DEV) {
      setInterval(() => void useUpdateStore.getState().checkForUpdate(), CHECK_EVERY_MS);
      window.addEventListener("focus", () => void useUpdateStore.getState().checkOnFocus());
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
        voice: useVoiceStore,
        friends: useFriendsStore,
      },
    });
  }
}
