# 0012. Malha em enxame: várias árvores, paridade e repassador na nuvem de reserva

- **Status:** aceita
- **Data:** 2026-10-20

## Contexto

A meta é a sala ser rápida e leve com 2 ou com milhares de pessoas, sem limite fixo, como um torrent. O rascunho da Fase 3 previa **uma árvore** de repasse com pai reserva (`docs/protocolo-malha.md`, versão anterior). Ele tem três problemas:

- quem repassa num galho alto derruba todo mundo abaixo quando sai;
- o esforço de repasse fica com poucos;
- quem está atrás de CGNAT, cerca de 1 em cada 4 no Brasil, só fala com quem tem rede aberta, e numa sala pequena muitas vezes não há ninguém assim. É o caso do amigo que não conseguia assistir.

O torrent puro (cada um pede os pedaços que faltam a vizinhos) espalha bem o esforço, mas leva de 10 a 60 s de atraso. Numa call, o atraso precisa ficar abaixo de 200 a 300 ms.

## Decisão

**Distribuição em enxame com várias árvores (no estilo SplitStream), empurrando o vídeo.**

- O vídeo de quem transmite é cortado em **K = 4 faixas, mais P = 1 de paridade (FEC)**. Com quaisquer 4 das 5 faixas a imagem é remontada.
- Cada faixa desce por uma árvore própria. Cada membro **repassa só na sua "faixa da casa"**: é galho numa árvore e folha nas outras. A saída de alguém corta no máximo uma faixa de quem estava abaixo, e a paridade cobre o buraco sem congelar.
- Quem transmite manda no máximo **2 cópias** (20 Mbps em 1080p60 com a paridade), divididas entre as árvores, não importa o tamanho da sala.
- Quem tem mais upload entra primeiro e fica perto da raiz, então as árvores ficam rasas.
- Quem está jogando doa no máximo 1,5 Mbps. Quem está atrás de CGNAT ou não tem upload para uma faixa só recebe.
- **Camadas (simulcast):** a de cima é a que quem transmite escolheu (até 1440p60), e abaixo vêm 1080p60 e 720p30. Quem não couber numa camada (falta de download ou de upload na sala) desce um degrau por vez, em vez de travar. Cada camada é um encode a mais no NVENC/AMF; em placas fracas o app gera só a escolhida e 720p.
- **Torrent como conserto, não como entrega:** pacote perdido é pedido de novo ao pai ou a um vizinho da mesma árvore (NACK). O vídeo chega empurrado, com latência de call.
- **Repassador na nuvem como último recurso.** É um repassador MoQ ou TURN de terceiros (por exemplo, o da Cloudflare): recebe uma cópia e só serve faixas a quem não alcança ninguém com vaga. Pode ser por CGNAT ou por falta de upload na sala. É opcional e pago por uso, e nas salas grandes ele não é usado.
- **Transporte:** QUIC, que já usamos. A semântica segue o Media over QUIC (MoQ, IETF): faixas como _tracks_, quadros como _objects_ e prioridade por camada. Assim um repassador MoQ de terceiros encaixa sem mudar o protocolo.
- **Voz em salas grandes:** a malha atual (todos com todos) vale até cerca de 8 pessoas. Acima disso, a voz passa pelas mesmas árvores e só **quem está falando** (os 3 ou 4 com voz ativa) é repassado.
- **Sinalização:** a sala do Worker sobe do limite de 8 para centenas. Para milhares, os membros trocam entre si quem está na sala (_gossip_), e o Worker só apresenta os primeiros contatos.

## Números do simulador

Saída de `cargo run -p sv-mesh --release --example simular`. Os números vêm de uma população com a mistura do Brasil:

- **Conexões:** 4G, cabo, fibra de 20 a 400 Mbps de upload.
- **NAT:** 25% atrás de CGNAT.
- **Jogando:** 20% das pessoas.
- **Cidades:** 5.

Em cada cenário, 5% das pessoas saem de uma vez. O atraso é só o de rede, de quem transmite até quem assiste. Captura, encode e decode somam mais uns 15 a 25 ms.

