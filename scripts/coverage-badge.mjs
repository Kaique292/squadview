// Junta a cobertura de linhas do Rust (cargo llvm-cov) e do TypeScript (vitest)
// e gera o JSON que o shields.io lê para a badge do README.
// Uso: node scripts/coverage-badge.mjs [arquivo-de-saída]
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const out = process.argv[2] ?? "build/badges/coverage.json";

const sources = {
  rust: () => JSON.parse(readFileSync("build/coverage-rust.json", "utf8")).data[0].totals.lines,
  desktop: () => JSON.parse(readFileSync("build/coverage-desktop/coverage-summary.json", "utf8")).total.lines,
  signaling: () => JSON.parse(readFileSync("build/coverage-signaling/coverage-summary.json", "utf8")).total.lines,
};

let covered = 0;
let total = 0;
const parts = [];
for (const [name, read] of Object.entries(sources)) {
  let lines;
  try {
    lines = read();
  } catch {
    console.error(`cobertura de "${name}" não encontrada: rode "yarn test" antes.`);
    process.exit(1);
  }
  const count = lines.count ?? lines.total;
  covered += lines.covered;
  total += count;
  parts.push(`${name} ${count ? ((lines.covered / count) * 100).toFixed(1) : "0.0"}%`);
}

const pct = total ? Math.floor((covered / total) * 1000) / 10 : 0;
const color = pct >= 90 ? "2EA043" : pct >= 75 ? "97CA00" : pct >= 60 ? "DFB317" : "E5484D";

if (!existsSync(dirname(out))) mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify({ schemaVersion: 1, label: "cobertura", message: `${pct}%`, color }, null, 2) + "\n");
console.log(`cobertura ${pct}% (${parts.join(", ")})`);
