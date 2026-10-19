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

| Mensagem                                                    | Quando                                                                                                                                                                                                              |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `{"type":"join","name":"Ana","protocol":1,"room":"Galera"}` | Primeira mensagem. `name`: 1 a 32 caracteres, sem caracteres de controle (espaços nas pontas são removidos). `room` (opcional, mesmas regras): nome da sala, que só vale para quem entra na sala vazia (quem cria). |
| `{"type":"room","name":"Squad"}`                            | Renomeia a sala (qualquer membro). Todos recebem `room-updated`.                                                                                                                                                    |
| `{"type":"rename","name":"K.A"}`                            | Troca o próprio apelido. Os outros recebem `member-updated`.                                                                                                                                                        |
| `{"type":"status","live":true}`                             | Começou ou parou de transmitir.                                                                                                                                                                                     |
| `{"type":"signal","to":"<peerId>","payload":{...}}`         | Mensagem para um membro. `payload` precisa ser um objeto JSON de até 8 KB.                                                                                                                                          |
| `{"type":"leave"}`                                          | Saída limpa. O servidor fecha a conexão com o código 1000.                                                                                                                                                          |

### Servidor → cliente

| Mensagem                                                                               | Quando                                                                                                        |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `{"type":"welcome","you":"<peerId>","members":[{"id","name","live"}],"room":"Galera"}` | Resposta ao `join`. `members` são os **outros** membros. `room`: nome da sala, ou `null` se ela não tem nome. |
| `{"type":"room-updated","name":"Squad"}`                                               | Alguém renomeou a sala (vai para todos, inclusive quem renomeou).                                             |
| `{"type":"member-joined","member":{...}}`                                              | Alguém entrou (não é enviado para quem entrou).                                                               |
| `{"type":"member-updated","member":{...}}`                                             | Alguém mudou o `live` ou o apelido.                                                                           |
| `{"type":"member-left","id":"<peerId>"}`                                               | Alguém saiu, caiu ou foi desconectado por erro.                                                               |
| `{"type":"signal","from":"<peerId>","payload":{...}}`                                  | Mensagem de outro membro.                                                                                     |
| `{"type":"error","code":"..."}`                                                        | Ver abaixo.                                                                                                   |

`peerId` são 16 caracteres hexadecimais aleatórios, gerados pelo servidor a cada `join`.

O nome da sala fica guardado nas conexões de quem está nela: quando o último membro sai, a sala esquece o nome. `room`, `rename` e `room-updated` entraram depois da versão 0.1.0 do app, sem mudar a versão do protocolo: apps antigos não mandam `room` e ignoram `room-updated`.

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
