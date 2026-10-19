---
name: release
description: Conduz uma release do SquadView (dev → main, PR do release-please, conferência da Release). Use quando o Kaique quiser publicar uma versão.
---

# Release

Cada passo que publica algo (push, PR, merge) precisa do OK do Kaique.

1. **Pré-checagem:** `gh run list --branch dev --limit 3`. A última verificação da `dev` tem que estar verde.
2. **Prévia do changelog:** liste os commits que vão entrar: `git log --oneline origin/main..origin/dev`. Avise se houver `feat`/`fix` com resumo ruim para o changelog.
3. **PR dev → main:** `gh pr create --base main --head dev --title "chore(release): integra dev na main"`, com a lista de mudanças no corpo.
4. Depois do merge (**merge commit**, nunca squash), o release-please abre o PR `chore(release): versão X.Y.Z`. Revise com o Kaique: a versão está certa (`feat` → minor em 0.x, `fix` → patch)? O changelog lê bem?
5. Após o merge do PR de release, acompanhe o workflow **Release** e confira na GitHub Release:
   - instalador `SquadView_X.Y.Z_x64-setup.exe`;
   - `SHA256SUMS.txt`;
   - notas iguais à seção do CHANGELOG.
6. Atualize o status da fase no `docs/roadmap.md` se a release fechar uma fase (skill `fase`).
