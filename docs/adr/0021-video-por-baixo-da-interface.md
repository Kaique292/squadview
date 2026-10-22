# 0021. Vídeo desenhado por baixo da interface (DirectComposition)

- **Status:** aceita
- **Data:** 2026-10-22

## Contexto

O vídeo (a nossa live e as lives que assistimos) é uma **janela nativa filha** (`WS_CHILD`), colada por cima da WebView2 na área do quadro (`crates/sv-media/src/window.rs`). Uma janela nativa sempre cobre a página. Para um selo ou um menu aparecer por cima da live, o app recorta um buraco nela (`SetWindowRgn`), e pelo buraco aparece **a página**, não o vídeo. Embaixo do vídeo, a página é só fundo preto.

Daí vêm os defeitos que as correções anteriores não resolveram, porque ajustavam o buraco e não a causa (três tentativas no menu, duas nos selos):

- **Selos e nome:** o fundo semitransparente fica preto sólido em vez de deixar ver o vídeo.
- **Cantos arredondados:** o recorte do Windows é em degraus e a pílula é suavizada; onde não bate, aparece preto.
- **Menus:** a animação de abrir (de transparente e 95% do tamanho) e a de fechar (sumindo) deixam o menu translúcido sobre o preto do buraco, a "moldura preta". O recorte também é aplicado por outro caminho (o Windows), um quadro atrasado em relação à página, e pisca.
- **Sombras:** ficam embaixo do vídeo e não aparecem.
- **Imagem fora de 16:9:** a janela do vídeo cobre o quadro inteiro e pinta de preto as barras do enquadramento. Com o fundo da página também preto, o quadro parece cortado.

Para isso funcionar, o app mantém um conjunto de peças:

- recortes: `measureHoles`, `watchVideoHoles`, `data-video-hole`, `set_video_holes`;
- esconder o vídeo quando um modal abre: `holdOverlay`, `set_video_hidden`;
- repasse do mouse do vídeo para a página (`video-pointer`, `TrackMouseEvent`, `handleVideoPointer`), porque o vídeo pega o mouse no lugar da interface.

**Prova (out/2026, no PC do Kaique):** a janela do app é transparente (`"transparent": true`), e o efeito de vidro (acrílico) já aparece através da página. Uma prova separada fez o mesmo com vídeo: wry e WebView2 transparentes, e um swap chain de composição num visual do DirectComposition preso à janela com `topmost = false`, por baixo da janela da WebView2. O resultado:

- os selos semitransparentes, os cantos arredondados, a sombra e o menu no meio da animação se misturaram com o vídeo de verdade;
- não precisou de nenhum recorte nem apareceu preto.

## Decisão

O vídeo passa a ser desenhado **por baixo da interface**, pelo DirectComposition, e não mais numa janela por cima.

- Cada vídeo é um **visual** com um swap chain de composição (`CreateSwapChainForComposition`), numa árvore presa à janela do app (`CreateTargetForHwnd(janela, topmost = false)`).
- A página fica transparente onde está o vídeo e desenha por cima o que quiser: selos, menus, sombras e animações, com mistura de verdade.
- O visual cobre **o quadro inteiro**, com o canto arredondado pelo recorte do próprio DirectComposition (`IDCompositionRectangleClip`). A imagem fica enquadrada na proporção dela, com as barras pretas dentro do quadro arredondado (como no Discord), e não se mistura com o que está atrás da janela.
- A página só fica transparente **enquanto a imagem chega**. Antes disso (e com o vídeo no picture in picture), o quadro continua preto, com o aviso por cima.
- Os fundos da página que ficam atrás de um quadro (a janela, a área principal e a sala) passam a ser uma camada própria (`Backdrop`). Ela pinta a mesma cor de antes, com um buraco (`clip-path`) do tamanho e dos cantos de cada quadro com imagem, 1 pixel para dentro. Assim as bordas suavizadas não deixam ver o que está atrás da janela.
- O **picture in picture** vira uma janela do app (Tauri), pequena, sem borda e sempre no topo. O visual do vídeo passa para a árvore dessa janela, e a página dela, transparente, mostra os controles por cima com o mouse em cima: quem transmite, o AO VIVO, silenciar, volume e voltar para o app. Arrastar move a janela e dois cliques voltam. Os controles só mandam pedidos à janela principal, que guarda o som da live (o mesmo dos menus) e devolve o estado.

## Consequências

- **A interface fica igual.** Os selos (qualidade e AO VIVO), o nome, os "...", os menus, as dicas, as animações, o foco, a tela cheia e o picture in picture continuam como estão. Só some o preto em volta deles, e a sombra dos menus passa a aparecer sobre a live. O picture in picture ganha controles, que faltavam.
- **Modais por cima da live:** ela continua passando por baixo do escurecido do modal, em vez de sumir.

