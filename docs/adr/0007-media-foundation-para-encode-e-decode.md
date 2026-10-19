# 0007. Media Foundation para encode e decode por hardware

- **Status:** aceita
- **Data:** 2026-10-19
- **Altera:** [0004](0004-abstracao-de-encoder.md) (a trait continua; muda a primeira implementação)

## Contexto

A ADR 0004 previa NVENC direto na Fase 1 e AMF só na Fase 5. Para o primeiro mínimo funcional, queremos que o amigo com placa AMD já participe, e o SDK do NVENC exige muito código FFI só para a NVIDIA.

## Decisão

A primeira implementação de `sv_encode::Encoder` (e do decoder em `sv-media`) usa os **MFTs de hardware do Media Foundation** do Windows, alimentados por texturas D3D11 através de um `IMFDXGIDeviceManager`. No PC com NVIDIA, por baixo, quem codifica é o NVENC; na AMD, o AMF; na Intel, o QuickSync.

Configuração: H.264, modo de baixa latência (`CODECAPI_AVLowLatencyMode`), CBR, sem B-frames, GOP longo com keyframe sob demanda (`CODECAPI_AVEncVideoForceKeyFrame`).

## Consequências

- Um único caminho de código para NVIDIA, AMD e Intel desde a Fase 1.
- Continua zero cópia: a textura da captura é convertida para NV12 na GPU e entregue ao MFT como `IMFDXGIBuffer`.
- Menos controle fino que os SDKs nativos (ex.: intra refresh, ajustes por fabricante). Se a medição (`perf-check`) mostrar latência ou qualidade insuficiente, um backend NVENC direto entra atrás da mesma trait.
- O comportamento varia um pouco entre drivers. Os testes de GPU (`#[ignore]`) precisam rodar em NVIDIA e AMD antes de cada release.

## Alternativas consideradas

- **NVENC direto agora:** mais controle, mas só NVIDIA e bem mais tempo até o primeiro uso real.
- **FFmpeg:** dependência grande e caminho zero cópia mais difícil com a textura da captura.
