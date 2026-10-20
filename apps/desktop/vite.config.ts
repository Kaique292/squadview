import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import pkg from "./package.json" with { type: "json" };

// Porta fixa: o Tauri abre a janela em http://localhost:1420 durante o `tauri dev`.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Versão do app na interface (o release-please atualiza o package.json).
  define: { "import.meta.env.APP_VERSION": JSON.stringify(pkg.version) },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "chrome120",
    // Duas páginas: o app e a splash de abertura.
    rollupOptions: { input: { main: "index.html", splash: "splash.html" } },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/main.tsx", "src/dev/**", "src/**/*.test.{ts,tsx}", "src/test-setup.ts"],
      reporter: ["text", "json-summary"],
      reportsDirectory: "../../build/coverage-desktop",
      // Piso da cobertura (o que já temos, arredondado para baixo): a esteira falha se cair.
      thresholds: { lines: 99, statements: 97, functions: 95, branches: 94 },
    },
  },
});
