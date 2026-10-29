# 0030. Malha na rede da sala: quem transmite coordena, faixas pela conexão direta e pelos pais

- **Status:** proposta
- **Data:** 2026-10-25

## Contexto

O planejador ([ADR 0012](0012-malha-em-enxame.md)), as faixas ([ADR 0028](0028-faixas-e-paridade-no-envio.md)) e a troca de pai ([ADR 0029](0029-troca-de-pai-na-malha.md)) existiam, mas a sala ainda mandava o vídeo inteiro de quem transmite para cada um. Numa sala de 8, quem transmite subia 7 cópias (56 Mbps em 1080p60), quase sempre mais do que a fibra de quem joga aguenta. Faltava a rede: quem decide o plano, como cada um sabe de onde vem cada faixa, como os membros se conectam entre si e o que acontece com apps antigos.

## Decisão

**Quem transmite coordena a malha da sua live**, e os membros recebem o plano pela sinalização.

- **Quem entra:** quem pede para assistir com o app novo manda no `watch-request` o NAT e o upload que pode doar (`mesh`). Quem transmite responde `mesh: true` no `watch-accept`. Apps antigos não mandam o campo e recebem o vídeo inteiro, como antes. Quem vem por um membro que repassa ([ADR 0014](0014-conexao-em-camadas.md)) também fica fora da malha.
- **A conexão direta continua:** o som, o relógio e os pedidos de keyframe seguem na conexão com quem transmite. Nela, o vídeo vem em faixas, só as que o plano manda direto (`Broadcaster::add_mesh`, com uma máscara de faixas que o coordenador muda a cada plano). O resto vem dos pais.
- **O plano:** quem entra refaz o plano (`sv_mesh::plan`); quem sai ou perde uma faixa é consertado só onde quebrou (`repair_with`). Cada membro afetado recebe um `mesh-assign` com o plano inteiro dele: de quem vem cada faixa, com os endereços e a fingerprint do pai, a faixa da casa e os filhos. Quem transmite já tem tudo isso dos pedidos para assistir, então não há ida e volta a mais.
- **Conexões entre membros:** o filho conecta no pai com o ALPN `squadview-mesh/1`. O pai autoriza e "fura" o NAT na direção dos filhos ao receber o plano. Pela conexão, o pai manda as faixas da casa que chegam a ele, cada filho com a sua fila.
- **Pai perdido:** o filho avisa quem transmite (`mesh-lost`) quando a conexão com o pai fecha, quando não abre depois de 3 tentativas, ou quando passam 300 ms sem faixa dele enquanto as outras chegam. Com a tela parada, nenhuma faixa chega de ninguém, e ninguém é dado como perdido. O conserto não volta para um par que falhou.
- **Quem recebe tudo direto** (sala de 1 ou 2 assistindo, ou quem não coube no plano) recebe só as 4 faixas de dados: num caminho só, a paridade não protege nada, e assim custa a quem transmite o mesmo que sem a malha.
- **Estimativas, por enquanto:** sem medição, cada membro anuncia 10 Mbps de upload, quem transmite conta como 30 Mbps (o planejador limita a 2 cópias de qualquer jeito) e a latência entre membros conta como 30 ms. NAT desconhecido conta como NAT comum; se a conexão não abrir, o conserto troca o pai. Quem transmite conta como rede aberta, porque quem entra na malha já está conectado direto nele.
- **Simulcast** ([ADR 0026](0026-simulcast-com-2-camadas.md)): os pais só repassam a camada principal. Quem recebe tudo direto continua trocando de camada como antes, com a camada baixa também em faixas (o `StripeHeader` diz a camada, e quem recebe recomeça a remontagem no keyframe dela). Quem tem pais fica preso na principal enquanto tiver.

## Consequências

- Numa sala de 4 ou mais, quem transmite manda no máximo umas 2 cópias do vídeo, e os outros repassam. O teste da sala com 4 assistindo mostra o repasse acontecendo e a saída de um pai sem ninguém parar de receber.
- Cada membro com filhos sobe faixas para eles. O limite vem do upload anunciado, que ainda é uma estimativa: quem tem menos do que isso pode atrasar os filhos até o conserto trocar o pai. A medição de upload real é o próximo passo.
- O áudio não usa a malha: cada um recebe o som do jogo direto de quem transmite (128 kbit/s por pessoa), o que fica bem abaixo do vídeo.
- Quem transmite vira o ponto único da coordenação da sua live. Se ele cai, a live acaba de qualquer jeito.
- Os números de saída e troca de pai simulados na ADR 0029 ainda precisam ser medidos com gente de verdade, em redes diferentes.

## Alternativas consideradas

- **Cada membro planeja sozinho com o que vê:** dispensa o coordenador, mas cada um só conhece os vizinhos, e dois filhos podem lotar o mesmo pai. Quem transmite já tem o plano inteiro.
- **Mandar o plano por mensagens entre membros (gossip):** necessário para milhares de pessoas, como prevê a ADR 0012, mas para salas de amigos a sinalização dá conta e é mais simples.
- **Tirar a conexão direta e mandar tudo pela malha (som, relógio, keyframes):** reduziria ainda mais o upload de quem transmite, mas exigiria repassar o controle e o relógio pelas árvores. O som é pequeno, e manter a conexão direta deixa a malha só com o que pesa.
- **Esperar a medição de upload antes de ligar a malha:** atrasaria o ganho maior (sair de 7 cópias para 2) por uma estimativa que o conserto já corrige quando um pai não dá conta.
