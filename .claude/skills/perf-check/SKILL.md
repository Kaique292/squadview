---
name: perf-check
description: Roteiro para medir o impacto do SquadView no jogo (FPS, 1% low, latência, uso do encoder e da GPU). Use antes e depois de mudanças em captura, encode, rede ou renderização.
---

# Medição de performance

O objetivo do projeto: transmitir 1080p60 com **menos de 5% de perda de FPS** no jogo e **menos de 100 ms** ponta a ponta.

## Preparação

- Build de release (`yarn build`); nunca meça o build de debug.
- Feche apps pesados; mesma resolução, mesmas configurações do jogo e mesmo trecho (replay, benchmark interno ou mapa vazio).
- Ferramenta de FPS: PresentMon ou CapFrameX (frametimes); uso do encoder pelo Gerenciador de Tarefas (GPU → "Video Encode") ou `nvidia-smi dmon -s u`.

## Roteiro

1. **Base:** jogue 2 min sem o SquadView. Anote FPS médio e 1% low.
2. **Transmitindo:** repita transmitindo em 1080p60. Anote os mesmos números, mais o uso do encoder e da GPU.
3. **Latência:** mostre um cronômetro em ms na tela transmitida e fotografe as duas telas juntas (a de quem transmite e a de quem assiste); a diferença é a latência ponta a ponta. Faça 5 medições e use a mediana.
4. Repita em pelo menos 2 jogos (um leve e um pesado).

## Relatório (cole no PR)

| Jogo | FPS base | FPS transmitindo | Δ%  | 1% low base → transmitindo | Encoder % | Latência (mediana) |
| ---- | -------- | ---------------- | --- | -------------------------- | --------- | ------------------ |

Inclua GPU, driver, resolução e versão do SquadView. Diga claramente o que **não** foi medido.
