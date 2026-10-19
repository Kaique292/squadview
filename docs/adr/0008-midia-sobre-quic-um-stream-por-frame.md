# 0008. Vídeo em um stream QUIC por frame, áudio em datagramas

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

O rascunho do protocolo previa NACK e FEC próprios sobre pacotes de mídia. Para o 1:1 da Fase 2, isso é muito código antes de saber se precisamos dele, e o QUIC já faz retransmissão e controle de congestionamento.

## Decisão

- **Vídeo:** cada frame codificado vai num **stream QUIC unidirecional próprio**. Streams não bloqueiam uns aos outros, então perder um pacote atrasa só aquele frame. Quem assiste descarta (`stop`) um frame que passar do prazo (~150 ms depois do esperado) e pede keyframe se perder um frame de referência.
- **Áudio:** cada pacote Opus (20 ms) vai num **datagrama QUIC**. Perda vira um pequeno buraco que o decoder Opus disfarça (PLC), sem atraso.
- **Controle** (pedidos de keyframe, estatísticas, pings para o relógio) num stream bidirecional de controle.

## Consequências

- Muito menos código na Fase 2, e a recuperação de perda fica a cargo do QUIC.
- Na Fase 3, o repasse entre membros encaminha frames inteiros (um stream por frame por filho). O NACK salto a salto do rascunho continua sendo uma opção a avaliar com medição.
- Um frame grande (keyframe) em rede ruim pode estourar o prazo; o encoder usa GOP longo e keyframe só sob demanda para reduzir isso.

## Alternativas consideradas

- **Tudo em datagramas com NACK/FEC próprios:** latência mais previsível, mas bem mais complexo.
- **Um único stream de vídeo:** um pacote perdido trava todos os frames seguintes (head-of-line blocking).
