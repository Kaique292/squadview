# 0031. Fliperama: controles além do Xbox, P1 guardado e avisos do que impede o P2

- **Status:** proposta
- **Data:** 2026-10-29

## Contexto

Depois dos primeiros usos do fliperama ([ADR 0023](0023-modo-fliperama.md)), chegaram queixas de amigos que "não conseguem conectar o controle e controlar sendo o P2". Uma leitura do código achou quatro causas, todas sem aviso nenhum:

1. **Só o XInput era lido no jogador.** Controles de PlayStation (DualShock 4, DualSense), Switch e genéricos não aparecem no XInput sem programas como o DS4Windows. O controle "não conectava", e a tela só dizia para clicar e usar o teclado.
2. **O amigo virava o P1 do jogo.** Com o host jogando no teclado, o controle virtual do amigo pegava a primeira vaga do Windows. Muitos jogos tratam esse controle como o P1 (e juntam o teclado ao P1), então os dois mexiam no mesmo personagem. O app mostrava "P2" (correção da ADR 0023), mas o número era só um rótulo.
3. **Controle virtual sem vaga.** O XInput tem 4 vagas para o PC inteiro. Com elas ocupadas, o controle virtual existe, mas o jogo não o vê, e o app mostrava "P2" como se estivesse tudo certo.
4. **Jogo fora de foco.** Muitos jogos ignoram os controles quando a janela não está em primeiro plano (o padrão do SDL e do Input System da Unity). Se o host clica no SquadView, o P2 para.

Além disso, o leitor ficava com o primeiro controle do XInput que achasse: com dois ligados, podia ficar com o errado.

## Decisão

- **Jogador lê todos os controles e junta os estados.** Os de Xbox e compatíveis continuam pelo XInput, a 1000 Hz. Os outros vêm pelo **Windows.Gaming.Input**, com o mapeamento de botões do SDL, pela crate **`gilrs`** (0.11, só o backend `wgi`, que usa o mesmo `windows` 0.62 do projeto).
  - Os controles que o Windows trata como "Gamepad" (Xbox e compatíveis) são pulados no `gilrs`, para não contarem duas vezes. No `gilrs`, só eles têm vibração (`is_ff_supported`).
  - Controle sem mapeamento do SDL fica de fora: os botões sairiam trocados. A tela avisa.
  - Os botões vão pela posição: o de baixo é o A, como no controle de Xbox.
  - Juntar os estados resolve a escolha do controle: o parado está no centro e não muda nada.
- **A tela do jogador diz o que foi achado:** "Controle de PlayStation conectado", "Nenhum controle encontrado: ligue um ou jogue com o teclado" ou que o controle não tem mapa de botões. Também avisa quando a conexão passa pelo repassador (mais atraso).
- **P1 guardado (opção do host, ligada por padrão).** Se o PC do host não tem controle nenhum (fora os nossos), um controle virtual parado é plugado antes do primeiro jogador e fica com a vaga 0 do Windows. Os amigos viram o segundo, o terceiro e o quarto controle, isto é, P2 a P4 no jogo.
  - Os plugs agora acontecem um por vez. Isso garante que o P1 é guardado antes do amigo, e também que a vaga do controle novo é descoberta sem ambiguidade: antes, dois plugs juntos podiam deixar a vaga sem resposta.
  - Todo controle plugado já começa solto. Medido no teste: antes da primeira atualização, o XInput mostrava os analógicos fora do centro (-3356, -1869, -3255, -848), e mandar o controle solto não adiantava, porque o ViGEmBus pula o relatório igual ao último que guardou (que começa zerado). O `plug` espera o XInput ver o controle, manda um estado quase solto e, em seguida, o solto. Sem isso, o controle que guarda o P1 (que nunca recebe estado) podia fazer um personagem andar sozinho.
  - A opção pode ser desligada: há jogos em que o teclado já é o P1 e o primeiro controle é o P2. Nesses, o controle guardado empurraria o amigo para o P3.
- **Avisos ao host:**
  - quando o Windows não dá vaga ao controle de um jogador, com o que fazer: desligar um controle e pedir para ele entrar de novo;
  - quando a janela do jogo sai do primeiro plano com alguém jogando. A conferência é a cada 500 ms e só com a captura de uma janela. O aviso aparece fixo no painel e, uma vez por fliperama, num toast.
- **Registro de desempenho:** `arcade-controllers` (família e se tem mapa, sem o nome), `arcade-players` (P, conectado e vaga no XInput), `arcade-seat` (com `relayed`), `arcade-focus` e o `reserveP1` no `arcade-open`. Com isso dá para pedir o registro a quem reclamar.

Nada muda no protocolo: os sinais e o datagrama do controle continuam os mesmos.

## Consequências

- Quem tem DualShock 4, DualSense, Pro Controller do Switch ou um controle genérico com mapa do SDL joga sem instalar nada.
- **Latência dos controles não-Xbox:** o `gilrs` lê o Windows.Gaming.Input numa thread própria a cada 8 ms, então esses controles podem esperar até 8 ms a mais que os de Xbox. É menos que o intervalo do próprio controle por Bluetooth, mas precisa entrar na medição da Etapa 5.
- **A confirmar no teste real:** se o Windows.Gaming.Input entrega a entrada com o SquadView fora do primeiro plano. Quem joga normalmente está com a janela do SquadView na frente (vendo o jogo), então o caso comum funciona.
- O controle guardado aparece para o jogo como um controle ligado e parado. Se o host ligar um controle depois de abrir o fliperama, ele vem depois do guardado (P2 para o Windows); nesse caso, fechar e abrir o fliperama de novo resolve.
- Uma dependência nova (`gilrs`, MIT/Apache-2.0), só no `sv-input` e só usada por quem joga.
- **Achado no teste, sem correção:** se um controle virtual é tirado uns 40 ms depois de aparecer no XInput e outro é plugado em seguida, o novo não ganha vaga (esperamos 20 s) até ser tirado também. Com o controle plugado por mais tempo (como quando alguém entra e sai do fliperama), o replug ganha vaga em cerca de 280 ms. Se aparecer no uso, o aviso de "sem vaga" do host mostra.

## Alternativas consideradas

- **Gamepad API da página (WebView2):** lê controles de PlayStation sem dependência nova, mas só no ritmo da página (até 16 ms) e por `invoke` a cada mudança. Ficaria abaixo do caminho de 1000 Hz do XInput.
- **SDL3 no Rust:** mapeamento igual e leitura em segundo plano, mas exige compilar o SDL (CMake) ou embutir uma DLL. Pesado demais para o ganho, enquanto o `gilrs` resolve.
- **Ler o HID do DualShock e do DualSense na mão:** sem dependência, mas é um protocolo por controle (USB e Bluetooth diferentes) para manter.
- **Trocar o número mostrado em vez de guardar o P1:** foi o que a ADR 0023 fez, e não resolve: o jogo continua vendo o amigo como o primeiro controle.
