# 0002. P2P sobre QUIC, sem servidor de mídia

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

O projeto é para grupos de amigos e deve ser usado por qualquer um sem pagar servidor. Um SFU central custaria banda e manutenção. Enviar uma cópia por amigo (malha completa) esgota o upload de quem transmite.

## Decisão

Os membros da sala formam uma **árvore de repasse**: o streamer envia para poucos membros, que encaminham para os outros, de acordo com upload, latência e perda medidos. O transporte é **QUIC** (crate `quinn`) com protocolo próprio, descrito em `docs/protocolo-malha.md`.

## Consequências

- Custo zero de servidor de mídia; a capacidade cresce com a sala.
- Controle total sobre camadas, repasse e troca de pai, coisa difícil de fazer com WebRTC.
- Cada salto soma latência; limitamos a profundidade a 2–3 saltos.
- O navegador não fala esse protocolo: a Discord Activity (assistir dentro do Discord) precisará de uma ponte, avaliada na Fase 6.

## Alternativas consideradas

- **WebRTC + SFU (LiveKit/mediasoup):** exige servidor e banda pagos.
- **WebRTC em malha completa:** o upload do streamer vira o limite com 3–4 pessoas.
