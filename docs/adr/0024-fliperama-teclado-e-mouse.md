# 0024. Fliperama: mouse no analógico e "controle total" de teclado e mouse

- **Status:** aceita
- **Data:** 2026-10-22

## Contexto

A [ADR 0023](0023-modo-fliperama.md) leva o controle (ou o teclado convertido em controle) de cada jogador a um controle virtual de Xbox no PC de quem hospeda, e recusa injetar teclado e mouse de verdade no host. Ficaram dois casos de fora:

- **Mouse do jogador.** Num jogo de tela dividida, mirar com o analógico direito pelo teclado é ruim. O mouse de quem joga não tem para onde ir, porque o jogo local tem um ponteiro só.
- **Jogos que só aceitam teclado e mouse.** Com eles, o controle virtual não serve. O Parsec resolve isso deixando um convidado usar teclado e mouse do host, com a permissão de quem hospeda.

## Decisão

1. **Mouse no analógico (sempre disponível):** na janela do fliperama, clicar no vídeo prende o ponteiro (_pointer lock_), e o movimento do mouse vira o **analógico direito** do controle virtual do jogador.
   - O desvio é proporcional à velocidade do mouse e volta ao centro quando o mouse para.
   - O botão esquerdo vira o gatilho direito (RT) e o direito vira o gatilho esquerdo (LT).
   - Esc solta o ponteiro.
   - Tudo isso acontece no PC do jogador. Para o host continua chegando só o estado do controle (ADR 0023), então nada muda na segurança.

2. **Controle total (só com permissão do host, um jogador por vez):** o host pode dar a **um** jogador o teclado e o mouse de verdade. Teclas, movimento do mouse, botões e rolagem vão por um stream confiável da conexão do fliperama (`squadview-arcade/1`), e o host os injeta com `SendInput`. Regras:
   - **Começa desligado.** Só o host liga, por jogador, no painel do fliperama. Ligar para outro jogador desliga o anterior.
   - **Só com a janela do jogo em primeiro plano.** O host só injeta enquanto a janela capturada pelo fliperama é a janela em primeiro plano do Windows. Se o host trocar de janela (Alt+Tab, outro app), nada passa e todas as teclas e botões são soltos.
   - **Exige captura de janela.** Com a tela inteira capturada não há "janela do jogo" para conferir, e o controle total não liga.
   - **Ponteiro preso no jogo.** Enquanto o jogador mexe o mouse, o ponteiro fica preso na área da janela do jogo, e um clique com o ponteiro fora dela não passa. Sem isso, num jogo em janela, o primeiro clique na barra de tarefas ou na área de trabalho acontecia de verdade, e só depois o jogo deixava de estar na frente. O ponteiro do host também fica preso enquanto o controle total está ligado; cortar solta.
   - **Teclas do sistema bloqueadas:** Windows (esquerda e direita), menu de contexto, Alt+Tab, Alt+F4, Alt+Esc, Ctrl+Esc e Ctrl+Shift+Esc. O Ctrl+Alt+Del nem é injetável no Windows.
   - **Corte na hora.** O host desliga pelo painel ou pelo atalho global **Ctrl+Alt+F12**, que funciona com o jogo em tela cheia. Ao desligar, cair ou sair, o host solta tudo que estava apertado.
   - **Nada fica gravado:** a permissão vale só para aquele fliperama aberto.

Também vale para todo jogador, com ou sem controle total: o botão **Guide** (o "X" do meio do controle) nunca chega ao controle virtual do host, porque ele abre a Xbox Game Bar e o Big Picture da Steam, fora do jogo.

## Consequências

- **Com o mouse no analógico,** jogos de tiro e de mirar em tela dividida ficam jogáveis por quem não tem controle.
- **Com o controle total,** jogos só de teclado e mouse funcionam no fliperama, um jogador por vez. Teclado e mouse de verdade são um só no PC, então dois jogadores ao mesmo tempo apertariam as mesmas teclas.
- **Risco:** com o controle total ligado, um amigo controla o teclado e o mouse do host dentro do jogo. As regras acima limitam isso à janela do jogo e às teclas que não são do sistema, mas o host precisa confiar em quem recebe. Por isso começa desligado, é individual e tem corte imediato.
- **Jogos com anti-cheat** podem ignorar ou punir entrada injetada. Isso é do jogo e fica documentado no app.
- **O que acompanhar:** se o filtro de primeiro plano deixa passar alguma coisa ao trocar de janela, e se algum jogo popular precisa de uma tecla bloqueada.

## Alternativas consideradas

- **Teclado e mouse de todos os jogadores ao mesmo tempo:** no Windows o teclado é um só. Teclas de jogadores diferentes se misturariam no mesmo jogo, sem como separar quem é quem.
- **Injetar sem conferir o primeiro plano:** se o host trocasse de janela, as teclas do amigo iriam para outro programa (um chat, o navegador). Inaceitável.
- **Mouse absoluto (posição na tela):** jogos com câmera em primeira pessoa leem o movimento relativo (_raw input_), e a posição absoluta dependeria da resolução dos dois PCs. O relativo funciona nos dois casos.
