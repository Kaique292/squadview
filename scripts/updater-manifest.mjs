// Gera o latest.json que o app instalado consulta para saber se há versão nova.
// Roda na esteira de release, depois do `yarn build:release` (que gera o instalador e o .sig).
// Uso: node scripts/updater-manifest.mjs <tag>   (ex.: v0.2.0)
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const REPO = "Kaique292/squadview";
const BUNDLE_DIR = "target/release/bundle/nsis";

const tag = process.argv[2];
if (!/^v\d+\.\d+\.\d+/.test(tag ?? "")) {
  console.error("informe a tag da release, ex.: node scripts/updater-manifest.mjs v0.2.0");
  process.exit(1);
}
const version = tag.slice(1);

const installer = readdirSync(BUNDLE_DIR).find((f) => f.endsWith("-setup.exe") && f.includes(`_${version}_`));
if (!installer) {
  console.error(`instalador da versão ${version} não encontrado em ${BUNDLE_DIR}`);
  process.exit(1);
}

let signature;
try {
  signature = readFileSync(join(BUNDLE_DIR, `${installer}.sig`), "utf8").trim();
} catch {
  console.error(`assinatura ${installer}.sig não encontrada: o build rodou com TAURI_SIGNING_PRIVATE_KEY?`);
  process.exit(1);
}

// Notas: a seção desta versão no CHANGELOG, sem o título.
const changelog = readFileSync("CHANGELOG.md", "utf8");
const section = changelog.split(/^## /m).find((s) => s.replace(/^\[/, "").startsWith(version));
const notes = section ? section.split("\n").slice(1).join("\n").trim() : "";

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url: `https://github.com/${REPO}/releases/download/${tag}/${encodeURIComponent(installer)}`,
    },
  },
};

writeFileSync(join(BUNDLE_DIR, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`latest.json gerado para ${installer}`);
