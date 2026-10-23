# 0023. Modo Lan House Fliperama: jogo local com controles remotos

- **Status:** aceita
- **Data:** 2026-10-22

## Contexto

Muitos jogos têm multiplayer só local: vários controles no mesmo PC e a tela dividida ou compartilhada (Bomberman, jogos de luta, party games). O Steam Remote Play Together e o Parsec resolvem isso transmitindo o jogo e trazendo os controles de volta pela rede. O SquadView já tem quase tudo:

- captura de janela com WGC e encode de hardware em baixa latência (`AVLowLatencyMode`, sem B-frames, CBR);
- conexão QUIC direta entre os membros, com os repasses de reserva;
- relógio sincronizado (`ClockEstimator`) e a latência medida da captura até a tela.

Falta o caminho de volta. Hoje nada lê controle ou teclado, e quem assiste não manda nada ao host além de `KeyframeRequest` e `Ping` pelo canal de controle.

Esse modo é a primeira parte do "modo Lan House". A rede local virtual entre os membros (o outro modo) fica para outra ADR.

## Decisão

O host abre um **fliperama** com a janela de um jogo. Até 3 membros da call entram como jogadores (P2 a P4) e assistem ao jogo numa janela própria. O controle de cada jogador vai ao host por **datagramas QUIC** numa conexão nova (`Purpose::Arcade`, ALPN `squadview-arcade/1`). No PC do host, cada jogador vira um **controle virtual de Xbox 360 pelo ViGEmBus**.

Pontos principais:

- **Vídeo:** reaproveita a live. O host transmite a janela do jogo como hoje e o jogador assiste pelo caminho normal (`WatchRequest`). A captura fica **fixa na janela** (sem "seguir o jogo"), e o fliperama pede a qualidade de menor latência: 60 fps, mantendo o encoder de sempre.
- **Entrada:** a conexão `Arcade` é separada da de vídeo, para não mexer no `Control` (postcard), que derrubaria o canal de controle dos apps antigos.
  - Cada datagrama leva o **estado completo** do controle: botões, gatilhos, analógicos e um número de sequência. Perder um não importa, porque o próximo corrige.
  - O jogador manda quando o estado muda e, parado, a cada 100 ms.
  - O host descarta pacotes fora de ordem.
- **Leitura no jogador:**
  - controle de Xbox pelo **XInput** (`Win32_UI_Input_XboxController`), num thread que lê a 1000 Hz (acima da prioridade normal);
  - **teclado** convertido em controle (mapeamento padrão editável), lido na página da janela do fliperama enquanto ela tem foco.
- **Vagas:** o host decide. Sinais novos pela sinalização (`ArcadeJoin`, `ArcadeSeat { slot }`, `ArcadeDeny { reason }`, `ArcadeLeave`). O status do host ganha o campo opcional `arcade: { game, slots, players }`, para a sala mostrar "Fliperama: 2 vagas".
  - O número do jogador mostrado é a vaga real do XInput que o Windows deu ao controle virtual, mais 1. Com o controle do host ligado, os jogadores ficam como P2 a P4; sem ele, o primeiro jogador é o P1, que é como o jogo vai numerá-lo.
    - **Atualização (out/2026, teste real):** o host é sempre o P1, e os jogadores ficam como P2 a P4. Com o host no teclado, o amigo aparecia como "P1", e isso confundia: no jogo (SuperTuxKart) ele era o segundo jogador. A vaga do XInput + 1 só vale quando já é 2 a 4; senão, o primeiro P livre.
  - O host pode tirar alguém ou trocar a ordem.
