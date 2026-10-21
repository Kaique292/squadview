# 0017. Mensagens privadas direto entre os dois PCs

- **Status:** aceita
- **Data:** 2026-10-21

## Contexto

O Kaique quer mensagens privadas entre dois amigos (texto, imagens, vídeos e qualquer arquivo),
com a lista de conversas na aba Amigos. A escolha dele: **nada passa pelo servidor, nem o
texto**. A mensagem só vai com os dois online; senão fica pendente no PC de quem mandou. Quem
manda decide, uma vez, se guarda uma cópia do anexo no próprio app (sem cache escondido); quem
recebe confirma antes de baixar arquivos grandes.

Hoje dois PCs só se conectam por QUIC dentro de uma sala: é a sala (`Room`) que troca os
endereços e as impressões digitais dos certificados (`VoiceOffer`/`VoiceAnswer`). Fora de sala
não existe esse caminho.

## Decisão

As mensagens vão por uma conexão QUIC direta entre os dois PCs, num ALPN próprio
(`squadview-dm/1`), com o mesmo `Peer` (STUN, hole punching, certificado autoassinado conferido
pela impressão digital, TURN de reserva) que a sala já usa.

Para os dois se acharem, o registro de amigos do Worker (`User`, ADR 0011) passa a **repassar
um sinal** de um amigo para o outro: `{"type":"dm-signal","to":"<id>","payload":{...}}` vira
`{"type":"dm-signal","from":"<id>","payload":{...}}` nas conexões do amigo. O registro só repassa
entre amigos e não guarda nada; se o amigo não está online, quem mandou recebe
`{"type":"dm-unreachable","id":"<id>"}`. Pelo mesmo registro o app pede credenciais do repassador
TURN (`turn-request` → `turn`), como a sala já manda (ADR 0014).

O conteúdo (texto, arquivos) e o histórico nunca passam pelo Worker. O histórico fica só no PC
de cada um.

## Consequências

- O Worker vê **que** dois amigos conectaram (o sinal tem endereços e a impressão digital do
  certificado), mas nunca o que eles conversam: a conexão é criptografada ponta a ponta pelo
  QUIC, e o TURN, quando usado, só repassa pacotes criptografados.
- Sem servidor guardando nada: mensagem para quem está offline espera no PC de quem mandou e vai
  sozinha quando os dois estiverem online. Trocar de PC não traz o histórico.
- O sinal tem o mesmo limite de 8 KB do `signal` da sala e conta no limite de mensagens por
  conexão; só amigos recebem (quem não é amigo não descobre nem se a pessoa está online, além do
  que a presença já mostra).
- Mensagens novas na conexão `/me`: o Worker antigo fecha a conexão de amigos ao recebê-las, então
  o **Worker precisa ser publicado antes** do app que usa mensagens.

## Alternativas consideradas

- **Worker guarda o texto até entregar:** mais prático (mensagem para quem está offline), mas o
  Kaique escolheu que nada passe pelo servidor.
- **Sinalizar por uma sala escondida (um `Room` só para os dois):** reaproveitaria o caminho da
  voz, mas o código da sala teria de ser combinado de algum jeito, e qualquer um com o código
  entraria; o registro de amigos já sabe quem é amigo de quem.
- **WebRTC na interface:** traria outra pilha de rede e de criptografia só para isso; o `Peer`
  com QUIC já está pronto e testado.
