# Protocolo de sinalização (versão 1)

A sinalização só ajuda os membros de uma sala a se encontrarem: lista quem está na sala, avisa quem está ao vivo e repassa mensagens `signal` (candidatos de endereço para o hole punching e fingerprints dos certificados QUIC). **Nenhum vídeo ou áudio passa por aqui.** O conteúdo de `signal` é definido pelo app (`sv-net`) e repassado sem ser interpretado.

O código que implementa este contrato fica em `src/protocol.ts` (tipos e validação) e `src/room-state.ts` (regras da sala).

## HTTP

| Requisição                                   | Resposta                                                                                      |
| -------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `POST /rooms`                                | `201 {"code":"B8RJ2Q"}`: um código novo de 6 caracteres, sem caracteres ambíguos (0/O, 1/I/L) |
| `GET /rooms/{CODE}` com `Upgrade: websocket` | `101`: conexão WebSocket com a sala                                                           |
| `GET /rooms/{CODE}` sem upgrade              | `426`                                                                                         |
| Código inválido ou qualquer outra rota       | `404`                                                                                         |

A sala passa a existir na primeira conexão; não é preciso criá-la antes com o `POST`, que só sorteia um código.

## Mensagens

Todas as mensagens são texto JSON. Mensagens binárias são recusadas.

### Cliente → servidor

| Mensagem                                                       | Quando                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `{"type":"join","name":"Ana","protocol":1,"room":"Galera"}`    | Primeira mensagem. `name`: 1 a 32 caracteres, sem caracteres de controle (espaços nas pontas são removidos). `room` (opcional, mesmas regras): nome da sala, que só vale para quem entra na sala vazia (quem cria).                                                                                                                                                                                                                                                                                   |
| `{"type":"room","name":"Squad"}`                               | Renomeia a sala (qualquer membro). Todos recebem `room-updated`.                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `{"type":"rename","name":"K.A","discord":{...}}`               | Troca o próprio apelido e, se vier `discord`, a conta do Discord (`null`: desvinculou). Os outros recebem `member-updated`.                                                                                                                                                                                                                                                                                                                                                                           |
| `{"type":"status","live":true,"muted":false,"deafened":false}` | Começou ou parou de transmitir, silenciou o microfone ou ensurdeceu (chat de voz). `muted` e `deafened` são opcionais (falsos se faltarem). `watching` (opcional, `null` se faltar) é o id de quem estamos assistindo: quem transmite vê quantos e quem assistem. `effect` (opcional) é o efeito de fala escolhido, `{"kind":"dots"\|"waves"\|"equalizer"\|"rays","color":"#rrggbb"}`; ausente, nulo, fora do formato ou desconhecido vira `null` (o anel padrão), sem derrubar a conexão (ADR 0015). |
| `{"type":"signal","to":"<peerId>","payload":{...}}`            | Mensagem para um membro. `payload` precisa ser um objeto JSON de até 8 KB.                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `{"type":"leave"}`                                             | Saída limpa. O servidor fecha a conexão com o código 1000.                                                                                                                                                                                                                                                                                                                                                                                                                                            |

### Servidor → cliente

| Mensagem                                                                                                  | Quando                                                                                                        |
| --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `{"type":"welcome","you":"<peerId>","members":[{"id","name","live","muted","deafened"}],"room":"Galera"}` | Resposta ao `join`. `members` são os **outros** membros. `room`: nome da sala, ou `null` se ela não tem nome. |
| `{"type":"room-updated","name":"Squad"}`                                                                  | Alguém renomeou a sala (vai para todos, inclusive quem renomeou).                                             |
| `{"type":"member-joined","member":{...}}`                                                                 | Alguém entrou (não é enviado para quem entrou).                                                               |
| `{"type":"member-updated","member":{...}}`                                                                | Alguém mudou o `live`, o apelido ou o estado de voz.                                                          |
| `{"type":"member-left","id":"<peerId>"}`                                                                  | Alguém saiu, caiu ou foi desconectado por erro.                                                               |
| `{"type":"signal","from":"<peerId>","payload":{...}}`                                                     | Mensagem de outro membro.                                                                                     |
| `{"type":"error","code":"..."}`                                                                           | Ver abaixo.                                                                                                   |

