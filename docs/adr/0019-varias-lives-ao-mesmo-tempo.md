# 0019. Várias lives ao mesmo tempo, cada uma num quadro da sala

- **Status:** aceita
- **Data:** 2026-10-21

## Contexto

Até a 0.8, assistir uma live trocava a sala inteira por uma tela só daquela live, com uma barra
embaixo, e só dava para assistir **uma** por vez. A nossa própria live já virou um quadro da sala,
como no Discord, e o Kaique quer o mesmo para a live de quem assistimos, com a possibilidade de
assistir várias ao mesmo tempo.

Abaixo do app quase nada impede isso: cada `RemoteView` (`sv-session`) já tem a própria conexão
QUIC, o próprio decodificador, a própria janela nativa e a própria saída de som. A suposição de
"uma live só" mora no estado do app em Rust (`watching: Option`), na posição única do vídeo
(`set_video_bounds`), nos eventos do vídeo sem dizer de qual live vêm, na sala (`pending_watch`,
`relay_source` únicos) e no protocolo da sala (cada membro anuncia **um** `watching`).

## Decisão

Cada live assistida vira um quadro na grade da sala, com a mesma cara do nosso (qualidade e AO
VIVO no canto, foco, tela cheia, menu no botão direito e nos "...", detalhes em gráficos), e o app
guarda as lives assistidas num mapa por pessoa: cada uma com a sua janela nativa, a sua posição, o
seu volume e as suas medições. No protocolo da sala, o `status` ganha `watchingAll` (a lista de
quem assistimos), e o `watching` de antes continua levando o primeiro id, para quem tem o app
antigo.

## Consequências

- Assistir não esconde mais a sala: a live aparece como mais um quadro, e as outras pessoas
  continuam na grade (ou na faixa de baixo, com a live em foco).
- Comandos de assistir passam a dizer **de quem** (`room_stop_watching`, volume, som, picture in
  picture), e os eventos do vídeo (`video-pointer`, `video-fullscreen`, `video-pip`, `watch-stats`)
  passam a dizer de qual live vêm. A posição de cada vídeo vai pelo id dele; os recortes dos menus
  continuam valendo para todos (cada vídeo já ignora os que não caem sobre ele).
- A sala aceita vários pedidos de assistir ao mesmo tempo e pode repassar mais de uma live a quem
  não alcança o streamer direto (ADR 0014), cada uma com o seu repassador.
- Custo: cada live a mais é mais uma decodificação na GPU e mais um fluxo de vídeo chegando (o
  upload de quem transmite não muda: é uma conexão por quem assiste, como já era). Por isso cada
  pessoa assiste **no máximo 4 lives ao mesmo tempo** (o Worker recusa um `watchingAll` maior e o
  app avisa antes de pedir a quinta); a medição de várias lives juntas entra na `perf-check`.
- Compatibilidade: o Worker publicado aceita o `watchingAll` novo e repassa os dois campos; apps
  antigos ignoram o campo novo e continuam vendo só o primeiro. Publicar o Worker antes do app.
- Picture in picture continua sendo de uma live por vez (soltar outra traz a anterior de volta).

## Alternativas consideradas

- **Manter a tela de assistir só para uma live e pôr as outras como quadros:** duas maneiras de
  mostrar a mesma coisa, e a barra de baixo continuaria escondendo a sala.
- **Trocar o `watching` por uma lista, sem manter o campo antigo:** quem estivesse no app antigo
  deixaria de ver quem assiste a live dele até atualizar.
- **Uma janela nativa só, desenhando várias lives nela (mosaico no Rust):** recortes, foco e menus
  por live ficariam muito mais difíceis, e cada `RemoteView` já tem a sua janela.
