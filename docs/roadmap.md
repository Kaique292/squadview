# Roadmap

Status: ⬜ não começou · 🟨 em andamento · ✅ concluída

Onde estamos (v0.10): a Fase 0 fechou. Na Fase 1, o pipeline funciona com NVIDIA e AMD (League of Legends: 59 fps transmitidos, ~5 ms de captura até o encode), mas falta medir a perda de FPS do jogo com PresentMon em mais um jogo. Na Fase 2, a conexão direta, o hole punching e os repasses de reserva já estão no ar; faltam o NACK e o teste com um amigo em outra rede, medindo a latência. Na Fase 3 já entraram o planejador e o simulador da malha, o IPv6, o repasse do vídeo e da voz por um membro da sala e a sala sem limite de pessoas; falta a sala distribuir o vídeo de verdade (faixas, simulcast, troca de pai). Na Fase 4, entraram servidores com salas fixas e presença, conversas privadas, picture in picture com controles, efeitos de fala, várias lives ao mesmo tempo, o vídeo desenhado por baixo da interface e reações e respostas nas conversas. Na Fase 5, já saíram o AMF (pelo Media Foundation) e o Rich Presence.

| Fase               | Status | Entrega                                                        | Critério de saída                                                                      |
| ------------------ | ------ | -------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| 0. Fundação        | ✅     | Estrutura, docs, esteira, esqueleto do app                     | CI verde em `dev` e `main`; release `v0.1.0` com instalador                            |
| 1. Pipeline local  | 🟨     | Captura → NVENC → decode → preview; áudio do jogo              | 1080p60 com perda de FPS no jogo < 5% e latência local < 30 ms                         |
| 2. Transmissão 1:1 | 🟨     | QUIC, hole punching, sinalização                               | 2 PCs em redes diferentes, < 100 ms ponta a ponta                                      |
| 3. Malha da sala   | 🟨     | Árvore de repasse, medição, pai reserva, 2 camadas, NACK/FEC   | Sala com 6+ pessoas; troca de pai < 1 s; membro fraco recebe 720p sem afetar os outros |
| 4. App completo    | 🟨     | Salas, vários streams, configurações, login Discord, UI final  | Fluxo completo sem terminal; prints reais no README                                    |
| 5. Distribuição    | 🟨     | AMF (AMD), Rich Presence, bot de aviso                         | `v1.0.0`                                                                               |
| 6. Futuro          | ⬜     | Discord Activity, QuickSync (Intel), gravação local, Lan House | —                                                                                      |

## Fase 0 — Fundação

- [x] Workspace Rust com os crates `sv-*`
- [x] App Tauri + React com o layout base
- [x] Worker de sinalização (esqueleto)
- [x] Commits semânticos com commitlint (sem coautor)
- [x] Docs, ADRs e roadmap
- [x] Agentes e skills do Claude
- [x] CI, release-please, instalador NSIS e badge de cobertura
- [x] Atualização automática assinada (OTA)
- [x] CI verde em `dev` e `main`
- [x] Release `v0.1.0` (e depois até a `v0.5.0`)

## Fase 1 — Pipeline local

- [x] `sv-capture`: captura de monitor e de janela via Windows.Graphics.Capture (segue o jogo do cliente para a partida e para o monitor em tela cheia, ver [captura.md](captura.md))
- [x] `sv-capture`: áudio do processo do jogo via WASAPI process loopback (+ Opus, buffer de jitter e reprodução)
- [x] `sv-encode`: encoder de hardware via Media Foundation (NVENC/AMF/QuickSync, H.264 baixa latência, keyframe sob demanda)
- [x] `sv-media`: decode por GPU e exibição na janela
- [x] Preview local na interface com estatísticas (fps, bitrate, latência, áudio) e registro local
- [ ] Medição com a skill `perf-check` em pelo menos 2 jogos (1 feito: LoL, 74 fps mantidos, 5 ms de latência)

## Fase 2 — Transmissão 1:1