`peerId` são 16 caracteres hexadecimais aleatórios, gerados pelo servidor a cada `join`.

O nome da sala fica guardado nas conexões de quem está nela: quando o último membro sai, a sala esquece o nome. `room`, `rename`, `room-updated` e os campos `muted`/`deafened` entraram depois da versão 0.1.0 do app, sem mudar a versão do protocolo: apps antigos não mandam `room` e ignoram `room-updated`.

### Conta do Discord

`join` e `rename` aceitam o campo opcional `discord`: `{"id":"123456789012345678","avatar":"<hash>"}`. `id` é o id numérico da conta (15 a 21 dígitos) e `avatar` é o hash da foto (32 caracteres hexadecimais, com `a_` na frente se for animada) ou `null` para a foto padrão. Todo membro sai com `discord` (a conta, ou `null`). Só vai o id e o hash, nunca uma URL: o app monta o endereço do CDN do Discord, então ninguém consegue fazer os outros abrirem um link qualquer. O servidor não confere se a conta é mesmo de quem mandou (como o nome): serve para mostrar a foto, não como identidade. O campo entrou na 0.3.0, sem mudar a versão do protocolo: apps antigos não mandam e ignoram.

### Amigos na sala

Desde a 0.3.0 ([ADR 0011](../../docs/adr/0011-amigos-e-presenca.md)):

- `join` aceita `"session":"<sessão>"`. Se a sessão for válida, o membro conta como **conferido**: a conta do Discord dele passa a ser a da sessão, e ele não consegue trocá-la pela de outra pessoa no `rename`.
- Todo membro sai com `"verified": true|false` (a conta foi conferida pela sessão). Só conferidos podem virar amigos.
- `{"type":"verify","session":"<sessão>"}` (cliente → servidor): quem entrou com o Discord **já dentro da sala** confere a sessão sem sair e entrar. Os outros recebem `member-updated` com `verified: true`. Sessão que não confere é ignorada.
- Membros conferidos na mesma sala entram nos recentes um do outro.
- `{"type":"join-request","id":"<id>","from":{"id","name","avatar"}}` (servidor → todos): um amigo de alguém da sala quer entrar.
- `{"type":"join-answer","id":"<id>","accept":true}` (cliente → servidor): qualquer membro aceita ou recusa. O primeiro a responder decide.
- `{"type":"join-request-closed","id":"<id>"}` (servidor → todos): o pedido foi respondido ou venceu (60 s).
- Só quem foi aceito recebe o código da sala, pelo próprio registro.

### Repassador TURN

Desde a 0.5.0 ([ADR 0014](../../docs/adr/0014-conexao-em-camadas.md)), para quem não conecta direto (CGNAT):

- `{"type":"turn","urls":["turn:turn.cloudflare.com:3478?transport=udp",...],"username":"...","credential":"..."}` (servidor → quem acabou de entrar): credenciais do repassador TURN. Só vai se o Worker tiver um provedor configurado (Cloudflare, Metered ou servidor fixo; ver `wrangler.toml`). Apps antigos ignoram.
- Sinal `relay-request` (entre apps, pelo `signal`): quem não alcança quem transmite pede a um membro que já assiste para repassar. A resposta é um `watch-accept` ou `watch-deny` normal.
- Nos sinais, `watch-accept` e `voice-answer` ganham o campo opcional `relay` (`"ip:porta"`): o endereço que quem aceita pegou emprestado no repassador. Quem conecta tenta os `candidates` e, se não fechar em 1,5 s, também o `relay`. O Worker não olha os sinais; o campo é só entre os apps.

## Amigos e presença

| Requisição                                                            | Resposta                                                                                          |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `POST /auth/discord` com `{"accessToken":"..."}`                      | `200 {"session","profile"}`, ou `401` se o Discord recusar o token, ou `503` sem `SESSION_SECRET` |
| `GET /me` com `Upgrade: websocket` e `Authorization: Bearer <sessão>` | `101`: conexão com o próprio registro (amigos e presença), ou `401` com sessão inválida           |