- **ViGEmBus:** só o host precisa. O app **não embute o instalador**: ao abrir o fliperama, ele verifica se o driver existe e, se não existir, leva ao instalador oficial (precisa de administrador uma vez). O cliente é uma crate Rust pura (`vigem-rust`), sem DLL: usa o mesmo `windows` 0.62 do projeto e distingue "driver não instalado" de outros erros. A `vigem-client` foi descartada por puxar o `winapi` antigo.
- **Desinstalar a qualquer momento:** em Configurações, a seção "Fliperama" mostra se o driver está instalado e tem um botão **"Remover o driver de controle"**.
  - O botão chama o desinstalador registrado no Windows (o mesmo de "Aplicativos instalados", com o aviso de administrador) e depois confere se o driver saiu.
  - Com o fliperama aberto, o app pede para fechá-lo antes.
  - Sem o driver, o fliperama volta ao estado "não instalado" e nada do modo fica rodando: o app só conecta ao driver enquanto um fliperama está aberto.
- **Segurança:** a entrada de um jogador **só** move o controle virtual dele. O app nunca injeta teclado nem mouse no host. Só membros da sala com vaga dada pelo host conseguem abrir a conexão `Arcade` (a fingerprint é conferida, como na voz).

O plano de implementação está em [`docs/fliperama.md`](../fliperama.md).

## Consequências

- **Para o jogador:** qualquer jogo com vários controles locais vira online, sem o jogo saber. Quem joga não instala nada.
- **Desempenho do host:** o host carrega o jogo e o encode, como numa live. O caminho de entrada é pequeno (datagramas de ~32 bytes e uma chamada ao driver por mudança), mas tudo precisa ser medido com a skill `perf-check`.
- **O driver é opcional e reversível:** só quem hospeda instala, e remove pelo próprio app quando quiser. Quem nunca abre um fliperama não instala nada.
- **Driver:** o ViGEmBus foi **arquivado em novembro de 2023**. As instalações existentes continuam funcionando, mas ele não recebe correções. Se uma atualização do Windows quebrar o driver, o fliperama para de funcionar, e a única saída seria um driver próprio, com assinatura da Microsoft (caro). Um sucessor oficial foi anunciado, sem data.
- **Limite do XInput:** o XInput tem 4 vagas no total. Se o host tiver mais de um controle físico, sobram menos vagas, e o app mostra as vagas reais.
- **Compatibilidade:** é uma ALPN nova, então apps antigos simplesmente não abrem fliperama. Sinais e o campo `arcade` são opcionais, e apps antigos os ignoram. O Worker precisa aceitar o campo `arcade` no `status` (a validação atual descarta campos desconhecidos).
- **Latência:** é o que decide se o modo é bom. A meta é **< 80 ms do botão até a tela** com conexão direta. Pelo repasse (TURN ou membro), a latência aumenta e o app avisa.
- **Limitações dos jogos:**
  - jogos que só aceitam teclado não funcionam como estão (exigiriam injetar teclado no host, o que esta ADR recusa);
  - jogos que só leem DirectInput não enxergam um controle de Xbox, e o ViGEm também emula DualShock 4, o que pode vir depois.
- **O que acompanhar:**
  - a latência de entrada, medida no host com o relógio sincronizado (o jogador carimba cada datagrama no relógio do host);
  - os quadros perdidos do jogo com o fliperama aberto.

## Alternativas consideradas

- **Mandar a entrada pelo canal de controle (`Control::Input`):** é um stream confiável, então uma perda trava os pacotes seguintes (head-of-line blocking). Além disso, uma variante nova no `Control` derruba o canal de controle dos apps antigos.
- **Injetar teclado e mouse no host (`SendInput`):** funciona em mais jogos, mas mistura as teclas de todos os jogadores com as do host e dá a um membro da sala controle do PC dele. É um risco de segurança grande demais.
- **Driver de controle virtual próprio:** sem depender de um projeto arquivado, mas exige desenvolvimento em kernel e assinatura de driver (EV e o portal da Microsoft). Fica como saída se o ViGEmBus parar de funcionar.
- **Embutir o instalador do ViGEmBus no app:** facilita a primeira vez, mas acoplaria o instalador do SquadView a um driver de kernel arquivado. Melhor deixar o host instalar pelo instalador oficial, sabendo o que está instalando.
- **Ler o controle na página (Gamepad API do navegador):** funciona na WebView, mas só com a janela em foco e com o polling da página. O XInput no Rust lê com regularidade e sem depender da interface.
