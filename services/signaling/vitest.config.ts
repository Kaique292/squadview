import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      // index.ts só liga o runtime da Cloudflare; ganha testes com vitest-pool-workers na Fase 2.
      exclude: ["src/**/*.test.ts", "src/index.ts"],
      reporter: ["text", "json-summary"],
      reportsDirectory: "../../build/coverage-signaling",
    },
  },
});