| Membros | 1080p | Sem vídeo | Atraso p95 | Saltos | Quem transmite | Cada um doa | Nuvem    | Congela ao sair 5% |
| ------- | ----- | --------- | ---------- | ------ | -------------- | ----------- | -------- | ------------------ |
| 2       | 100%  | 0%        | 21 ms      | 2      | 20 Mbps        | 0           | 10 Mbps  | 0%                 |
| 8       | 100%  | 0%        | 44 ms      | 2      | 20 Mbps        | 2,5 Mbps    | 50 Mbps  | 0%                 |
| 100     | 100%  | 0%        | 55 ms      | 3      | 20 Mbps        | 8,7 Mbps    | 116 Mbps | 0%                 |
| 1.000   | 100%  | 0%        | 57 ms      | 3      | 20 Mbps        | 10 Mbps     | 0        | 0%                 |
| 5.000   | 100%  | 0%        | 58 ms      | 4      | 20 Mbps        | 10 Mbps     | 0        | 0,4%               |

**Com 1440p60 escolhido.** Quem transmite tem fibra de 150 Mbps; o resto é igual:

| Membros | 1440p | Sem vídeo | Atraso p95 | Saltos | Quem transmite | Cada um doa | Nuvem    | Congela ao sair 5% |
| ------- | ----- | --------- | ---------- | ------ | -------------- | ----------- | -------- | ------------------ |
| 8       | 100%  | 0%        | 44 ms      | 2      | 40 Mbps        | 4,5 Mbps    | 104 Mbps | 0%                 |
| 100     | 100%  | 0%        | 52 ms      | 3      | 40 Mbps        | 16,2 Mbps   | 360 Mbps | 0%                 |
| 1.000   | 100%  | 0%        | 60 ms      | 4      | 40 Mbps        | 20 Mbps     | 0        | 0%                 |
| 5.000   | 100%  | 0%        | 62 ms      | 4      | 40 Mbps        | 20 Mbps     | 0        | 1,7%               |

Em 1440p, quem transmite precisa de uns 55 Mbps de upload livre, e cada um que assiste doa em média 20 Mbps. Com a mistura de internet simulada, a sala dá conta sem descer ninguém; com muita gente de upload fraco, uma parte desceria para 1080p.

Comparações, com os mesmos cenários:

- **Sem o repassador na nuvem:** de 2 a 100 pessoas, entre 20% e 50% ficam sem vídeo. São as pessoas atrás de CGNAT. Nas salas grandes não muda nada.
- **Sem a paridade:** a saída de 5% congela 16% a 20% das pessoas nas salas grandes, contra 0% a 0,4% com ela.
- **Uma árvore só (o desenho antigo):** congela 7,5% a 8,8% das pessoas, e os galhos altos carregam todo o peso.

## Consequências

- A latência cresce com o log do tamanho da sala (4 saltos para 5.000 pessoas), e o upload de quem transmite é fixo. A sala escala sem pesar em quem joga.
- Em média, quem assiste doa um pouco mais do que baixa (cerca de 10 Mbps para um stream de 8 Mbps, por causa da paridade). Quem não pode doar (CGNAT, 4G, jogando) é coberto pelos outros, e em sala pequena pela nuvem.
- O repassador na nuvem traz um custo por uso e faz o vídeo de algumas pessoas passar por terceiros. Por isso ele é **opt-in** e só entra quando não há caminho direto. A configuração e a conta ficam com quem hospeda o projeto, e a decisão de ligar será registrada quando chegar a hora.
- Antes de cada etapa da Fase 3, o planejador (`sv_mesh::plan`) e o simulador (`sv_mesh::sim`) mudam primeiro, com teste. A rede real vem depois.
- Ficam para medir no app real: congestionamento real (o simulador usa capacidade fixa), tempo de reconstrução depois de uma saída, e o custo de CPU de repassar 5 faixas.

## Alternativas consideradas

- **Árvore única com pai reserva (o rascunho anterior):** congela muita gente quando um galho sai, e o peso fica com poucos.
- **Torrent puro (pull de pedaços):** atraso de segundos. Serve para conserto, não para entrega.
- **Servidor de mídia (SFU), como o do Discord:** é o melhor em latência, mas tem custo e operação contínuos e foge da premissa do projeto. Fica só como reserva para quem não alcança ninguém, que é o papel do repassador na nuvem.
- **WebRTC:** não traz repasse em árvore; o QUIC próprio com a semântica do MoQ dá controle total do repasse e encaixa num repassador MoQ.
