# Roadmap

Status: ⬜ não começou · 🟨 em andamento · ✅ concluída

| Fase               | Status | Entrega                                                       | Critério de saída                                                                      |
| ------------------ | ------ | ------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| 0. Fundação        | 🟨     | Estrutura, docs, esteira, esqueleto do app                    | CI verde em `dev` e `main`; release `v0.1.0` com instalador                            |
| 1. Pipeline local  | ⬜     | Captura → NVENC → decode → preview; áudio do jogo             | 1080p60 com perda de FPS no jogo < 5% e latência local < 30 ms                         |
| 2. Transmissão 1:1 | ⬜     | QUIC, hole punching, sinalização                              | 2 PCs em redes diferentes, < 100 ms ponta a ponta                                      |
| 3. Malha da sala   | ⬜     | Árvore de repasse, medição, pai reserva, 2 camadas, NACK/FEC  | Sala com 6+ pessoas; troca de pai < 1 s; membro fraco recebe 720p sem afetar os outros |
| 4. App completo    | ⬜     | Salas, vários streams, configurações, login Discord, UI final | Fluxo completo sem terminal; prints reais no README                                    |
| 5. Distribuição    | ⬜     | AMF (AMD), Rich Presence, bot de aviso                        | `v1.0.0`                                                                               |
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

- [ ] `sv-capture`: captura de monitor e de janela via Windows.Graphics.Capture
- [ ] `sv-capture`: áudio do processo do jogo via WASAPI process loopback
- [ ] `sv-encode`: implementação NVENC (H.264 baixa latência, keyframe sob demanda)
- [ ] `sv-media`: decode D3D11VA e exibição na janela
- [ ] Preview local na interface com estatísticas (fps, bitrate, latência)
- [ ] Medição com a skill `perf-check` em pelo menos 2 jogos

## Fase 2 — Transmissão 1:1

- [ ] `sv-net`: conexão QUIC com TLS e ALPN `squadview/1`
- [ ] Sinalização por WebSocket no Durable Object da sala
- [ ] Hole punching UDP e fallback por terceiro membro
- [ ] Envio de mídia com NACK e pedido de keyframe

## Fase 3 — Malha da sala

- [ ] `sv-mesh`: construção da árvore e pai reserva
- [ ] Simulcast/SVC com 2 camadas
- [ ] FEC adaptativo
- [ ] Simulador de sala para testes (perda, jitter, saída de membros)

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
