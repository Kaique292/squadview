// Sobe o Worker de sinalização local (wrangler dev), roda um comando com
// SQUADVIEW_SIGNALING_URL apontando para ele e derruba o Worker no fim.
//
// Os testes de integração da sala e dos amigos (crates/*/tests/*_live.rs) só rodam com essa
// variável definida; assim o `yarn test` e a esteira testam a sala de verdade, ponta a ponta.
//
// Uso: node scripts/with-worker.mjs <comando> [argumentos...]
import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const PORT = Number(process.env.SQUADVIEW_TEST_WORKER_PORT ?? 8799);
/** Segredo das sessões só para os testes (o de produção fica no Cloudflare). */
export const TEST_SESSION_SECRET = "segredo-dos-testes-de-integracao";

const [command, ...args] = process.argv.slice(2);
if (!command) {
  console.error("uso: node scripts/with-worker.mjs <comando> [argumentos...]");
  process.exit(2);
}

const signaling = join(process.cwd(), "services/signaling");
const wrangler = join(
  dirname(createRequire(join(signaling, "package.json")).resolve("wrangler/package.json")),
  "bin/wrangler.js",
);

const worker = spawn(
  process.execPath,
  [wrangler, "dev", "--port", String(PORT), "--ip", "127.0.0.1", "--var", `SESSION_SECRET:${TEST_SESSION_SECRET}`],
  { cwd: signaling, stdio: ["ignore", "pipe", "pipe"] },
);
let workerLog = "";
worker.stdout.on("data", (d) => (workerLog += d));
worker.stderr.on("data", (d) => (workerLog += d));

function stopWorker() {
  if (worker.exitCode !== null) return;
  // No Windows o wrangler abre processos filhos (workerd): derruba a árvore toda.
  if (process.platform === "win32")
    spawnSync("taskkill", ["/pid", String(worker.pid), "/T", "/F"], { stdio: "ignore" });
  else worker.kill("SIGTERM");
}
process.on("exit", stopWorker);
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => process.exit(130));

// Espera o Worker responder (o primeiro `wrangler dev` baixa e compila coisas).
const deadline = Date.now() + 120_000;
for (;;) {
  try {
    const r = await fetch(`http://127.0.0.1:${PORT}/rooms`, { method: "POST" });
    if (r.status === 201) break;
  } catch {
    // Ainda subindo.
  }
  if (worker.exitCode !== null || Date.now() > deadline) {
    console.error("O Worker de sinalização não subiu:\n" + workerLog);
    process.exit(1);
  }
  await new Promise((r) => setTimeout(r, 500));
}
console.log(`Worker de sinalização local em ws://127.0.0.1:${PORT}`);

const result = spawnSync(command, args, {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: {
    ...process.env,
    SQUADVIEW_SIGNALING_URL: `ws://127.0.0.1:${PORT}`,
    SQUADVIEW_TEST_SESSION_SECRET: TEST_SESSION_SECRET,
  },
});
stopWorker();
process.exit(result.status ?? 1);
