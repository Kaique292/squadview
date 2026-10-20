// Trava de cobertura do Rust: lê o relatório do cargo llvm-cov, mostra a cobertura de linhas por
// crate e falha se o total ficar abaixo do mínimo. (O --fail-under-lines do llvm-cov falha sem dizer
// o número, o que deixa a esteira vermelha sem explicação.)
//
// Uso: node scripts/coverage-check.mjs [mínimo-em-%] [relatório]
import { readFileSync } from "node:fs";

const min = Number(process.argv[2] ?? 70);
const file = process.argv[3] ?? "build/coverage-rust.json";
const data = JSON.parse(readFileSync(file, "utf8")).data[0];

const crates = new Map();
for (const f of data.files) {
  const path = f.filename.replace(/\\/g, "/");
  const match = path.match(/(crates\/[^/]+|apps\/desktop\/src-tauri)\//);
  const name = match ? match[1] : "outros";
  const c = crates.get(name) ?? { covered: 0, count: 0 };
  c.covered += f.summary.lines.covered;
  c.count += f.summary.lines.count;
  crates.set(name, c);
}
for (const [name, c] of [...crates].sort()) {
  console.log(`${((100 * c.covered) / c.count).toFixed(1).padStart(6)}%  ${name}`);
}

const total = data.totals.lines.percent;
console.log(`${total.toFixed(1).padStart(6)}%  TOTAL (mínimo ${min}%)`);
if (total < min) {
  console.error(`::error::Cobertura do Rust em ${total.toFixed(1)}%, abaixo do mínimo de ${min}%.`);
  process.exit(1);
}
