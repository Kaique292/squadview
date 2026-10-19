import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/index.css";
import { useUpdateStore } from "./update/update-store";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// Procura atualização ao abrir, só no app instalado (no navegador e no `yarn dev` não há updater).
if (!import.meta.env.DEV && "__TAURI_INTERNALS__" in window) {
  void useUpdateStore.getState().checkForUpdate();
}
