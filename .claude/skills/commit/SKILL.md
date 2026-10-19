---
name: commit
description: Cria commits semânticos no SquadView, com o escopo certo e sem coautor. Use quando o Kaique pedir para commitar.
---

# Commit semântico

Só commite quando o Kaique pedir explicitamente.

1. Veja o que mudou: `git status` e `git diff` (e `git diff --staged`).
2. Separe em commits lógicos: uma mudança por commit (ex.: código e docs da mesma feature podem ir juntos; uma refatoração não relacionada vai separada).
3. Para cada commit, escolha:
   - **tipo:** `feat`, `fix`, `perf`, `refactor`, `docs`, `build`, `test`, `ci`, `chore` ou `style` (tabela em `docs/git-e-versionamento.md`);
   - **escopo:** `core`, `capture`, `encode`, `media`, `net`, `mesh`, `signal`, `ui`, `app`, `docs`, `ci`, `deps` ou `release`;
   - **resumo:** português, imperativo, minúsculo, sem ponto final, até ~72 caracteres, escrito para quem lê o changelog.
4. Corpo opcional explicando o **porquê**. `BREAKING CHANGE:` no rodapé se quebrar compatibilidade.
5. **Nunca** inclua `Co-authored-by:` nem assinatura de IA. O autor é só o Kaique.
6. Faça `git add` dos arquivos específicos (nunca `git add -A` sem conferir) e `git commit -m "..."`. O hook `commit-msg` roda o commitlint; se recusar, corrija a mensagem, sem usar `--no-verify`.
7. Mostre o `git log --oneline` dos commits criados. Não dê push sem o Kaique pedir.
