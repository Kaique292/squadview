# 0015. Efeito de fala escolhido por cada um, enviado no status da sala

- **Status:** aceita
- **Data:** 2026-10-21

## Contexto

Quem fala na sala ganha um anel verde em volta da foto, na lista de membros e nos cards da voz.
O Kaique quer que cada pessoa possa trocar esse anel por um efeito animado (esfera de pontos,
ondas de linhas, equalizador ou raios), na cor que escolher, e que **todos da sala vejam o
efeito de quem está falando**, não só ela. Para isso a escolha precisa chegar aos outros apps.

## Decisão

O efeito vai como um campo opcional `effect: { kind, color } | null` na mensagem `status` da
sinalização, a mesma que já leva ao vivo, mudo, surdo e quem assistimos. O Worker guarda o
efeito no membro e o repassa em `member-updated` e no `welcome`. O desenho é feito pela
interface de cada app, com SVG animado só por CSS.

## Consequências

- Nenhuma mensagem nova e nenhuma mudança na malha P2P: o efeito não passa pelo QUIC nem pela
  voz, e a versão do protocolo da sinalização continua 1.
- Compatível nos dois sentidos: apps até o 0.5.0 não mandam o campo (vira `null`) e ignoram o
  que recebem; o Worker antigo descarta o campo sem recusar o `status`.
- Um efeito fora do formato ou de um tipo que o Worker não conhece vira `null` (o anel padrão),
  sem derrubar a conexão. No app, `kind` é lido como texto: um efeito novo, de uma versão mais
  nova, não estraga a leitura do membro; a interface desenha o anel padrão no lugar.
- A cor só aceita `#rrggbb`: nada de CSS arbitrário vindo de outro membro.
- Custo: o efeito é montado uma vez e só se animam camadas inteiras (cada `<svg>` gira, pulsa ou
  respira), com transform e opacity, que a GPU faz sem redesenhar; parado e pausado quando a
  pessoa não fala. Animar o que está _dentro_ do SVG obriga o Chromium a redesenhar a cada
  quadro: medido com 8 pessoas falando, custava até ~40% da thread da interface; com camadas,
  fica entre ~0,4% e ~3,5%. Não toca em captura, encode, rede de mídia nem no render do vídeo.
- Trocar de efeito manda um `status`, que conta no limite de mensagens por conexão; como só
  acontece ao clicar em "Aplicar", não chega perto do limite.

## Alternativas consideradas

- **Só no meu app (cada um escolhe como vê todo mundo):** mais simples, sem mexer no protocolo,
  mas não atende ao pedido: o efeito é uma forma de cada pessoa se mostrar para a sala.
- **Mensagem nova (`effect`) na sinalização:** separaria o assunto, mas o Worker atual responde
  `invalid-message` e fecha a conexão para tipos que não conhece; apps novos cairiam da sala
  enquanto o Worker não fosse atualizado.
- **Mandar pela malha P2P (sinal entre os membros):** a sala já tem um lugar central para o
  estado de cada membro; repetir isso entre pares só criaria mais caminhos para ficar dessincronizado.
