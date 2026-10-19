# 0006. Atualização automática assinada pelas GitHub Releases

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

Quem instala uma versão sem atualizador fica nela para sempre, a menos que reinstale à mão. Num app usado entre amigos, isso deixa a sala com versões misturadas, e membros com protocolos diferentes não conseguem entrar na mesma sala.

## Decisão

Usar o **plugin de atualização do Tauri** desde a primeira versão pública:

- o app consulta `https://github.com/Kaique292/squadview/releases/latest/download/latest.json` ao abrir;
- cada instalador é **assinado** (minisign) com uma chave privada guardada só pelo mantenedor e nos secrets do GitHub; a chave pública fica em `tauri.conf.json`;
- o app só instala pacotes com assinatura válida;
- a interface mostra uma faixa "Nova versão disponível" e o usuário escolhe quando atualizar e reiniciar.

## Consequências

- Todo mundo converge para a versão mais nova sem esforço.
- Sem servidor: o GitHub Releases hospeda o instalador, a assinatura e o `latest.json`.
- **Perder a chave privada ou a senha impede atualizar as instalações existentes.** Ela tem backup fora do repositório.
- Builds locais (`yarn build`) não precisam da chave; só o `yarn build:release` da esteira assina.

## Alternativas consideradas

- **Só avisar e mandar baixar no navegador:** mais simples, mas na prática ninguém atualiza.
- **Microsoft Store:** atualiza sozinha, mas exige conta paga e revisão a cada versão.
