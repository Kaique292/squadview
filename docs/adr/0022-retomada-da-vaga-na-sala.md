# 0022. Retomada da vaga na sala depois de uma queda da sinalização

- **Status:** aceita
- **Data:** 2026-10-22

## Contexto

Os registros de desempenho mostraram quedas da conexão com a sinalização (o WebSocket com o Worker) a cada 15 min a 2 h, todas resolvidas em ~1,4 s. Ou seja, a internet estava funcionando e só o WebSocket tinha fechado. As causas possíveis estão fora do nosso controle: reinício do Durable Object (todo `wrangler deploy` e as manutenções da Cloudflare), roteadores no caminho e conexões que param de responder.

O problema é o que acontecia depois. O Worker avisava `member-left` na hora e a pessoa voltava como membro novo, com outro id. Por causa do `member-left`, os outros fechavam a conexão QUIC de voz com ela, mesmo com essa conexão funcionando (ela não depende da sinalização). A voz caía, tocavam os sons de saída e entrada, e quem caiu via os avisos "caiu" e "voltou". Um soluço de 1 s virava uma desconexão de verdade.

No Discord isso não aparece por dois motivos. O cliente retoma a sessão (`resume`) com a mesma identidade, e a voz é uma conexão separada do canal de controle.

## Decisão

Quem cai sem `leave` mantém a vaga na sala por **60 s**. O `welcome` traz um token de retomada. Se o app voltar dentro do prazo, mandando `resume: {id, token}` no `join`, ele recebe o **mesmo id**, e os outros recebem só um `member-updated`: para eles, ninguém saiu. Se não voltar, o alarme do Durable Object avisa `member-left` no fim do prazo.

O app tenta voltar na hora (a primeira tentativa não espera), só mostra o aviso de queda se a volta passar de 3 s e anota no registro o motivo de cada queda e se a vaga foi retomada.

## Consequências

- **Quedas curtas ficam invisíveis.** Voz, lives e lista de membros não mudam para ninguém, e quem caiu não vê aviso.
- **As vagas ficam no armazenamento do Durable Object (`seats`)**, então sobrevivem ao reinício da sala. Quando a sala reinicia, as conexões somem sem `webSocketClose`. O primeiro `join` depois disso dá prazo às vagas sem conexão, e quem não voltar sai no fim dele.
- **Conexão antiga pendurada é trocada pela nova.** Se o app volta antes de a sala perceber a queda, a conexão antiga é fechada com o código 4410 (`replaced`), sem tirar ninguém da sala.
- **Saída de verdade continua imediata.** Quem sai com `leave` ou é derrubado por erro (mensagem inválida, limite de taxa) sai na hora e não deixa vaga.
- **Quem fecha o app à força continua na lista por até 60 s.** Durante o prazo, sinais para essa pessoa se perdem sem o erro `unknown-peer` (o app tenta de novo). Isso é aceitável: o Discord faz o mesmo.
- **O token é um segredo de 128 bits**, conhecido só por quem recebeu o `welcome` e nunca gravado no registro. Membro conferido só retoma a vaga da própria conta.
- **Compatível com apps antigos, sem mudar a versão do protocolo.** Os campos são opcionais, e só o app que manda `resumable: true` no `join` ganha vaga guardada. O app antigo (até a 0.10) continua saindo da sala na hora quando cai e volta como membro novo, exatamente como antes.
  - Revisão de 2026-10-22: a primeira versão guardava a vaga de todos. Com os apps 0.10 (que voltam com outro id e, ao sair, nem mandam o `leave`), a vaga guardada aparecia como um "clone" de quem tinha saído, ainda "AO VIVO", por até 60 s. Agora a vaga é só de quem sabe voltar para ela; o alarme também passa a se reagendar sempre, para uma vaga nunca ficar além do prazo.
- **App reaberto volta para a mesma vaga.** Sem o token (o app foi fechado e aberto de novo), o membro conferido retoma a vaga da mesma conta do Discord, se ela ainda estiver no prazo. Assim também não aparece clone de quem reabre o app.
- **Fica de fora o WebSocket de amigos e presença.** Ele tem o mesmo padrão de queda, mas a queda nele não afeta a voz. Se os registros mostrarem que incomoda, vale outra ADR.
- **O que acompanhar:** os motivos anotados em `room-connection` (`reason`) e quantas voltas são `resumed: false`.

## Alternativas consideradas

- **Só não fechar a voz no `member-left` enquanto o QUIC estiver vivo:** a lista ainda piscaria, com sons de saída e entrada e o id trocado, e o quadro da live ficaria preso ao id antigo. Resolveria só parte do problema.
- **Prazo de tolerância só na memória do Durable Object (`setTimeout`):** não sobrevive à hibernação nem ao reinício da sala, que é justamente uma das causas das quedas.
- **Ping mais frequente para evitar as quedas:** não resolve o reinício do Durable Object nem o fechamento pela Cloudflare. O ping a cada 20 s já mantém a conexão ativa.
- **Mudar a versão do protocolo:** forçaria todos a atualizar ao mesmo tempo, sem necessidade, já que os campos novos são opcionais.