O token do Discord é usado uma vez (para ler `/users/@me`) e não é guardado. A sessão é `{id, exp}` assinado com HMAC-SHA256, vale 90 dias e o registro manda uma nova (`{"type":"session"}`) a cada conexão.

App → registro:

| Mensagem                                                                              | O que faz                                                               |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `{"type":"profile","name":"K.A","avatar":"<hash>"}`                                   | Nome e foto que os amigos veem                                          |
| `{"type":"status","room":{"code":"B8RJ2Q","name":"Galera"},"live":false}`             | Em que sala está (`room: null` fora de sala). O código fica no servidor |
| `{"type":"friend-request","id":"<discord id>"}`                                       | Pede amizade (só a quem está nos recentes)                              |
| `{"type":"friend-accept"}` / `friend-decline` / `friend-cancel` / `unfriend` com `id` | Responde, cancela ou desfaz                                             |
| `{"type":"set-direct","id":"...","direct":true}`                                      | Esse amigo entra direto nas minhas salas                                |
| `{"type":"join","id":"..."}`                                                          | Quero entrar na sala desse amigo                                        |
| `{"type":"invite","id":"..."}`                                                        | Chama esse amigo para a minha sala                                      |

Registro → app:

| Mensagem                                                                 | Quando                                                                                                      |
| ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `{"type":"state","me","friends","incoming","outgoing","recents"}`        | Ao conectar e a cada mudança. Cada amigo vem com `direct` e `presence` (`online`, `inRoom`, `room`, `live`) |
| `{"type":"friend-request","from":{...}}`                                 | Chegou um pedido de amizade                                                                                 |
| `{"type":"join-result","result":{"kind":"approved","code","room","by"}}` | Resposta ao `join`: também `pending`, `denied`, `not-in-room` ou `not-friend`                               |
| `{"type":"invited","from":{...},"code":"B8RJ2Q","room":"Galera"}`        | Um amigo chamou para a sala                                                                                 |
| `{"type":"session","session":"..."}`                                     | Sessão renovada                                                                                             |
| `{"type":"error","code":"..."}`                                          | `invalid-message` (fecha, 4400), `rate-limited` (fecha, 4429), `unknown-person`, `too-many`, `not-in-room`  |

### Servidores (ADR 0016)

Pela mesma conexão `/me`. O registro confere quem pede e repassa ao Durable Object `Server`, que avisa o registro dos outros membros. Limites: 20 servidores por pessoa, 100 membros e 20 salas por servidor, 20 convites ativos (valem 7 dias), ícone em data URL `image/webp|png|jpeg` até 12 KB.

App → registro:

| Mensagem                                                       | O que faz                                                   |
| -------------------------------------------------------------- | ----------------------------------------------------------- |
| `{"type":"server-create","name":"Turma"}`                      | Cria o servidor (quem cria é o dono), já com a sala "Geral" |
| `{"type":"server-join","invite":"<id 16 hex>.<segredo 12>"}`   | Entra pelo convite                                          |
| `{"type":"server-action","server":"<id>","op":{"kind":"..."}}` | Pedido a um servidor nosso (abaixo)                         |

`op`: `update` (`name` e/ou `icon`, `null` tira o ícone), `delete` (só o dono), `leave` (o dono não sai), `invite` (qualquer membro), `room-create` (`name`), `room-rename` (`room`, `name`), `room-delete` (`room`; a última sala fica), `kick` (`member`: administrador tira membros, só o dono tira administradores) e `role` (`member`, `role`: `admin` ou `member`, só o dono). Mexer no servidor (nome, ícone, salas) é do dono e dos administradores.

Registro → app:

| Mensagem                                                            | Quando                                                                                                                           |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `{"type":"servers","servers":[...]}`                                | Ao conectar: todos os nossos (`id`, `name`, `icon`, `rooms`, `members`)                                                          |
| `{"type":"server","server":{...}}`                                  | Entramos, criamos ou alguém mudou um servidor nosso                                                                              |
| `{"type":"server-removed","id":"..."}`                              | Saímos, fomos tirados ou o servidor foi apagado                                                                                  |
| `{"type":"server-invite","server":"...","invite":"<id>.<segredo>"}` | Convite criado (o app monta o link `.../convite/?servidor=<convite>`, que abre `squadview://servidor/<convite>`)                 |
| `{"type":"error","code":"..."}`                                     | `not-member`, `not-allowed`, `unknown-room`, `unknown-member`, `invite-invalid`, `server-full`, `too-many`, `owner-cannot-leave` |

O Worker antigo não conhece essas mensagens e fecha a conexão de amigos (4400): publique o Worker antes do app.

### Mensagens privadas (ADR 0017)

O conteúdo das conversas vai direto entre os dois PCs (QUIC); o registro só repassa o sinal para eles se conectarem e não guarda nada.

| Direção        | Mensagem                                                           | O que faz                                                                                 |
| -------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| App → registro | `{"type":"dm-signal","to":"<discord id>","payload":{...}}`         | Repassa o sinal ao amigo (`payload` objeto, até 8 KB). Só entre amigos (`unknown-person`) |
| Registro → app | `{"type":"dm-signal","from":"<discord id>","payload":{...}}`       | Chegou o sinal de um amigo, em todas as nossas conexões                                   |
| Registro → app | `{"type":"dm-unreachable","id":"<discord id>"}`                    | O amigo não está online: a mensagem espera no nosso PC                                    |
| App → registro | `{"type":"turn-request"}`                                          | Pede credenciais do repassador TURN para a conversa direta                                |
| Registro → app | `{"type":"turn","urls":[...],"username":"...","credential":"..."}` | As credenciais (nada vem sem provedor configurado)                                        |

Também precisam do Worker publicado antes do app.

## Erros

| `code`             | Causa                                                                             | Fecha a conexão? |
| ------------------ | --------------------------------------------------------------------------------- | ---------------- |
| `invalid-message`  | JSON fora do contrato, mensagem binária, mensagem bruta > 16 KB ou segundo `join` | Sim (4400)       |
| `version-mismatch` | `protocol` diferente de 1                                                         | Sim (4426)       |
| `room-full`        | A sala já tem 8 membros                                                           | Sim (4409)       |
| `rate-limited`     | Mais de 100 mensagens em 10 s                                                     | Sim (4429)       |
| `not-joined`       | Mensagem antes do `join`                                                          | Não              |
| `unknown-peer`     | `signal` para alguém que não está na sala (ou para si mesmo)                      | Não              |

Quem é desconectado por erro sai da sala para os outros (`member-left`).

## Exemplo

```
Ana  → {"type":"join","name":"Ana","protocol":1}
Ana  ← {"type":"welcome","you":"e861a48efd157840","members":[]}
Beto → {"type":"join","name":"Beto","protocol":1}
Beto ← {"type":"welcome","you":"883915c4429296fb","members":[{"id":"e861a48efd157840","name":"Ana","live":false}]}
Ana  ← {"type":"member-joined","member":{"id":"883915c4429296fb","name":"Beto","live":false}}
Ana  → {"type":"status","live":true}
Beto ← {"type":"member-updated","member":{"id":"e861a48efd157840","name":"Ana","live":true}}
Beto → {"type":"signal","to":"e861a48efd157840","payload":{"candidates":["203.0.113.5:40000"]}}
Ana  ← {"type":"signal","from":"883915c4429296fb","payload":{"candidates":["203.0.113.5:40000"]}}
```

## Testar localmente

```powershell
yarn workspace @squadview/signaling dev   # Worker e Durable Object locais em http://127.0.0.1:8787, sem conta
yarn workspace @squadview/signaling test  # testes unitários com cobertura
```

Os testes do Durable Object com `@cloudflare/vitest-pool-workers` ficaram de fora: a versão atual (0.23) exige Vitest 4, e o projeto usa Vitest 5. Por isso a lógica fica em módulos puros com 100% de cobertura. Os testes simulam o adaptador real, que cria um objeto de conexão novo a cada chamada. O `index.ts` só liga esses módulos ao runtime e foi conferido ponta a ponta com `wrangler dev`.