- [x] `sv-net`: conexão QUIC com TLS e ALPN `squadview/1` (mais `squadview-voice/1` e `squadview-dm/1`)
- [x] Sinalização por WebSocket no Durable Object da sala
- [x] Hole punching UDP e fallback por terceiro membro (repasse por quem já assiste e TURN de reserva, [ADR 0014](adr/0014-conexao-em-camadas.md))
- [x] Envio de mídia com NACK e pedido de keyframe: o NACK é a retransmissão do QUIC em cada stream, salto a salto ([ADR 0028](adr/0028-faixas-e-paridade-no-envio.md))
- [ ] Teste com um amigo em outra rede, medindo a latência ponta a ponta

## Fase 3 — Malha da sala ([ADR 0012](adr/0012-malha-em-enxame.md))

Salas sem limite fixo, com atraso de call: malha em enxame com várias árvores.

- [x] `sv-mesh`: planejador em enxame (4 faixas + paridade, faixa da casa, NAT, camadas)
- [x] Simulador de sala (população do Brasil, CGNAT, saída de membros) com os números na ADR
- [x] Simulcast com 2 camadas no encoder (a principal e 720p30), a baixa só quando alguém precisa e escolhida por quem transmite para cada espectador ([ADR 0026](adr/0026-simulcast-com-2-camadas.md)); falta medir com o `perf-check`
- [x] Faixas e paridade no envio (`sv-net`) e remontagem na recepção ([ADR 0028](adr/0028-faixas-e-paridade-no-envio.md)); a sala passa a usar no item seguinte
- [ ] Repasse pela faixa da casa e troca de pai sem congelar: o conserto do plano está no planejador e no simulador (troca p95 de 370 a 390 ms, [ADR 0029](adr/0029-troca-de-pai-na-malha.md)); falta a rede da sala (coordenação por quem transmite, conexões da malha e medição de upload)
- [x] Repassador na nuvem de reserva (TURN da Cloudflare), só quando o direto não fecha ([ADR 0014](adr/0014-conexao-em-camadas.md)); por enquanto entre dois membros, a malha usa depois
- [x] Repasse por um membro da sala que já assiste (camada do meio da ADR 0014), um salto, só vídeo e som do jogo
- [x] Conexão direta também por IPv6 (sem CGNAT)
- [x] Voz repassada por um membro da sala quando dois não se alcançam, assinada de ponta a ponta ([ADR 0020](adr/0020-voz-repassada-pela-sala.md))
- [x] Sala sem limite fixo de pessoas na sinalização
- [ ] Voz em salas grandes: só quem está falando é repassado
- [ ] Sinalização para centenas de membros (e gossip para milhares)

## Fase 4 — App completo

- [x] Criar/entrar em sala por código
- [x] Vários streams ao mesmo tempo na grade, até 4 por pessoa ([ADR 0019](adr/0019-varias-lives-ao-mesmo-tempo.md))
- [x] Configurações de qualidade, fonte e áudio
- [x] Login com Discord (OAuth)
- [x] Prints reais no README (atualizar a cada versão)
- [x] Amigos com presença, pedidos e chamar para a sala
- [x] Voz na sala com supressão de ruído, ganho automático e sons de aviso
- [x] Efeitos de fala com cor e prévia, vistos por toda a sala
- [x] Barra da chamada e página de configurações no estilo do Discord
- [x] Assistir a live em picture in picture
- [x] Servidores com salas fixas, convites, papéis e ícone com recorte ([ADR 0016](adr/0016-servidores-e-salas-fixas.md))
- [x] Presença nas salas do servidor e entrada direta para membros ([ADR 0018](adr/0018-presenca-nas-salas-do-servidor.md))
- [x] Conversas privadas direto entre dois amigos, com anexos ([ADR 0017](adr/0017-mensagens-privadas-p2p.md))
- [x] Limpar o cache do app inteiro ou de cada parte

## Fase 5 — Distribuição

- [x] Encoder AMF (pelo Media Foundation)
- [x] Rich Presence com botão para entrar na sala
- [ ] Bot de aviso no servidor do Discord

## Fase 6 — Futuro

- [ ] Modo Lan House Fliperama: jogo local com controles remotos, P2 a P4 ([ADR 0023](adr/0023-modo-fliperama.md), [plano](fliperama.md))
- [ ] Modo Lan House em rede: rede local virtual entre os membros da call (ADR própria)
- [ ] Discord Activity
- [ ] QuickSync (Intel)
- [ ] Gravação local
