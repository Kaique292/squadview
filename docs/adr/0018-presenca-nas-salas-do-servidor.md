# 0018. Presença nas salas do servidor e entrada direta para membros

- **Status:** aceita
- **Data:** 2026-10-21

## Contexto

Na ADR 0016, cada sala de servidor é um código de sala comum, e só quem está **dentro** da sala sabe
quem está nela. Por isso, ao abrir o app, as salas do servidor pareciam vazias mesmo com amigos
dentro, e o único jeito visível de chegar até eles era o "Entrar" da página de Amigos. Esse
caminho sempre pede permissão a quem está na sala, mesmo quando quem pede é o dono do servidor:
o registro não sabia que aquela sala é de um servidor dos dois.

## Decisão

O registro de cada pessoa (`User`) guarda quais códigos de sala são de servidores dela (vê as
salas em cada retrato de servidor que recebe) e, quando o `status` do app muda de sala, avisa o
`Server` dessa sala: quem entrou ou saiu, e se está ao vivo. O `Server` guarda quem está em cada
sala e manda a todos os membros a mensagem nova `server-presence` (só ids, sala e ao vivo: nome e
foto o app já tem na lista de membros). E quando um amigo pede para entrar e a nossa sala é de um
servidor do qual ele também é membro, o registro aprova na hora, sem pedir a ninguém.

## Consequências

- A lateral do servidor mostra quem está em cada sala, mesmo para quem está fora delas, e quem
  está ao vivo. Clicar na sala continua entrando direto (o código já era o da sala).
- Membro de um servidor entra numa sala dele sem pedir permissão, também pelo "Entrar" dos amigos.
- Só membros aparecem: quem entrou numa sala do servidor só com o código não é membro e não
  aparece para os outros (a ADR 0016 já aceita que o código funciona para quem o tiver).
- O registro guarda um índice `código → servidor` atualizado a cada retrato de servidor; uma sala
  criada há pouco só entra no índice quando o retrato novo chega (o mesmo aviso que já existe).
- Custo: uma chamada ao `Server` quando alguém entra ou sai de uma sala dele (não a cada mensagem),
  e um aviso para cada membro (até 100), como as outras mudanças de servidor. Mudar de "ao vivo"
  também avisa, para o selo aparecer.
- A presença vive no armazenamento do `Server` e é apagada quando o registro avisa que a pessoa
  saiu da sala ou fechou o app. Se o registro cair sem avisar (raro: o Durable Object fecha as
  conexões), a pessoa fica aparecendo até a próxima mudança dela.
- Mensagem nova `server-presence` na conexão `/me`, só do Worker para o app: o app antigo ignora
  mensagens que não conhece, então não precisa publicar o Worker antes do app desta vez, mas o
  recurso só funciona com os dois atualizados.
- Privacidade: a presença de amigos continua sem o código da sala (ADR 0011); dentro de um
  servidor, os membros já veem os códigos das salas, então ver quem está em cada uma não revela
  nada novo para quem é de fora.

## Alternativas consideradas

- **A sala (`Room`) avisar o `Server` a cada entrada e saída:** a sala não sabe de qual servidor
  ela é; teria que guardar isso, e ela esquece tudo quando esvazia (ADR 0016).
- **Mandar o código da sala na presença dos amigos:** só cobriria amigos (não todos os membros) e
  desfaria a decisão de privacidade da ADR 0011.
- **Pôr quem está nas salas dentro do retrato do servidor:** o retrato leva o ícone (até 12 KB) e
  seria reenviado inteiro a cada entrada e saída; a mensagem separada é pequena.
