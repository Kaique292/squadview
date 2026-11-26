# 0034. Redesign imersivo: a sala reage a quem está nela

- **Status:** aceita
- **Data:** 2026-11-26

## Contexto

A interface segue a hierarquia do Discord ([ADR 0005](0005-ui-estilo-discord.md)): barra de salas, lateral, sala no centro e controles embaixo. Ela é clara e familiar, mas é plana. Uma sala com três amigos ao vivo parece igual a uma sala vazia, e os momentos que importam (entrar, ficar ao vivo, alguém começar a assistir) passam sem nenhum destaque.

Queremos um visual que dê vontade de mostrar para os amigos, sem perder a familiaridade (lei de Jakob) nem pesar no jogo. O app roda ao lado de jogos, então uma animação que custa FPS é um defeito, por mais bonita que seja.

O [protótipo](https://claude.ai/artifact/Ptg5mWVMsUzTHSSDjBqCna) mostra a direção em três telas: a sala ao vivo, a entrada na sala e a transmissão começando.

## Decisão

A estrutura do Discord continua. O que muda é a superfície e o movimento, em quatro pilares:

1. **Luz ambiente:** a live em foco "vaza" as cores dela pela sala, num fundo atrás de tudo. A luz vem do próprio vídeo: o renderizador da live em foco ([ADR 0021](0021-video-por-baixo-da-interface.md)) também desenha uma imagem minúscula dela, de 32 × 18, num swap chain próprio, e o compositor do Windows a estica até a sala, com suavização. Tudo fica na GPU: o `VideoProcessor` reduz o quadro para 256 × 144, o `GenerateMips` tira a média até 32 × 18, e nada volta para a CPU. A página põe por cima uma vinheta escura estática, que some com o contorno da imagem esticada.
2. **Profundidade:** camadas de vidro sobre a live e a luz ambiente: topo da live, doca de controles, lateral e modais. Sobre o vídeo, o vidro é só um escurecido translúcido, sem desfoque: o `backdrop-filter` da página não alcança o vídeo, que fica por baixo dela (ADR 0021).
3. **Movimento com física:** transições com mola (um leve passar do ponto antes de assentar) em quadros, botões e trocas de foco, sempre em resposta a uma ação, e com resposta em menos de 400 ms (limiar de Doherty).
4. **Momentos marcantes:** entrar na sala (progresso por etapas), ficar ao vivo (contagem e onda de entrada) e alguém começar a assistir ganham uma cena própria, curta e que não bloqueia o uso.

Regras que valem para todo efeito:

- **Só o que a GPU compõe sozinha:** `transform` e `opacity`. Nada de animar `box-shadow`, `filter`, `width`, `height` ou posição, que obrigam o navegador a refazer layout ou pintura a cada quadro.
- **Contínuo só com o app em uso:** efeitos que se repetem (luz ambiente que flutua, anel de quem fala, pulso do AO VIVO) pausam com a janela minimizada, sem foco ou escondida atrás do jogo.
- **Reduzir movimento:** com a opção do Windows (`prefers-reduced-motion`), nenhum efeito se move; as mudanças acontecem direto.
- **Medido:** cada etapa do redesign que mexe na sala é medida com a skill `perf-check`, com o FPS do jogo com o app aberto antes e depois. Um efeito que não passa não entra.

## Consequências

- **Fica mais fácil:** o app ganha identidade própria além do "parecido com o Discord", e o estado da sala (quem fala, quem está ao vivo, a qualidade da conexão) fica visível de relance.
- **Fica mais difícil:**
  - toda tela nova precisa seguir os tokens de movimento e profundidade do `docs/design/ui.md`, e cada efeito contínuo precisa da pausa;
  - o vidro com desfoque (`backdrop-filter`) custa GPU no WebView2: fica só nas superfícies pequenas (topo da live, doca, menus), nunca na área inteira.
- **A regra antiga muda:** "nada de animação contínua enquanto alguém joga" vira "nada de animação contínua sem o app em uso".
- **Implementação em etapas,** um PR por tela, com a medição de FPS em cada um:
  1. Tokens de movimento e profundidade, e a pausa dos efeitos contínuos.
  2. Sala ao vivo: luz ambiente, quem fala e a doca de vidro.
  3. Entrada na sala, com o progresso por etapas.
  4. Ficar ao vivo, com a contagem e o aviso de quem começou a assistir.

## Alternativas consideradas

- **Manter o visual atual e só polir:** seguro, mas não muda a percepção do app, que é o objetivo.
- **Trocar a estrutura (navegação nova, layout próprio):** daria mais personalidade, mas quebra a lei de Jakob: a galera já sabe usar a estrutura do Discord.
- **Desfoque do próprio compositor para a luz ambiente (`IDCompositionGaussianBlurEffect`, com o mesmo swap chain da live num segundo visual):** fica lindo, mas medido com `crates/sv-media/examples/luz_ambiente.rs` o compositor do Windows foi de 0,6% para 16% da GPU, e reduzir o visual antes de desfocar não mudou nada (ele desfoca na resolução da tela). A imagem minúscula esticada ficou dentro do ruído da medição.
- **Efeitos em WebGL (shaders) para a luz ambiente:** mais ricos, mas ocupam a GPU que o jogo e o encoder usam, e o mesmo efeito sai em CSS com uma camada desfocada parada.
- **Ler a cor média da live na CPU e pintar a luz em CSS:** simples na interface, mas exige copiar a imagem da GPU para a CPU a cada amostra, o que o CLAUDE.md proíbe. Desenhar a live pequena e desfocada na própria GPU dá o mesmo efeito, mais fiel, sem a cópia.
- **Cores fixas por live (como no protótipo):** baratas, mas não acompanham o jogo. Ficam só como reserva enquanto a primeira imagem não chega.
