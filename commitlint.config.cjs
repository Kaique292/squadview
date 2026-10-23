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

module.exports = {
  extends: ["@commitlint/config-conventional"],
  ignores: [(message) => KNOWN_LONG_HEADERS.has(message.split(/\r?\n/)[0].trim())],
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
