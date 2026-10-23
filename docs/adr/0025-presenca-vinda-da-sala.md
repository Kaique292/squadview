# 0025. Presença das salas do servidor vinda da própria sala

- **Status:** aceita
- **Data:** 2026-10-23

## Contexto

Na ADR 0018, quem avisa o `Server` de que alguém está numa sala dele é o registro (`User`) de cada
**membro**. Por isso, quem entrou numa sala do servidor só com o código ou o link (sem ser membro)
nunca aparecia para quem via a sala de fora: no primeiro teste com a galera, o servidor tinha dois
membros, a sala tinha quatro pessoas, e de fora só aparecia uma. No Discord, todo mundo que está
num canal de voz aparece na lista do canal.

## Decisão

Quem avisa passa a ser a **própria sala**. O `Server` diz a cada sala dele que ela é dele
(`bindServer`); a sala guarda o id do servidor e, quando muda quem está nela (entra, sai, cai e não
volta) ou o ao vivo, o mudo ou o silenciado de alguém, manda ao `Server` a lista de quem está nela:
id, nome, conta do Discord (id e foto), ao vivo, mudo e silenciado. O id é o do Discord para quem
entrou conferido (o mesmo da lista de membros) e o id da sala para os outros.

O `Server` guarda essa lista por sala e manda a `server-presence` de sempre a todos os membros,
agora com o nome e a conta do Discord de cada um, para o app mostrar quem não é membro. Enquanto
uma sala ainda não mandou a lista (sala parada desde antes desta versão), vale a presença antiga,
vinda dos registros dos membros.

O `Server` liga as salas na hora em que são criadas e confere as ligações sempre que é chamado
(retrato, presença): as salas que já existiam ficam ligadas logo depois da publicação, e a sala
apagada (ou o servidor apagado) é desligada.

## Consequências

- Todo mundo que está numa sala do servidor aparece para os membros, como no Discord, com nome,
  foto, ao vivo, mudo e silenciado, inclusive quem não é membro e quem entrou sem o Discord.
- Quem não é membro passa a ser visto pelos membros do servidor (nome e foto). É o que o Discord
  faz, e quem tem o código da sala já via essas pessoas lá dentro.
- O mudo e o ao vivo vêm da sala, então funcionam também com o app antigo de quem está na sala
  (antes dependiam do app novo mandar ao registro, #107).
- Custo: uma chamada ao `Server` por mudança na lista (não a cada mensagem: a sala compara com a
  última lista mandada), e um aviso para cada membro, como na ADR 0018.
- A presença antiga (dos registros) continua como reserva e pode sair numa próxima versão.
- Sem mudança no protocolo das salas; a `server-presence` ganha `name` e `discord` (o app antigo
  ignora campos que não conhece e continua mostrando só os membros).
