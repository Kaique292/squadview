# Protocolo da malha (rascunho)

> Status: **rascunho**, vira especificação na Fase 3. Mudanças aqui exigem uma ADR. O desenho atual é o da [ADR 0012](adr/0012-malha-em-enxame.md): malha em enxame com várias árvores.

## Objetivo

Distribuir cada stream da sala para todos os membros, de 2 a milhares, com atraso de call (abaixo de 100 ms de rede) e sem pesar em quem transmite. O esforço de repasse se espalha por todos, como num torrent, e a saída de alguém não congela ninguém.

## Papéis

- **Streamer:** quem transmite. Codifica e envia para poucos "filhos" diretos.
- **Repassador:** membro que recebe e encaminha para outros membros.
- **Folha:** membro que só assiste.
- Um mesmo membro pode ter papéis diferentes em streams diferentes da sala.

## Medição

Cada membro publica para a sala, a cada 2 s:

| Campo                     | Origem                                                          |
| ------------------------- | --------------------------------------------------------------- |
| `upload_kbps` livre       | estimativa por congestionamento do QUIC + teste curto ao entrar |
| `download_kbps` livre     | idem                                                            |
| `rtt_ms` até cada vizinho | pings do QUIC                                                   |
| `loss`                    | pacotes perdidos / esperados na janela                          |
| `in_game`                 | processo em tela cheia detectado + escolha manual               |

## Faixas e árvores

1. O vídeo é cortado em **K = 4 faixas, mais P = 1 de paridade**: com quaisquer 4 das 5 faixas, a imagem é remontada.
2. Cada faixa desce por uma **árvore própria**. Cada membro tem uma **faixa da casa** e só repassa nela: é galho numa árvore e folha nas outras.
3. Quem transmite manda no máximo 2 cópias, divididas igualmente entre as árvores, para semear cada uma.
4. **Tronco:** em cada árvore, quem tem ela como casa entra primeiro, do maior upload para o menor. Cada um pendura em quem transmite ou em outro do tronco, pelo menor atraso.
5. **Folhas:** cada membro pega as outras faixas de quem está no tronco de cada árvore, pelo menor atraso, respeitando a vaga e o NAT.
6. Quem está atrás de CGNAT, ou não tem upload para uma faixa, só recebe. Quem está jogando doa no máximo 1,5 Mbps.
7. **Repassador na nuvem** (opcional): recebe uma cópia e só serve quem não alcança ninguém com vaga.
8. A malha é refeita quando alguém entra, sai ou muda muito a medição. A troca é gradual: só muda quem ganha bastante com ela.

O planejamento está em `sv_mesh::plan` e é testado com o simulador `sv_mesh::sim` (`cargo run -p sv-mesh --release --example simular`).

## Camadas de qualidade

- O streamer gera duas camadas (alta e baixa) por simulcast ou SVC, cada uma com as suas 5 faixas.
- Cada membro assina a camada que o download dele aguenta (`sv_mesh::layer_for_download`).
- Um repassador só encaminha as camadas que os filhos pediram.

## Recuperação de perda

- **NACK:** o filho pede o pacote perdido ao pai, não ao streamer. O pai guarda os últimos ~500 ms.
- **Paridade entre faixas:** a quinta faixa cobre a perda de qualquer outra, inclusive a saída de quem repassava.
- **FEC dentro da faixa:** pacotes de redundância ligados quando a perda passa de 2%.
- **Keyframe sob demanda:** se a perda tornar o vídeo irrecuperável, o pedido sobe até o streamer.

## Troca de pai

- Se o pai de uma faixa some por mais de 300 ms, a imagem segue pela paridade (4 das 5 faixas) e o membro pede um pai novo naquela árvore.
- Meta: ninguém congela na saída de uma pessoa; menos de 1 s para a faixa voltar.

## Atravessar NAT

Em camadas ([ADR 0014](adr/0014-conexao-em-camadas.md)):

1. Os membros trocam candidatos de endereço pela sinalização. Quem aceita a conexão manda também o seu endereço no repassador TURN (`relay`), se tiver.
2. Quem conecta tenta direto (hole punching UDP), por IPv4 e IPv6 ao mesmo tempo. No IPv6 não há NAT: o furo dos dois lados só abre o firewall do roteador.
3. Se não fechar em 1,5 s (CGNAT simétrico), tenta também pelo `relay`. Já funciona para vídeo e voz entre dois membros.
4. Se ainda não der, pede `relay-request` a um membro que já assiste aquele stream, e ele repassa os frames como estão. Já funciona para o vídeo, com um salto. Para a voz, a [ADR 0020](adr/0020-voz-repassada-pela-sala.md) faz o mesmo: quem chama pede `voice-relay-request` a um membro que já fala com os dois, e ele copia os pacotes de voz por uma conexão de repasse (`sv-voz-repasse`), cada um com a origem. Na malha (Fase 3), isso vira as faixas vindas de membros com rede aberta e vaga, e o repassador fica só para quem não alcança ninguém.

## Mensagens (esboço)

| Mensagem          | Direção           | Conteúdo                                            |
| ----------------- | ----------------- | --------------------------------------------------- |
| `Hello`           | membro ↔ membro   | versão do protocolo, `PeerId`, capacidades de codec |
| `Stats`           | membro → sala     | medições acima                                      |
| `Assign`          | streamer → membro | pai, pai reserva, camada                            |
| `Media`           | pai → filho       | pacote de mídia (camada, sequência, timestamp)      |
| `Nack`            | filho → pai       | sequências perdidas                                 |
| `KeyframeRequest` | membro → streamer | —                                                   |
| `Bye`             | membro → sala     | saída limpa                                         |
