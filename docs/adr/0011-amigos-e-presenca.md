# 0011. Amigos, presença e pedido para entrar na sala

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

Hoje só se entra numa sala com o código ou o link. A galera quer ver os amigos no app, saber quem está online ou numa sala e pedir para entrar, como no Discord. Para isso o serviço precisa saber **quem é quem** de verdade: o nome e a foto que vão na sala ([0009](0009-login-com-discord.md)) são só o que o app diz, e qualquer um poderia se passar por outra pessoa.

O Discord não deixa apps comuns lerem a lista de amigos (o escopo `relationships.read` é só para parceiros) nem buscar usuários pelo nome. Então a lista de amigos tem de ser do SquadView.

## Decisão

**Identidade conferida pelo Worker.**

- No login, o app manda o token do Discord (escopo `identify`) **uma vez** para `POST /auth/discord`. O Worker lê `/users/@me` no Discord e devolve uma **sessão do SquadView**: `{id, exp}` assinado com HMAC-SHA256 (segredo `SESSION_SECRET` do Worker), válida por 90 dias e renovada a cada conexão.
- O token do Discord continua descartado logo depois, como na 0009. A sessão do SquadView só serve para o SquadView.
- O app guarda a sessão em `session.json`, na pasta de dados do app. Quem entrou antes desta versão precisa entrar de novo para usar os amigos.

**Um Durable Object por pessoa (`User`), chaveado pelo id do Discord.**

- O app abre um WebSocket em `/me` e manda a sessão como primeira mensagem (nunca na URL).
- Ali ficam guardados:
  - amigos, com a opção "entra direto" de cada um;
  - pedidos de amizade enviados e recebidos;
  - recentes (até 30);
  - a sala atual. O código só fica no servidor: os amigos veem o nome da sala, não o código.
- Os `User` conversam entre si por RPC: presença, pedidos e convites.

**Recentes e amizade.**

- Na sala, quem entra com a sessão vale como **conferido**. O id do Discord do membro passa a ser o da sessão, nunca o que o app diz.
- A sala avisa o `User` de cada membro conferido sobre os outros conferidos. Assim nascem os recentes.
- Só dá para pedir amizade a alguém dos recentes, para não virar spam por id.

**Presença.**

- Os amigos veem: offline, online, na sala "Nome" ou transmitindo.
- A presença muda na hora, enquanto houver um app conectado.

**Entrar na sala de um amigo.**

- O pedido vai do `User` do amigo para a sala, e **qualquer membro** pode aceitar ou recusar.
- Se o amigo marcou você como "entra direto", você entra sem pedir.
- O código da sala só é entregue a quem foi aceito. O pedido expira em 60 s.

**Chamar para a sala.**

- Um membro convida um amigo, que recebe o convite já com o código.

O protocolo da sala continua na versão 1. `join` ganha o campo opcional `session`, e a sala ganha `join-request` e `join-answer`. Apps antigos seguem funcionando, sem os amigos.

## Consequências

- Ninguém consegue fingir ser outra pessoa nos amigos nem nos recentes.
- O Worker passa a guardar dados: ids do Discord, nomes, hashes de foto e a lista de amigos. Ainda é pouco e é o necessário. Nada de mídia passa por ele.
- Deploy novo exige configurar o segredo: `wrangler secret put SESSION_SECRET`. Sem ele, o login de amigos falha com mensagem clara e as salas continuam funcionando.
- A sessão é um token de portador guardado em arquivo. Quem copiar o arquivo age como a pessoa **só no SquadView**, e por até 90 dias. Sair da conta apaga o arquivo.
- O "entra direto" vale só para o amigo por quem você pediu, não para todos da sala. Isso fica mais simples de entender e de conferir.

## Alternativas consideradas

- **Guardar o token do Discord no app e o Worker conferir a cada conexão:** o token do Discord vale mais que o nosso e exigiria renovação com refresh token. Descartado.
- **Sem conferir identidade:** qualquer um poderia aparecer como seu amigo. Descartado.
- **Lista de amigos do próprio Discord:** o escopo não é liberado para apps comuns.
- **Banco D1 ou KV em vez de um Durable Object por pessoa:** a presença precisa de WebSocket e de avisar na hora. O Durable Object já faz as duas coisas com hibernação, como as salas.
