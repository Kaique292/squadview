# 0016. Servidores com salas fixas, guardados no Worker

- **Status:** proposta
- **Data:** 2026-10-21

## Contexto

Hoje uma sala só existe enquanto alguém está nela: quando todos saem, some o nome, e o app não
lembra nenhuma lista. O Kaique quer que cada pessoa possa criar **servidores**, com nome e ícone,
e **salas** dentro deles que continuam existindo, como no Discord. O dono e os administradores
mexem no servidor (salas, nome, ícone, membros); quem entra por convite só usa as salas.

## Decisão

Cada servidor é um Durable Object `Server` no Worker (`idFromName(id)`, id de 16 hex), com nome,
ícone, membros com papel (`owner`, `admin`, `member`), salas e convites guardados no KV do
SQLite. As regras ficam num módulo puro (`server-state.ts`). O app conversa com o servidor pela
conexão de amigos (`/me`), que já tem a identidade conferida pela sessão (ADR 0011): o registro
`User` da pessoa repassa o pedido ao `Server` e guarda a lista de servidores dela; o `Server`
avisa o registro de cada membro quando algo muda.

Cada sala do servidor tem um **código fixo** de sala comum (6 caracteres), sorteado ao criar.
Entrar na sala é entrar nesse código, como hoje: a voz, o vídeo e a malha não mudam.

## Consequências

- Servidores exigem entrar com o Discord (a sessão é quem diz quem é o dono e quem é membro).
- Nada de mídia passa pelo servidor; ele só guarda texto pequeno e o ícone (96 px, até 12 KB em
  base64, comprimido pelo app para caber no limite de 16 KB por mensagem).
- Limites: 100 membros por servidor, 20 salas por servidor, 20 servidores por pessoa, 20
  convites ativos por servidor, convites valem 7 dias.
- Convite é `<id do servidor>.<segredo>` (link `squadview://servidor/...`): sem o segredo
  ninguém entra, e o dono pode apagar o servidor (os convites morrem junto).
- O código de uma sala do servidor funciona como qualquer código de sala: quem souber entra,
  mesmo sem ser membro. O código não aparece para quem não é membro; restringir a entrada na
  própria sala fica para depois (exigiria o `Room` perguntar ao `Server`).
- Mensagens novas na conexão `/me` (`server-*`, `room-*`, `member-*`): o Worker antigo fecha a
  conexão de amigos ao receber uma mensagem que não conhece, então o **Worker precisa ser
  publicado antes** do app que usa servidores.
- Avisar os membros a cada mudança custa uma chamada por membro (até 100), só quando alguém
  mexe no servidor, não a cada mensagem de sala.

## Alternativas consideradas

- **Guardar os servidores só no PC de cada um:** não tem como o convite, o dono e as salas
  ficarem iguais para todos sem um lugar central.
- **Servidor dentro do registro do dono (`User`):** o servidor sumiria junto com a conta do dono
  e todo mundo dependeria de um único registro; um `Server` próprio deixa os papéis claros.
- **Uma sala de verdade (`Room`) por sala do servidor, guardando o servidor:** a sala esquece
  tudo quando esvazia de propósito (privacidade e custo); misturar as duas coisas complicaria a
  lógica que já está testada.
