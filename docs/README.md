# Documentação do SquadView

| Documento                                     | Para quê                                                  |
| --------------------------------------------- | --------------------------------------------------------- |
| [Arquitetura](arquitetura.md)                 | Como as peças se encaixam: captura, encode, malha, decode |
| [Captura](captura.md)                         | Qualquer modo de tela, seguindo o jogo entre janelas      |
| [Protocolo da malha](protocolo-malha.md)      | Como a sala distribui o vídeo entre os membros            |
| [Roadmap](roadmap.md)                         | Fases do projeto, entregas e critérios de saída           |
| [Fliperama](fliperama.md)                     | Plano do modo Lan House Fliperama (controles remotos)     |
| [Decisões (ADRs)](adr/)                       | Por que escolhemos cada tecnologia                        |
| [Desenvolvimento](desenvolvimento.md)         | Preparar o PC, rodar, testar e gerar o instalador         |
| [Git e versionamento](git-e-versionamento.md) | Branches, commits semânticos, SemVer e changelog          |
| [Esteira](esteira.md)                         | CI/CD: o que roda em cada PR e como sai uma release       |
| [Testes](testes.md)                           | Estratégia de testes, cobertura e testes de performance   |
| [Design da interface](design/ui.md)           | Tokens, cores, tipografia e componentes                   |

## Decisões registradas

| #                                                         | Decisão                                                                             |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| [0001](adr/0001-tauri-rust-react.md)                      | Tauri + Rust no núcleo + React na interface                                         |
| [0002](adr/0002-p2p-quic-sem-servidor-de-midia.md)        | P2P sobre QUIC, sem servidor de mídia                                               |
| [0003](adr/0003-sinalizacao-cloudflare-worker.md)         | Sinalização num Cloudflare Worker                                                   |
| [0004](adr/0004-abstracao-de-encoder.md)                  | Encoder atrás de uma trait, NVENC primeiro                                          |
| [0005](adr/0005-ui-estilo-discord.md)                     | Interface com a hierarquia visual do Discord                                        |
| [0006](adr/0006-atualizacao-automatica.md)                | Atualização automática assinada pelas GitHub Releases                               |
| [0007](adr/0007-media-foundation-para-encode-e-decode.md) | Media Foundation para encode e decode por hardware                                  |
| [0008](adr/0008-midia-sobre-quic-um-stream-por-frame.md)  | Vídeo em um stream QUIC por frame, áudio em datagramas                              |
| [0009](adr/0009-login-com-discord.md)                     | Login com o Discord por OAuth2 com PKCE, sem segredo no app                         |
| [0010](adr/0010-chat-de-voz-na-sala.md)                   | Chat de voz na sala: malha QUIC, Opus mono e supressão de ruído                     |
| [0011](adr/0011-amigos-e-presenca.md)                     | Amigos, presença e pedido para entrar na sala, com identidade conferida pelo Worker |
| [0012](adr/0012-malha-em-enxame.md)                       | Malha em enxame: várias árvores, paridade e repassador na nuvem de reserva          |
| [0013](adr/0013-seguir-o-jogo-sem-injecao.md)             | Seguir o jogo entre janelas e para o monitor, sem injeção                           |
| [0014](adr/0014-conexao-em-camadas.md)                    | Conexão em camadas: direto, depois o repassador TURN de quem aceita (CGNAT)         |
| [0015](adr/0015-efeito-de-fala-no-status.md)              | Efeito de fala de cada um, enviado no status da sala                                |
| [0016](adr/0016-servidores-e-salas-fixas.md)              | Servidores com salas fixas, guardados no Worker                                     |
| [0017](adr/0017-mensagens-privadas-p2p.md)                | Mensagens privadas direto entre os dois PCs                                         |
| [0018](adr/0018-presenca-nas-salas-do-servidor.md)        | Presença nas salas do servidor e entrada direta para membros                        |
| [0019](adr/0019-varias-lives-ao-mesmo-tempo.md)           | Várias lives ao mesmo tempo, cada uma num quadro da sala                            |
| [0020](adr/0020-voz-repassada-pela-sala.md)               | Voz repassada por um membro da sala quando dois membros não se alcançam             |
| [0021](adr/0021-video-por-baixo-da-interface.md)          | Vídeo desenhado por baixo da interface (DirectComposition)                          |
| [0022](adr/0022-retomada-da-vaga-na-sala.md)              | Retomada da vaga na sala depois de uma queda da sinalização                         |
| [0023](adr/0023-modo-fliperama.md)                        | Modo Lan House Fliperama: jogo local com controles remotos (ViGEmBus)               |
| [0024](adr/0024-fliperama-teclado-e-mouse.md)             | Fliperama: mouse no analógico e "controle total" de teclado e mouse (com permissão) |
| [0026](adr/0026-simulcast-com-2-camadas.md)               | Simulcast com 2 camadas, escolhidas por quem transmite para cada espectador         |
| [0028](adr/0028-faixas-e-paridade-no-envio.md)            | Faixas e paridade no envio: um stream QUIC por faixa e remontagem com prazo         |
| [0029](adr/0029-troca-de-pai-na-malha.md)                 | Troca de pai na malha: conserto local, só para quem perdeu uma faixa                |
| [0030](adr/0030-malha-na-rede-da-sala.md)                 | Malha na rede da sala: quem transmite coordena, faixas pela conexão direta e pais   |
