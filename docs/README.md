# Documentação do SquadView

| Documento                                     | Para quê                                                  |
| --------------------------------------------- | --------------------------------------------------------- |
| [Arquitetura](arquitetura.md)                 | Como as peças se encaixam: captura, encode, malha, decode |
| [Protocolo da malha](protocolo-malha.md)      | Como a sala distribui o vídeo entre os membros            |
| [Roadmap](roadmap.md)                         | Fases do projeto, entregas e critérios de saída           |
| [Decisões (ADRs)](adr/)                       | Por que escolhemos cada tecnologia                        |
| [Desenvolvimento](desenvolvimento.md)         | Preparar o PC, rodar, testar e gerar o instalador         |
| [Git e versionamento](git-e-versionamento.md) | Branches, commits semânticos, SemVer e changelog          |
| [Esteira](esteira.md)                         | CI/CD: o que roda em cada PR e como sai uma release       |
| [Testes](testes.md)                           | Estratégia de testes, cobertura e testes de performance   |
| [Design da interface](design/ui.md)           | Tokens, cores, tipografia e componentes                   |

## Decisões registradas

| #                                                  | Decisão                                      |
| -------------------------------------------------- | -------------------------------------------- |
| [0001](adr/0001-tauri-rust-react.md)               | Tauri + Rust no núcleo + React na interface  |
| [0002](adr/0002-p2p-quic-sem-servidor-de-midia.md) | P2P sobre QUIC, sem servidor de mídia        |
| [0003](adr/0003-sinalizacao-cloudflare-worker.md)  | Sinalização num Cloudflare Worker            |
| [0004](adr/0004-abstracao-de-encoder.md)           | Encoder atrás de uma trait, NVENC primeiro   |
| [0005](adr/0005-ui-estilo-discord.md)              | Interface com a hierarquia visual do Discord |