- **Somem os defeitos da live:** o preto nos selos, nos cantos e nos menus, a moldura nas animações, as piscadas e as barras pretas.
- **Some o código de contorno:**
  - os recortes (`measureHoles`, `watchVideoHoles`, `data-video-hole`, `set_video_holes`);
  - o esconder com modal (`holdOverlay` só para o vídeo, `set_video_hidden`);
  - o repasse do mouse (`video-pointer`, `handleVideoPointer`, o `TrackMouseEvent` e as mensagens de mouse da janela).

  O vídeo deixa de ser janela e não pega mais o mouse: clique, botão direito, dois cliques e teclado vão direto para a página.

- **Várias lives** viram vários visuais na mesma árvore, sem uma janela e uma thread de mensagens por live.
- **Ordem na página:** o vídeo fica por baixo de **tudo** da página. Os ancestrais do quadro precisam ser transparentes onde o vídeo aparece. O preto em volta dos quadros na sala vem da própria página (por exemplo, o fundo de cada quadro sem vídeo e um fundo da sala com recorte por máscara), nunca de um fundo opaco por trás do quadro com vídeo.
- **O desfoque de vidro** (`backdrop-filter`) não borra o vídeo: ele só existe dentro da página. Hoje nada desfoca a live; fica como regra não usar desfoque por cima dela.
- **Posição:** a página continua informando onde fica cada quadro (pixels físicos), e o visual acompanha. Num redimensionamento rápido, o vídeo pode ficar um quadro atrasado em relação à página, como já acontece hoje com a janela.
- **Desempenho:** o desenho continua na GPU, com flip model, e quem junta a página e o vídeo é o compositor do Windows, como já é. Antes de publicar, medir com a skill `perf-check` (FPS do jogo e latência com 1 e com 4 lives).
- **Requisito:** DirectComposition existe desde o Windows 8; o app já exige Windows 10 2004 ou mais novo.

## Plano de troca

Em três PRs, cada um com testes e medição. O app continua funcionando entre um e outro.

1. **`sv-media`: a composição** (sem mudar o app ainda).
   - `Composition`: o dispositivo do DirectComposition preso à janela do app, a raiz da árvore e um visual por vídeo, com posição, tamanho, recorte arredondado e visível/escondido.
   - O `Renderer` passa a usar o swap chain de composição; a conversão NV12 continua na GPU. O swap chain tem o tamanho do quadro, com a imagem enquadrada dentro.
   - Picture in picture: soltar e devolver só troca o visual de árvore (para a janela do picture in picture e de volta).
   - Testes: posição e recorte, troca para o picture in picture e volta, vários visuais.
2. **O app usa a composição e a interface perde os contornos.**
   - `sv-session` (`Surface`): a mesma interface de hoje (posição, escondido, picture in picture), agora sobre o visual. Os recortes saem.
   - App em Rust: a composição nasce com a janela principal (`preview.rs`, `room.rs`), e o picture in picture vira a janela `pip` do app (`pip.rs`).
   - Interface:
     - `useVideoSlot` informa a posição e o raio do canto, e se a imagem já está chegando;
     - os fundos atrás dos quadros viram `Backdrop`, transparentes onde há imagem;
     - modais não escondem mais o vídeo;
     - a página da janelinha (`index.html#pip=...`) com os controles;
     - clique, dois cliques e botão direito ficam com os eventos normais da página.
   - Testes: os da interface sem os recortes e um de ponta a ponta no app em Rust com o `MockRuntime`.
   - Medição com a `perf-check` antes do PR (1 e 4 lives).
3. **Limpeza:** apagar o caminho da janela filha (`WindowTarget::Embedded`, `apply_holes`, o tratamento de mouse da janela), o que ficou sem uso na interface (`data-video-hole`, `watchVideoHoles`, `measureHoles`, `video-pointer`) os comandos `set_video_holes`, `set_video_hidden` e o repasse do mouse (`emit_pointer`), os eventos que vinham da janela do vídeo (`WindowEvent`: dois cliques, F11, Esc, mouse e o picture in picture antigo; agora é a página que recebe tudo isso), e atualizar `docs/arquitetura.md`. A janela solta (`WindowTarget::Standalone`) fica para o exemplo de terminal.

## Alternativas consideradas

- **Continuar com a janela por cima e ajustar os recortes:** já foram cinco tentativas. Com um buraco, o máximo é o preto ficar menor; transparência, sombra e animação sobre o vídeo são impossíveis.
- **Desenhar selos, nome e botões dentro do próprio vídeo (D3D e texto nativo):** resolve os selos, mas não os menus, as dicas e os avisos da página, e duplica a interface em dois lugares.
- **Hospedar a WebView2 em modo de composição (`CoreWebView2CompositionController`), com o vídeo e a página na mesma árvore:** daria controle total da ordem, mas o Tauri e o wry usam a WebView2 em janela, e trocar exigiria um fork do wry. A janela transparente já resolve com a WebView2 em janela.
- **Mandar o vídeo para a página (WebCodecs ou `<video>`):** o vídeo sairia da GPU para o processo da WebView2, com cópia e latência maiores, o contrário do "a imagem não sai da GPU".
