// Commits semânticos do SquadView: https://www.conventionalcommits.org/pt-br/
// Regras completas em docs/git-e-versionamento.md.

/** O projeto tem um único autor: nenhum commit pode trazer coautoria. */
const noCoAuthor = ({ raw }) => {
  const found = /^co-authored-by:/im.test(raw ?? "");
  return [!found, "o commit não pode ter trailer Co-authored-by (o projeto tem um único autor)"];
};

module.exports = {
  extends: ["@commitlint/config-conventional"],
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
