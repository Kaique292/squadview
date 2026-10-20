# Roadmap

Status: ⬜ não começou · 🟨 em andamento · ✅ concluída

Onde estamos (v0.1.0): a Fase 0 fechou com a release. Na Fase 1, o pipeline funciona com NVIDIA e AMD (League of Legends: 59 fps transmitidos, ~5 ms de captura até o encode), mas falta medir a perda de FPS do jogo com PresentMon. A Fase 2 funciona com a sinalização publicada e falta o teste com um amigo em outra rede. Das Fases 4 e 5 já saíram as salas, as configurações, o login com o Discord, o AMF (pelo Media Foundation) e o Rich Presence.

| Fase               | Status | Entrega                                                       | Critério de saída                                                                      |
| ------------------ | ------ | ------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| 0. Fundação        | ✅     | Estrutura, docs, esteira, esqueleto do app                    | CI verde em `dev` e `main`; release `v0.1.0` com instalador                            |
| 1. Pipeline local  | 🟨     | Captura → NVENC → decode → preview; áudio do jogo             | 1080p60 com perda de FPS no jogo < 5% e latência local < 30 ms                         |
| 2. Transmissão 1:1 | 🟨     | QUIC, hole punching, sinalização                              | 2 PCs em redes diferentes, < 100 ms ponta a ponta                                      |
| 3. Malha da sala   | ⬜     | Árvore de repasse, medição, pai reserva, 2 camadas, NACK/FEC  | Sala com 6+ pessoas; troca de pai < 1 s; membro fraco recebe 720p sem afetar os outros |
| 4. App completo    | 🟨     | Salas, vários streams, configurações, login Discord, UI final | Fluxo completo sem terminal; prints reais no README                                    |
| 5. Distribuição    | 🟨     | AMF (AMD), Rich Presence, bot de aviso                        | `v1.0.0`                                                                               |
| 6. Futuro          | ⬜     | Discord Activity, QuickSync (Intel), gravação local           | —                                                                                      |

## Fase 0 — Fundação

- [x] Workspace Rust com os crates `sv-*`
- [x] App Tauri + React com o layout base
- [x] Worker de sinalização (esqueleto)
- [x] Commits semânticos com commitlint (sem coautor)
- [x] Docs, ADRs e roadmap
- [x] Agentes e skills do Claude
- [x] CI, release-please, instalador NSIS e badge de cobertura
- [x] Atualização automática assinada (OTA)
- [ ] CI verde em `dev` e `main`
- [ ] Release `v0.1.0`

## Fase 1 — Pipeline local

- [x] `sv-capture`: captura de monitor e de janela via Windows.Graphics.Capture (segue o jogo do cliente para a partida e para o monitor em tela cheia, ver [captura.md](captura.md))
- [x] `sv-capture`: áudio do processo do jogo via WASAPI process loopback (+ Opus, buffer de jitter e reprodução)
- [x] `sv-encode`: encoder de hardware via Media Foundation (NVENC/AMF/QuickSync, H.264 baixa latência, keyframe sob demanda)
- [x] `sv-media`: decode por GPU e exibição na janela
- [x] Preview local na interface com estatísticas (fps, bitrate, latência, áudio) e registro local
- [ ] Medição com a skill `perf-check` em pelo menos 2 jogos (1 feito: LoL, 74 fps mantidos, 5 ms de latência)

## Fase 2 — Transmissão 1:1

- [ ] `sv-net`: conexão QUIC com TLS e ALPN `squadview/1`
- [ ] Sinalização por WebSocket no Durable Object da sala
- [ ] Hole punching UDP e fallback por terceiro membro
- [ ] Envio de mídia com NACK e pedido de keyframe

## Fase 3 — Malha da sala ([ADR 0012](adr/0012-malha-em-enxame.md))

Salas sem limite fixo, com atraso de call: malha em enxame com várias árvores.

- [x] `sv-mesh`: planejador em enxame (4 faixas + paridade, faixa da casa, NAT, camadas)
- [x] Simulador de sala (população do Brasil, CGNAT, saída de membros) com os números na ADR
- [ ] Simulcast com 2 camadas no encoder (1080p60 e 720p30)
- [ ] Faixas e paridade no envio (`sv-net`) e remontagem na recepção
- [ ] Repasse pela faixa da casa e troca de pai sem congelar
- [ ] Repassador na nuvem de reserva (MoQ/TURN), opt-in
- [ ] Voz em salas grandes: só quem está falando é repassado
- [ ] Sinalização para centenas de membros (e gossip para milhares)

## Fase 4 — App completo

- [ ] Criar/entrar em sala por código
- [ ] Vários streams ao mesmo tempo na grade
- [ ] Configurações de qualidade, fonte e áudio
- [ ] Login com Discord (OAuth)
- [ ] Prints reais no README

## Fase 5 — Distribuição

- [ ] Encoder AMF

- [ ] Rich Presence com botão "Assistir"
- [ ] Bot de aviso no servidor do Discord
