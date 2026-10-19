# 0004. Encoder atrás de uma trait, NVENC primeiro

- **Status:** aceita (primeira implementação alterada pela [0007](0007-media-foundation-para-encode-e-decode.md))
- **Data:** 2026-10-19

## Contexto

A maioria do grupo inicial usa NVIDIA, mas há quem use AMD, e no futuro Intel. Cada fabricante tem um SDK diferente.

## Decisão

Todo encoder implementa a trait `sv_encode::Encoder`. A implementação **NVENC** entra na Fase 1; **AMF** na Fase 5; QuickSync depois. O backend é escolhido pela GPU detectada.

## Consequências

- O resto do app (rede, malha, interface) não conhece o fabricante.
- Testes podem usar um encoder falso.
- Diferenças de recursos entre fabricantes (ex.: AV1) aparecem como capacidades consultáveis, não como `if` espalhado.

## Alternativas consideradas

- **FFmpeg como camada única:** simplifica, mas acrescenta uma dependência grande e dificulta o caminho zero-cópia com a textura da captura.
- **Media Foundation:** cobre os três fabricantes, mas com menos controle de latência que os SDKs nativos. Fica como alternativa para o QuickSync.
