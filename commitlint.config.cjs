// Commits semânticos do SquadView: https://www.conventionalcommits.org/pt-br/
// Regras completas em docs/git-e-versionamento.md.

/** Trailer de coautoria de um robô do GitHub (ex.: `dependabot[bot]`). */
const BOT_CO_AUTHOR = /^co-authored-by:\s*[^<]*\[bot\]\s*</i;

/**
 * O projeto tem um único autor: nenhum commit pode trazer coautoria (de pessoa ou de IA).
 * A exceção são robôs do GitHub, que o próprio GitHub acrescenta ao fazer squash de um PR deles.
 */
const noCoAuthor = ({ raw }) => {
  const found = (raw ?? "").split(/\r?\n/).some((line) => /^co-authored-by:/i.test(line) && !BOT_CO_AUTHOR.test(line));
  return [!found, "o commit não pode ter trailer Co-authored-by (o projeto tem um único autor)"];
};

/**
 * Commits que já estão no histórico da `dev` (que não aceita reescrita) com o título acima do
 * limite. A regra continua valendo para todos os outros; não acrescente nada aqui sem motivo.
 */
const KNOWN_LONG_HEADERS = new Set([
  "fix: voz no volume do Discord, grade em foco, atualizar como no Discord e amigos conferidos na sala (#17)",
]);

/**
 * Squashes de PRs do Dependabot que já estão na `dev` com trailers de coautoria do próprio autor do
 * projeto: a branch deles recebeu commits nossos (uma correção, ou a `dev` trazida pelo botão de
 * atualizar), e o GitHub juntou os autores no squash. Para não repetir: não mexa na branch de um PR
 * do Dependabot; para trazer a `dev`, comente `@dependabot rebase`; para corrigir algo, abra outro PR.
 */
const KNOWN_DEPENDABOT_SQUASHES = new Set([
  "build(deps-dev): bump jsdom from 30.1.3 to 31.0.0 (#148)",
  "build(deps): bump the rust group across 1 directory with 6 updates (#154)",
  "ci(deps): bump EmbarkStudios/cargo-deny-action from 2.1.1 to 2.1.3 (#153)",
  "ci(deps): bump taiki-e/install-action from 2.88.1 to 2.90.6 (#152)",
]);

const header = (message) => message.split(/\r?\n/)[0].trim();

module.exports = {
  extends: ["@commitlint/config-conventional"],
  ignores: [(message) => KNOWN_LONG_HEADERS.has(header(message)) || KNOWN_DEPENDABOT_SQUASHES.has(header(message))],
  plugins: [{ rules: { "no-co-author": noCoAuthor } }],
  rules: {
    "no-co-author": [2, "always"],
    "subject-case": [0],
    "body-max-line-length": [1, "always", 120],
    "scope-enum": [
      2,
      "always",
      [
        "core",
        "capture",
        "encode",
        "media",
        "net",
        "mesh",
        "input",
        "signal",
        "ui",
        "app",
        "docs",
        "ci",
        "deps",
        "deps-dev",
        "release",
      ],
    ],
  },
};
