# Design da interface

A interface segue a **hierarquia** do Discord, porque é onde a galera já vive, com identidade própria (ver [ADR 0005](../adr/0005-ui-estilo-discord.md)). Por cima dessa estrutura, a sala é **imersiva**: reage a quem está nela, com luz ambiente, profundidade e movimento com física ([ADR 0034](../adr/0034-redesign-imersivo.md), [protótipo](https://claude.ai/artifact/Ptg5mWVMsUzTHSSDjBqCna)). Os tokens em código ficam em `apps/desktop/src/styles/index.css`.

## Princípios

As leis de UX que guiam cada tela, e o que elas querem dizer aqui:

- **Jakob (o familiar):** a estrutura é a do Discord. O que surpreende é a superfície e o movimento, nunca onde as coisas ficam.
- **Fitts (alvos grandes e perto):** os controles da chamada ficam juntos numa doca embaixo, com 52 px de alvo; a ação principal de cada tela é o maior botão dela.
- **Hick e divulgação progressiva (poucas escolhas):** cada tela tem uma ação principal. Opções avançadas (qualidade, dispositivos) ficam atrás de "Ajustar…" ou de menus.
- **Miller (blocos pequenos):** listas e menus em grupos de até 7 itens, com cabeçalhos de seção.
- **Doherty (resposta em menos de 400 ms):** todo clique responde na hora (o botão afunda); o que demora mostra esqueleto ou progresso.
- **Gradiente de objetivo:** processos com etapas (entrar na sala, baixar um modelo, atualizar) mostram a barra e as etapas que já foram.
- **Usabilidade estética:** o visual é parte da função; mas um efeito nunca esconde o estado da sala (quem fala, quem está ao vivo, a conexão).
- **Gestalt:** o que é do mesmo grupo divide a mesma superfície (proximidade e região comum); o que tem a mesma função tem a mesma cara (semelhança).
- **Heurísticas de Nielsen:** estado sempre visível, sair e desfazer fácil (parar a transmissão, sair da sala), erros com explicação e ação, e o mínimo de texto e de opção por tela.

## Layout

```
┌────┬──────────────┬─────────────────────────────────────────┐
│ SV │ Nome da sala │  Grade de streams                       │
│    │──────────────│  ┌───────────────┐ ┌───────────────┐    │
│ 🟣 │ 🔴 Ana  AO VIVO│ │  Ana (1080p60) │ │ Beto (720p30) │    │
│ 🟢 │ ⚪ Beto        │ └───────────────┘ └───────────────┘    │
│ ＋ │ ⚪ Caio        │                                         │
│    │──────────────│  [ Transmitir minha tela ]              │
│    │ 👤 Você   ⚙  │                                         │
└────┴──────────────┴─────────────────────────────────────────┘
 72px     240px                    flexível
```

- **Barra de salas (72 px):** ícones redondos que viram quadrados arredondados no hover; o botão ＋ cria sala.
- **Barra de título (32 px):** própria (janela sem moldura do Windows), na cor da barra de salas, arrastável, com o nome da sala no meio e os botões minimizar, maximizar e fechar.
- **Barra lateral (240 px):** canto superior esquerdo arredondado e borda fina, como no Discord; nome da sala e a sala como "canal de voz" com o tempo conectado e os membros.
- **Card do usuário:** flutua no pé da barra de salas e da lateral (largura das duas), com "Conectado à sala" em cima e o usuário embaixo.
- **Área principal:** grade de streams; clicar num stream expande para foco, com os outros em miniatura.

## Cores

| Token           | Valor     | Uso                                     |
| --------------- | --------- | --------------------------------------- |
| `surface-float` | `#111214` | Menus e dicas                           |
| `surface-0`     | `#1e1f22` | Barra de título, barra de salas, inputs |
| `surface-1`     | `#2b2d31` | Barra lateral, cards em modais          |
| `surface-panel` | `#232428` | Card flutuante do usuário               |
| `surface-2`     | `#313338` | Área principal, modais                  |
| `surface-3`     | `#383a40` | Botões neutros                          |
| `surface-hover` | `#404249` | Hover de itens                          |
| `text`          | `#f2f3f5` | Texto principal                         |
| `text-muted`    | `#b5bac1` | Texto secundário                        |
| `text-faint`    | `#80848e` | Dicas, placeholders                     |
| `brand`         | `#6e4ff6` | Ações principais, marca                 |
| `live`          | `#f23f43` | Selo AO VIVO                            |
| `online`        | `#23a55a` | Conectado, criar sala                   |
| `warning`       | `#f0b232` | Conexão instável                        |

Contraste mínimo: 4,5:1 para texto normal (WCAG AA).

### Vidro

Em Configurações → Aparência, o "Efeito de vidro" (0–100%, padrão 40%) deixa a estrutura translúcida: título e barra de salas, lateral, área principal, card do usuário e modais usam os tokens `glass-*`, que misturam as cores acima com transparência conforme `--glass`. Acima de 0%, a janela liga o Acrylic do Windows, e o desfoque do que está atrás fica a cargo do DWM, fora do processo do app. Em 0% tudo fica sólido, igual às cores da tabela. A escolha fica salva em `localStorage` (`squadview.vidro`).

## Tipografia

- Fonte: **Noto Sans** (livre, embutida via `@fontsource-variable/noto-sans`), a mesma que o Discord usa de reserva quando a gg sans dele não carrega. A gg sans e a ABC Ginto são proprietárias e não entram no app. Segoe UI Variable fica de reserva.
- Aplicação no padrão do Discord:
  - Cabeçalhos de seção ("SALAS DE VOZ", "NA SALA"): 12 px, maiúsculas, peso 600, espaçamento 0,02em (`text-xs font-semibold tracking-wide uppercase`).
  - Nomes de salas e itens de navegação: 16 px, peso 500.
  - Nomes de quem está numa sala e de conversas: 14 px, peso 500.
  - Corpo e descrições: 14 px, peso 400; textos de apoio em 12 px.
  - Títulos de tela e de modal: 20–24 px, peso 700.
- Texto com `-webkit-font-smoothing: antialiased` e `text-rendering: optimizeLegibility`, como no Discord.

## Componentes

- Primitivos acessíveis do **Radix UI** (Dialog, DropdownMenu, Tooltip, Slider, Switch) estilizados com Tailwind.
- Ícones **lucide-react**, 16–22 px.
- **Checkbox:** sempre `apps/desktop/src/ui/Checkbox.tsx`. Use `Checkbox` (com rótulo) em formulários e modais, e `CheckMark` (só a caixa) dentro dos `CheckboxItem` dos menus. Nunca `<input type="checkbox">` nativo nem caixa desenhada à mão. Ao marcar, a caixa enche com um pulo e o ✓ se desenha; ao desmarcar, o ✓ se apaga e a caixa esvazia.
- Raio: 4 px (selos), 8 px (botões pequenos e itens de lista), 12 px (cards), 14 px (botões da doca), 16 px (ícones de sala e quadros da sala), 18 px (live em foco), 24 px (modais e cenas).
- Animações seguem os tokens de [Movimento](#movimento).

## Estados que toda tela precisa ter

- Vazio (ninguém transmitindo).
- Carregando (conectando à sala).
- Erro com explicação e ação ("Não deu para alcançar a Ana. Tentando por outro membro…").
- Conexão instável (selo amarelo com a camada atual).

## Acessibilidade

- Tudo navegável pelo teclado, com foco visível.
- Rótulos `aria-label` em botões que só têm ícone.
- Nada transmitido só por cor: o selo AO VIVO tem texto.

## Movimento

- Todo modal usa o componente `apps/desktop/src/ui/Modal.tsx`, que já traz as animações. Não crie modais com o Radix direto.
- **Abrir:** fundo escurece em 180 ms; a caixa sobe 12 px e cresce de 96% para 100% em 220 ms (`cubic-bezier(0.16, 1, 0.3, 1)`).
- **Fechar:** mais rápido (140 ms), no sentido inverso. O Radix só remove o modal depois que a animação termina.
- **Trocar de passo dentro do modal** (ex.: fonte → qualidade): o conteúdo novo entra deslizando 16 px da direita (`animate-step-in`), e a largura do modal se ajusta com transição.
- Quem ativou "Mostrar animações" desligado no Windows (`prefers-reduced-motion`) não vê animação nenhuma: as mudanças acontecem direto.

### Tokens

| Token          | Valor                               | Uso                                                       |
| -------------- | ----------------------------------- | --------------------------------------------------------- |
| `ease-out`     | `cubic-bezier(0.16, 1, 0.3, 1)`     | Entradas, trocas de cor e de luz                          |
| `ease-spring`  | `cubic-bezier(0.34, 1.56, 0.64, 1)` | Quadros, botões e foco: passa um pouco do ponto e assenta |
| `dur-resposta` | 120–160 ms                          | Hover, cor de fundo, o botão que afunda                   |
| `dur-mola`     | 360–520 ms                          | Quadros que se movem, troca de foco, botões que sobem     |
| `dur-cena`     | 600–900 ms                          | Luz ambiente, entrada na sala, ficar ao vivo              |

- **Clique:** o alvo afunda para 92–97% (`ease-spring`), e volta ao soltar.
- **Hover:** quadros e botões sobem 3–4 px.
- **Troca de foco:** a live nova entra de 103% para 100% com opacidade, e a luz ambiente troca de cor em `dur-cena`.

### O que pode se mover

- **Só `transform` e `opacity`.** Nada de animar `box-shadow`, `filter`, `backdrop-filter`, `width`, `height`, `top` ou `left`: obrigam o navegador a refazer pintura ou layout a cada quadro. Um anel que pulsa é um elemento próprio que muda de escala e opacidade, não uma sombra animada.
- **Contínuo só com o app em uso.** O que se repete (a luz ambiente que flutua, o anel de quem fala, o ponto do AO VIVO, o carregamento das miniaturas) pausa com a janela minimizada, sem foco ou escondida atrás do jogo. Sem isso, o efeito não entra.
- **Medido:** mudança que adiciona efeito na sala passa pela skill `perf-check`, com o FPS do jogo antes e depois.

## Imersão

- **Luz ambiente:** a live de alguém em foco aparece de novo, minúscula (32 × 18) e esticada, atrás de toda a sala, e as cores dela vazam em volta do quadro e da faixa. Quem desenha é o renderizador da live, por baixo da interface ([ADR 0021](../adr/0021-video-por-baixo-da-interface.md)), sem ler a imagem para a CPU.
  - Liga só com a live já com imagem, fora do picture in picture e com o app em uso; fora disso, a sala é o preto de sempre (`voice/ambient-light.ts`).
  - A imagem da luz é atualizada a cada 6 quadros da live (10 vezes por segundo a 60 fps): para uma luz difusa, parece contínuo, e custa menos de 1 ponto de GPU (medido com `crates/sv-media/examples/luz_ambiente.rs app`).
  - A sala inteira vira uma área transparente da página (chave `luz-ambiente` em `ui/see-through.ts`), e a vinheta (`.ambient-vignette`, um `Backdrop` que só abre buraco nas lives) escurece por cima: 42% no meio, 88% nas bordas.
- **Brilho da live em foco:** uma sombra larga na cor da live sob o quadro em foco, que muda junto com a luz.
- **Vidro sobre a live:** o topo da live (de quem é, quem assiste, a qualidade), a doca de controles e os menus são de vidro: fundo escuro translúcido e borda de 1 px branca a 6–8%. Sobre o vídeo não há desfoque: o `backdrop-filter` não alcança o vídeo, que fica por baixo da página. Fora da live (lateral, modais), o desfoque de 14–18 px fica só nas superfícies pequenas, nunca na área inteira.
- **Quem fala:** a borda do quadro fica na cor do efeito de fala da pessoa (ou verde, `online`), e um brilho interno na mesma cor aparece e encolhe enquanto some, a cada 1,1 s (`.speak-pulse`). O pulso é para dentro porque o quadro corta o que passa da borda; só a opacidade e a escala se animam, ele pausa com o app fora de uso (`loop`) e some com "reduzir movimento" (a borda fica).
- **Doca e topo da live:** de vidro (`glass-live`). Os cantos da barra da chamada dividem o espaço que sobra: com folga, a doca fica no meio; na janela estreita, ela anda para o lado em vez de passar por baixo dos botões do canto.
- **Momentos:**
  - **Entrar na sala:** com a sala encontrada, uma cena por cima da sala (`room/EntryScene.tsx`) mostra quem já está lá e uma barra com as etapas de verdade: "Sala encontrada", "Conectando direto com cada um… x de y" (contando quem já está em `voice-peers`) e "Pronto". Sozinho, fica pronta na hora; se alguém demorar, fica pronta em 6 s com quem deu. Some sozinha 1,4 s depois de pronta, e um clique ou o Esc a fecham na hora. A cena `#demo=entrando` mostra a entrada no meio.
  - **Ficar ao vivo:** a live começa no instante do clique, sem contagem: uma contagem de 3 s, como no protótipo, atrasaria a resposta ao clique (limiar de Doherty). O momento é a entrada no ar: o selo AO VIVO entra com mola e uma onda vermelha percorre a borda do quadro da nossa live e some para dentro (`.live-wave`), uma vez. A onda só toca na montagem logo depois de entrar no ar (`liveSince` no `stream-store`, até 1,5 s), nunca ao trocar de fonte nem ao mover o quadro entre grade, foco e faixa. Com "reduzir movimento", não aparece.
  - **Alguém começou a assistir:** um aviso com a foto da pessoa, "Fulano começou a assistir" (`stream/viewer-notices.ts`), vindo da lista real de quem assiste. Quem sai não gera aviso; mais de 3 de uma vez viram um aviso só.
  - Nenhum momento bloqueia: dá para clicar em tudo enquanto ele acontece.

## Rolagem

- Toda área com rolagem usa `apps/desktop/src/ui/ScrollArea.tsx`, nunca `overflow-auto` com a barra nativa.
- A barra é sobreposta: fica por cima do conteúdo e não ocupa espaço. Tem 10 px de área de clique e um polegar de 6 px, arredondado, na cor `surface-0`.
- Aparece com o mouse em cima ou ao rolar (fade de 150 ms) e some 500 ms depois que o mouse sai (fade de 300 ms).

## Modal "Transmitir minha tela"

Segue o fluxo de dois passos do compartilhamento do Discord, com arte e textos próprios:

1. **Escolher a fonte:** abas "Aplicativos" (janelas) e "Telas" (monitores), com grade de miniaturas. Clicar num card vai para o passo 2. "Ficar ao vivo" fica desabilitado até escolher.
2. **Qualidade:** "O que você está transmitindo" (com "Trocar"), "Transmitindo para" e a qualidade, com resolução 720p / 1080p / 1440p / Fonte e taxa de quadros 15 / 30 / 60. **Tudo liberado, sem plano pago.** Embaixo, o teto estimado de upload por pessoa assistindo.
