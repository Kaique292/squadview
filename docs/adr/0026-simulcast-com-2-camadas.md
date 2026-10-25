# 0026. Simulcast com 2 camadas, escolhidas por quem transmite para cada espectador

- **Status:** proposta
- **Data:** 2026-10-24

## Contexto

Até a 0.13, cada live tinha um encoder só, e o `Broadcaster` mandava os mesmos frames para todos. Quem tinha internet fraca perdia frames na fila e ficava pedindo keyframe, com a imagem travando. Não havia como mandar menos para ele sem baixar a qualidade de todo mundo. A [ADR 0012](0012-malha-em-enxame.md) e o critério de saída da Fase 3 pedem que o membro fraco receba 720p sem afetar os outros.

Também apareceu um problema de medição. O QUIC guarda na fila de envio de cada conexão o que escrevemos e a rede ainda não levou, até uns 10 MB. Então a fila do espectador (6 frames) quase nunca enche: numa rede fraca o atraso cresceria por segundos antes de algum frame ser descartado.

## Decisão

Quem transmite gera a camada principal (a qualidade escolhida) e, **só enquanto algum espectador precisa**, uma camada baixa de 720p30 com um segundo encoder de hardware. Para cada espectador, quem transmite decide a camada pelo que se perde na conexão dele: frames que não cabem na fila ou vídeo preso no QUIC por mais de 250 ms.

- **Encoder:** a camada baixa converte a fonte direto para 720p na GPU (nada sai da GPU) e liga quando o `Broadcaster` diz que alguém precisa (`wants_low`). Quando ninguém mais precisa, o encoder é destruído. Se a qualidade escolhida já é 720p30 ou menos, não há camada baixa.
- **Troca:** acontece no keyframe da camada nova, então o decoder recomeça num quadro completo e a imagem não congela: até o keyframe chegar, o espectador continua recebendo a camada antiga. Se a camada nova não chega em 3 s (o encoder não abriu), a troca é desfeita.
- **Descer:** 2 frames perdidos na mesma janela de 1 s. **Subir:** depois de 10 s sem perda na camada baixa. Se a tentativa de subir falha em menos de 5 s, a próxima espera o dobro (até 120 s). Depois de 30 s estável na principal, a espera volta a 10 s.
- **Vídeo preso:** o `BacklogMeter` faz a conta "bytes escritos menos bytes que a conexão mandou" (`udp_tx`) a cada frame, sem deixar ficar negativa. Acima de 256 KB por 250 ms, conta como frame perdido.
- **Quem assiste:** o decoder lê o tamanho visível do tipo de saída novo quando o stream muda (`MF_E_TRANSFORM_STREAM_CHANGE`), e as estatísticas passam a mostrar a resolução que chega de fato.
- **Compatibilidade:** o `watch-request` ganha `simulcast: true` (opcional; ausente vale `false`). Só quem mandou o campo troca de camada; apps antigos, cujo decoder não acompanha a troca de resolução, ficam sempre na principal. Nada muda nos frames da rede: cada conexão recebe uma camada só, com a sequência dela.
- **Repasse por um membro** ([ADR 0014](0014-conexao-em-camadas.md)): o membro repassa o que recebe, uma camada só, sem troca. As faixas da malha vêm depois.

## Consequências

- Quem tem internet fraca passa a receber 720p30 (~2,5 Mbps) em vez de travar, e quem tem internet boa não perde nada.
- O segundo encoder custa tempo do encoder da GPU e uma conversão a mais, só enquanto alguém está na camada baixa. Precisa de medição com a skill `perf-check` (com e sem a camada baixa ligada), porque o app roda ao lado de jogos.
- Placas GeForce limitam as sessões de NVENC ao mesmo tempo (hoje 8). Com várias lives por pessoa ([ADR 0019](0019-varias-lives-ao-mesmo-tempo.md)), a camada baixa pode não abrir. Nesse caso o espectador fica na principal, e o motivo aparece nos detalhes da live.
- A escolha é de quem transmite, que só vê a sua conexão com cada espectador. Na malha (faixas e troca de pai), cada membro passa a assinar a camada pelo próprio download (`sv_mesh::layer_for_download`), e essa lógica se move para o repassador.
- O sinal de rede atrasada é aproximado. Retransmissões contam como "enviado" e podem esconder um atraso com perda alta. A perda ainda é coberta pela fila e pelos pedidos de keyframe.

## Alternativas consideradas

- **SVC (camadas num stream só):** o encoder H.264 do Media Foundation só faz camadas temporais (menos fps), não de resolução. Simulcast funciona igual em NVENC, AMF e QuickSync.
- **Quem assiste escolhe e avisa (mensagem nova no stream de controle):** exige que quem transmite entenda a mensagem. Um app antigo encerraria o stream de controle ao receber uma mensagem desconhecida, e com ele os pedidos de keyframe. Fica para a malha, com versão do protocolo no `Hello`.
- **Camada baixa sempre ligada:** custaria o segundo encoder o tempo todo, mesmo numa sala em que todos têm internet boa.
- **Diminuir a fila de envio do QUIC (`send_window`):** valeria para todas as conexões (voz, conversas, anexos) e não diz para quem trocar de camada; medir o vídeo preso resolve só onde importa.
