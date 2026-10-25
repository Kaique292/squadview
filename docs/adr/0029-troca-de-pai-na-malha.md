# 0029. Troca de pai na malha: conserto local, só para quem perdeu uma faixa

- **Status:** proposta
- **Data:** 2026-10-24

## Contexto

Na malha em enxame ([ADR 0012](0012-malha-em-enxame.md)), cada membro recebe cada faixa de um pai numa árvore e repassa só na sua faixa da casa. Quando alguém sai, quem recebia dele perde uma faixa. A paridade ([ADR 0028](0028-faixas-e-paridade-no-envio.md)) segura a imagem enquanto faltar só uma, mas a faixa precisa voltar logo: a meta da Fase 3 é a troca de pai em menos de 1 s, sem congelar ninguém.

Refazer o plano inteiro a cada saída mexeria em todo mundo: gente que estava bem trocaria de pai, abriria conexões novas e arriscaria travar sem motivo.

## Decisão

Quando alguém sai, o plano é **consertado só onde quebrou** (`sv_mesh::plan::repair`):

- As ligações cujo pai continua na sala ficam como estão.
- Cada ligação órfã ganha um pai novo: quem transmite, se ainda tem cota naquela árvore, ou alguém do tronco da mesma árvore com vaga, pelo menor atraso. **Nunca alguém abaixo do órfão**, que formaria um ciclo e a faixa nunca chegaria. Os órfãos do tronco são consertados antes das folhas, porque os filhos deles dependem disso.
- Quem não acha pai fica sem aquela faixa. Com a paridade, a imagem continua enquanto faltar só uma; com duas faltando, o membro desce de camada no próximo plano.
- `sv_mesh::plan::changes` compara os dois planos e devolve só as ligações que mudaram. É o que precisa ser avisado a cada membro.

Na rede (o próximo passo, ainda não feito):

- **Quem coordena:** quem transmite. Ele tem o plano e recebe da sinalização quem entra e sai da sala.
- **Perceber a saída:** quem sai avisa (`Bye`). Quem cai sem avisar é percebido pelo filho: 300 ms sem nenhuma faixa daquele pai, ou a conexão fechada. O filho avisa quem transmite.
- **Avisar:** quem transmite manda a cada membro afetado só a sua `Change` (árvore e pai novo), pela sinalização. O membro conecta no pai novo (ALPN própria da malha) enquanto a paridade cobre a faixa que falta.
- **Medição:** o planejador precisa de upload, download e NAT de cada membro. O NAT já é conhecido (`Peer::nat`). Upload e download começam com uma estimativa conservadora e são corrigidos pela taxa de entrega real das conexões. Essa parte vem com o passo da rede.

## Números do simulador

`cargo run -p sv-mesh --release --example simular`, malha com o repassador na nuvem de reserva, 5% saindo de uma vez. A troca é estimada como 300 ms para perceber a saída, mais um aperto de mão QUIC e a primeira faixa (1,5 RTT até o pai novo).

| Membros | Mudam de pai | Continuam com imagem | Troca de pai p95 |
| ------- | ------------ | -------------------- | ---------------- |
| 1.000   | 14,9%        | 100%                 | 390 ms           |
| 5.000   | 21,7%        | 100%                 | 372 ms           |

Mudam de pai exatamente os que perderam uma faixa com a saída (a coluna "cobre" e "congela" do simulador). Em salas de até 100 pessoas, com essa semente ninguém saiu. Num cenário de 8 pessoas em que sai quem estava no tronco, todos trocam de pai naquela árvore (são todos filhos dele), e a troca fica em 354 ms no p95.

## Consequências

- Uma saída mexe só em quem dependia de quem saiu. A imagem não congela (a paridade cobre) e a faixa volta em menos de meio segundo no p95 simulado.
- Consertos seguidos podem deixar as árvores menos equilibradas que um plano novo. Um replanejamento completo, gradual, continua previsto para quando alguém entra ou a medição muda muito (ADR 0012).
- O tempo de 300 ms para perceber a saída é um palpite: curto demais troca de pai por uma oscilação da rede, longo demais deixa o membro mais tempo só com a paridade. Precisa de medição no app real.
- Quem transmite vira o coordenador da sua live. Numa sala com várias lives, cada um coordena a sua.

## Alternativas consideradas

- **Refazer o plano inteiro a cada saída:** mais equilibrado, mas troca o pai de quem estava bem e multiplica as conexões novas a cada saída.
- **Pai reserva fixo para cada membro (o rascunho antigo):** troca mais rápida, mas reserva upload que fica parado e não resolve a saída do próprio reserva.
- **O filho escolhe o pai novo sozinho:** dispensa o coordenador, mas cada filho só vê os vizinhos e pode lotar o mesmo pai. Quem transmite já tem o plano inteiro.
