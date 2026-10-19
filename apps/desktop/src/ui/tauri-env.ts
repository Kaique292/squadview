/** Fora do Tauri (testes, navegador) não há janela nativa para controlar. */
export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
