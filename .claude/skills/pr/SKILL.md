---
name: pr
description: Abre um pull request do SquadView para a branch dev, com título semântico e o template preenchido. Use quando o Kaique pedir para abrir um PR.
---

# Abrir PR

Só abra o PR quando o Kaique pedir.

1. Confirme a branch atual (`git branch --show-current`). Ela não pode ser `dev` nem `main`.
2. Rode `yarn lint` e `yarn test` (Rust pelo PowerShell) e anote os resultados reais.
3. Confira que nenhum commit tem coautor: `git log --format=%B origin/dev..HEAD | grep -i co-authored` (tem que voltar vazio).
4. Faça push da branch (com a autorização do Kaique) e abra:
   `gh pr create --base dev --title "<tipo>(<escopo>): <resumo>" --body-file <arquivo>`
5. O corpo segue `.github/pull_request_template.md`: o que muda, como testar, como foi medido (se mexeu em mídia ou rede) e o checklist marcado de verdade.
6. O título vira o commit do squash: tem que ser semântico e servir para o changelog.
7. Não inclua assinatura de IA no corpo.
