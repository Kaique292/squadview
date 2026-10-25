# 0028. Faixas e paridade no envio: um stream QUIC por faixa e remontagem com prazo

- **Status:** proposta
- **Data:** 2026-10-24

## Contexto

A [ADR 0012](0012-malha-em-enxame.md) decidiu cortar o vídeo em 4 faixas mais 1 de paridade, cada uma descendo por uma árvore, para que a saída de quem repassa não congele ninguém. O planejador (`sv_mesh::plan`) já monta as árvores, mas a rede ainda mandava cada frame inteiro num stream ([ADR 0008](0008-midia-sobre-quic-um-stream-por-frame.md)). Faltava decidir o formato das faixas, como quem recebe junta faixas vindas de vários pais e onde entra o NACK que o roadmap pede desde a Fase 2.

## Decisão

Cada frame codificado vira **4 faixas de dados do mesmo tamanho (a última completada com zeros) e 1 de paridade, o XOR das 4**. Cada faixa vai num **stream unidirecional QUIC próprio**, com um `StripeHeader` na frente: o cabeçalho do frame, a camada, o índice da faixa e o tamanho do frame inteiro.

- **Remontagem** (`sv_net::stripes::Reassembler`): junta as faixas de todas as conexões de um stream num lugar só. Com 4 faixas quaisquer, refaz a que faltou pelo XOR e entrega o frame. Os frames saem **na ordem** (o decoder precisa). Um frame incompleto é pulado quando outro mais novo já está pronto e a primeira faixa que espera chegou há mais de 150 ms; aí sai um aviso de perda e o decoder pede um keyframe. Até 16 frames ficam em montagem ao mesmo tempo, e os buffers das faixas são reaproveitados.
- **NACK:** cada faixa num stream QUIC já é retransmitida salto a salto. O pacote perdido é pedido de novo ao pai, não a quem transmite, que é o NACK da ADR 0012. Não há um `Nack` próprio no protocolo. A perda que a retransmissão não resolve a tempo (o pai saiu) é coberta pela paridade.
- **Paridade só com XOR:** 1 faixa de paridade cobre a perda de 1 faixa qualquer, que é o caso da saída de um pai numa árvore. Perder 2 faixas do mesmo frame vira perda do frame.
- **Escopo deste passo:** o envio (`send_stripe`) e a recepção (`StripeReceiver`) existem em `sv-net`, com teste de ponta a ponta. Ainda não são usados pela sala: ligar as faixas às árvores (faixa da casa, troca de pai) é o próximo item do roadmap, e vem com a ALPN própria da malha, para apps antigos não receberem faixas.

## Consequências

- Uma conexão que cai tira uma faixa, e os frames continuam inteiros. O teste com 5 conexões, uma caindo no meio do stream, mostra isso.
- A paridade aumenta o envio em 25% (5 faixas para 4 de conteúdo), como a ADR 0012 já previa.
- Cada frame vira 5 streams QUIC em vez de 1. O limite de streams abertos (512) continua folgado a 60 fps, mas o custo de CPU de abrir 300 streams por segundo por conexão precisa de medição no app real.
- O XOR e a cópia para o frame inteiro passam pela CPU, nos bytes já codificados (dezenas de KB por frame, algumas centenas num keyframe). A imagem continua na GPU. O frame remontado é um `Vec` novo por frame, como na recepção de hoje.
- Esperar o frame mais velho até 150 ms segura os seguintes. É o preço de entregar na ordem, e o prazo é curto perto da troca de pai (< 1 s).

## Alternativas consideradas

- **Reed-Solomon (mais de uma paridade):** cobriria a perda de 2 faixas, mas custa mais CPU e mais envio. Com uma árvore por faixa, a saída de um pai tira só uma faixa. Fica para depois, se a medição mostrar perda dupla frequente.
- **Datagramas com NACK próprio:** dão controle fino de prazo, mas obrigam a reimplementar retransmissão, ordem e controle de congestionamento que o QUIC já faz por stream.
- **Cortar por pacote (intercalado) em vez de por frame:** espalha melhor uma rajada de perda, mas complica a remontagem e o repasse. Por frame, cada faixa é um objeto inteiro, como os _objects_ do MoQ.
