# 0009. Login com o Discord por OAuth2 com PKCE, sem segredo no app

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

A galera do SquadView já está no Discord. Entrar com o perfil de lá deixa nome e foto iguais nas salas e prepara o convite e o Rich Presence. O app é open source e roda no PC de cada um, então não pode carregar um _client secret_: qualquer pessoa leria o segredo no código ou no binário.

## Decisão

- O app no Discord Developer Portal é um **Public Client**. O login usa o OAuth2 _authorization code_ com **PKCE (S256)** e escopo `identify`, só o perfil público.
- O Rust abre o navegador na página de autorização do Discord e espera o retorno num servidor HTTP local, preso em `127.0.0.1:53134` (o _redirect_ cadastrado é `http://127.0.0.1:53134/callback`). O `state` aleatório é conferido, cada pedido é limitado a 8 KB e o login tem prazo de 3 minutos.
- O código é trocado por um token, o app lê `/users/@me` e **descarta o token**. Só o perfil (id, nome, apelido e hash da foto) fica salvo em `discord.json`, na pasta de dados do app. As fotos vêm de `cdn.discordapp.com`, liberado no CSP.
- O HTTPS usa `reqwest` com rustls no provedor `ring`, o mesmo do resto do app.

## Consequências

- Nenhum segredo no repositório nem no instalador.
- O app não guarda token, então não consegue agir em nome da pessoa. Para ver o perfil atualizado, ela entra de novo.
- Os outros membros da sala ainda veem só a inicial. Mandar a foto pela sinalização é uma mudança no protocolo e terá ADR própria.
- A porta 53134 precisa estar livre durante o login. Se estiver ocupada, o app avisa.

## Alternativas consideradas

- **Client secret no app:** vazaria. Descartado.
- **Servidor próprio trocando o código:** exigiria um backend com o segredo; o projeto evita servidores.
- **Ler o perfil pelo cliente do Discord (IPC/RPC local):** depende do Discord aberto e de aprovação extra para os escopos de RPC. Fica para o Rich Presence.
