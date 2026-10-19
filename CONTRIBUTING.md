# Como contribuir

Que bom que você quer ajudar! O SquadView é feito para a galera usar entre amigos, e toda ajuda conta: código, testes em PCs e redes diferentes, design, documentação e ideias.

## Antes de começar

- Procure nas [issues](https://github.com/Kaique292/squadview/issues) se o assunto já existe.
- Para mudanças grandes (arquitetura, protocolo da malha, nova dependência pesada), abra uma issue antes, para conversarmos.
- Leia o [guia de desenvolvimento](docs/desenvolvimento.md) e a [arquitetura](docs/arquitetura.md).

## Passo a passo

1. Faça um fork e clone o repositório.
2. Parta da `dev`: `git switch dev && git switch -c feat/minha-ideia`.
3. `corepack enable && yarn install` (instala os hooks do git).
4. Faça a mudança com testes.
5. Rode `yarn lint` e `yarn test`.
6. Commits no [padrão semântico](docs/git-e-versionamento.md), por exemplo `feat(ui): mostra a camada de qualidade no card do stream`.
7. Abra o PR para a **`dev`** e preencha o template.

## Regras do projeto

- **PRs vão para a `dev`.** A `main` é a produção e só recebe a `dev`.
- **Commits semânticos**, conferidos pelo commitlint no seu PC e no CI.
- **Um autor por commit:** commits com `Co-authored-by:` são recusados.
- **Performance importa:** mudanças em captura, encode, rede ou renderização precisam dizer no PR como foram medidas.
- **Decisões de arquitetura** viram uma [ADR](docs/adr/).
- Docs e mensagens do app em português. Issues e PRs em inglês também são bem-vindos.

## Testando com amigos

Testar em redes diferentes é uma das ajudas mais valiosas. Ao reportar um problema, conte a placa de vídeo, o driver, a internet (download e upload) e quantas pessoas estavam na sala. O template de bug já pede isso.

## Código de conduta

Ao participar, você concorda com o [Código de Conduta](CODE_OF_CONDUCT.md).
