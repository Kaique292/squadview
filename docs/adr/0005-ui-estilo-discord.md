# 0005. Interface com a hierarquia visual do Discord

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

Quem vai usar o SquadView já passa horas no Discord. Uma interface familiar dispensa tutorial.

## Decisão

Seguir a **hierarquia** do Discord (barra de salas à esquerda, barra lateral com membros, área principal com a grade de streams, painel do usuário embaixo, tema escuro em camadas) com **identidade própria**: paleta, logo e componentes do SquadView. Nenhum asset, ícone ou marca do Discord é copiado.

## Consequências

- Curva de aprendizado quase zero.
- Os tokens em `docs/design/ui.md` e `apps/desktop/src/styles/index.css` são a fonte da verdade do visual.

## Alternativas consideradas

- **Visual totalmente novo:** mais trabalho de design e mais estranhamento para o público.
