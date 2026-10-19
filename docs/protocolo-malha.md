# Protocolo da malha (rascunho)

> Status: **rascunho**, vira especificação na Fase 3. Mudanças aqui exigem uma ADR.

## Objetivo

Distribuir cada stream da sala para todos os membros usando o upload de quem tem sobra, com no máximo 2–3 saltos, e se recuperar em menos de 1 segundo quando alguém sai ou trava.

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

## Construção da árvore

1. Calcula a nota de cada candidato (`sv_mesh::PeerStats::relay_score`): vagas de repasse, penalizadas por latência e perda.
2. O streamer alimenta direto os melhores, até o próprio limite de vagas (guardando banda para o jogo).
3. Os demais membros entram como filhos do repassador de melhor nota com vaga livre, respeitando a profundidade máxima.
4. Cada membro recebe também um **pai reserva**, já conectado e em espera.
5. A árvore é recalculada quando a nota de alguém muda mais de 30% ou um membro entra ou sai. A troca é gradual: só muda quem ganha bastante com a mudança.

## Camadas de qualidade

- O streamer gera duas camadas (alta e baixa) por simulcast ou SVC.
- Cada membro assina a camada que o download dele aguenta (`sv_mesh::layer_for_download`).
- Um repassador só encaminha as camadas que os filhos pediram.

## Recuperação de perda

- **NACK:** o filho pede o pacote perdido ao pai, não ao streamer. O pai guarda os últimos ~500 ms.
- **FEC:** pacotes de redundância ligados quando a perda passa de 2%.
- **Keyframe sob demanda:** se a perda tornar o vídeo irrecuperável, o pedido sobe até o streamer.

## Troca de pai

- Se o pai some por mais de 300 ms, o filho passa para o pai reserva na hora.
- A sala elege um novo pai reserva em seguida.
- Meta: menos de 1 s de imagem congelada.

## Atravessar NAT

1. Os membros trocam candidatos de endereço pela sinalização.
2. Tentam conexão direta (hole punching UDP).
3. Se falhar (CGNAT simétrico), a conexão passa por um terceiro membro que enxerga os dois.

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
