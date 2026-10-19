import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      // index.ts só liga os módulos ao runtime da Cloudflare (ver PROTOCOL.md, "Testar localmente").
      exclude: ["src/**/*.test.ts", "src/index.ts"],
      reporter: ["text", "json-summary"],
      reportsDirectory: "../../build/coverage-signaling",
    },
  },
});
