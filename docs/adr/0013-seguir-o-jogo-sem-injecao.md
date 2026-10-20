# 0013. Seguir o jogo entre janelas e para o monitor, sem injeção

- **Status:** aceita
- **Data:** 2026-10-20

## Contexto

Jogos com launcher (League of Legends, Valorant e muitos outros) trocam de **processo** entre o cliente e a partida, e o cliente não fecha: só se esconde. A captura só trocava de janela quando a antiga morria, então a transmissão congelava no lobby. Além disso, o app pedia para o jogador trocar a tela cheia por "janela sem bordas" quando a imagem parava, o que quem joga em tela cheia não aceita. Detalhes e requisitos em [captura.md](../captura.md).

## Decisão

O vigia da prévia (a cada 500 ms) segue a "família" do jogo, definida pela pasta de instalação do executável, e, quando a janela do jogo cobre o monitor e a captura dela para, captura o monitor do jogo, voltando para a janela assim que outra coisa ficaria visível. Tudo com Windows.Graphics.Capture e consultas Win32 só de leitura, com a decisão numa lógica pura (`sv_capture::GameFollower`).

## Consequências

- Funciona com qualquer modo de tela comum no Windows 10/11 sem o jogador mexer no jogo, e com qualquer jogo instalado por qualquer loja, sem lista de jogos.
- Nenhum risco com anti-cheat: nada entra no processo do jogo.
- Capturar o monitor exige cuidado com privacidade: o vigia volta para a janela na hora em que algo de fora do jogo ficaria na frente dele, e para a transmissão se o jogo fechar enquanto o monitor é capturado.
- Cada troca reinicia captura e encoder (um keyframe e um instante sem imagem).
- A checagem custa menos de 1 ms (mediana) a cada 500 ms no vigia, fora do caminho do frame.
- A acompanhar: tela cheia exclusiva legada (sem otimizações de tela cheia), em que o WGC pode não receber quadros (não reproduzido no Windows 11). Se aparecer na prática, a saída é a DXGI Desktop Duplication.

## Alternativas consideradas

- **"Game capture" com hook no `Present` (estilo OBS):** pega qualquer tela cheia, mas injeta DLL no jogo; Vanguard, EAC e BattlEye podem banir a conta.
- **Sempre capturar o monitor:** simples, mas mostra para a sala tudo o que aparece por cima do jogo (alt-tab, notificações, Discord) e perde o recorte da janela.
- **Pedir "janela sem bordas" ao jogador:** era o comportamento antigo; contradiz o requisito de não mexer no jogo.
- **Lista de jogos conhecidos (cliente → executável da partida):** quebra a cada jogo novo ou atualização; a regra da pasta de instalação cobre os casos sem manutenção.
- **DXGI Desktop Duplication desde já:** desnecessária nos testes (o WGC pegou tela cheia sem bordas e exclusiva no Windows 11); fica como próximo passo se aparecer um caso real.
