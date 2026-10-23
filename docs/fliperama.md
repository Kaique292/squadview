# Plano: modo Lan House Fliperama

Decisão e motivos: [ADR 0023](adr/0023-modo-fliperama.md). Este documento é o plano de implementação, em etapas que entram cada uma no seu PR para a `dev`.

**Meta:** o host abre o fliperama com a janela de um jogo; até 3 membros entram como P2 a P4, veem o jogo numa janela própria e jogam com o controle (ou teclado) deles. **< 80 ms do botão até a tela** com conexão direta, sem derrubar o FPS do jogo do host (skill `perf-check`).

## Como funciona

```
Jogador (P2–P4)                                   Host
XInput 250 Hz / teclado ──datagrama QUIC (Arcade)──▶ vaga → controle virtual X360 (ViGEmBus) → jogo
janela do fliperama ◀──────── live normal (Stream) ─── captura WGC da janela → encoder
              └──── sinais ArcadeJoin/Seat/Deny/Leave pela sinalização ────┘
```

## Etapa 0: prova de conceito do driver (só host, sem rede)

- [x] Instalar o ViGEmBus pelo instalador oficial numa máquina de teste e registrar a versão: **1.22.0**, desinstalador `MsiExec.exe /X{966606F3-2745-49E9-BF15-5C3EAA4E9077}`.
- [x] Exemplo `examples/arcade_loopback.rs` no crate novo `crates/sv-input`:
  - lê o controle físico pelo XInput (250 leituras por segundo);
  - repete o estado num controle virtual X360 (`vigem-rust`) e mostra quanto cada atualização leva;
  - mostra a vaga do XInput que o Windows deu ao controle virtual.
  - Rodar: `cargo run -p sv-input --example arcade_loopback -- [--from 0] [--seconds 60]`. Back + Start por 2 s para parar.
- [ ] Abrir um jogo com vários controles locais e conferir que ele vê o controle virtual como P2.
- [x] Detectar se o driver está instalado (`driver_status`: `Installed`, `Missing`, `Incompatible` ou `Unavailable`) e achar o desinstalador registrado (`find_uninstaller`, em `HKLM\...\Uninstall` nas vistas de 64 e 32 bits, pelo nome do produto).
  - Conferido sem o driver: o exemplo mostra o link do instalador oficial e para.
  - Conferido com o driver: o exemplo acha o driver e o desinstalador, e o teste `#[ignore]` `pluga_atualiza_e_tira_um_controle_virtual` (`cargo test -p sv-input -- --ignored`) passa: o XInput vê o estado aplicado no controle virtual, e a vaga fica livre ao desplugar.
  - Achados:
    - no **primeiro** plug depois de instalar o driver, o Windows instala o driver do controle e a espera "pronto" volta abortada (`ERROR_OPERATION_ABORTED`); do segundo em diante, funciona;
    - logo depois do plug, o driver ainda não sabe a vaga do XInput (`XUSB_GET_USER_INDEX` responde `ERROR_INSUFFICIENT_BUFFER`). O `plug` pergunta por até 2 s e, enquanto isso, também olha qual vaga do XInput passou a estar ocupada.
  - Na Etapa 3, o host deve tentar plugar de novo uma vez quando o primeiro plug for abortado.
- **Pronto quando:** o jogo responde ao controle virtual e a detecção funciona com e sem o driver.

## Etapa 1: crate `sv-input` (lógica pura + plataforma)

- [x] `PadState` em 12 bytes (`to_bytes`/`from_bytes`), `merge` (controle físico + teclado) e zona morta. O datagrama (`wire.rs`) leva o tipo, o `seq` e o estado: 17 bytes.
- [x] `PadReader`: thread que lê o XInput a 250 Hz e entrega só quando o estado muda (com zona morta), sem alocação no laço. Sem controle ligado, procura de novo a cada 500 ms.
- [x] `KeyMap` e `KeyboardPad`: teclado (pelo `KeyboardEvent.code`) → controle, com o mapeamento padrão (WASD e setas, J/Espaço, K, L, I, U/O, Q/E, Enter, Backspace, teclado numérico) editável e salvo em `arcade.json`. O mouse vira o analógico direito (ADR 0024).
- [x] `VirtualPads` (só host): cria e remove os controles X360, aplica o estado e devolve a vaga do XInput. O controle sai do PC no `Drop`.
- [x] Controle total (`kbm.rs`, ADR 0024): `vk_from_code`, `KbmGuard` (bloqueia as teclas do sistema e solta tudo), `Injector` (`SendInput` só com a janela do jogo em primeiro plano) e `PanicHotkey` (Ctrl+Alt+F12).
- [x] Testes: 31 no `sv-input`. O formato, o mapeamento, as regras do controle total e o teclado viram controle rodam na CI; o driver fica no `#[ignore]`.

## Etapa 2: rede (`sv-net`) e protocolo

- [x] `Purpose::Arcade` com a ALPN `squadview-arcade/1` em `crates/sv-net/src/endpoint.rs` e `lib.rs`.
- [x] Datagrama de entrada: `[tipo u8 = 1][seq u32][PadState]`, 17 bytes. O `host_us` ficou de fora: o jogador só tem o relógio do host enquanto assiste a live, e a latência de entrada fica para a Etapa 5 (RTT/2 da conexão do fliperama, ou o relógio da live).
- [x] Mandar quando muda e, parado, a cada 100 ms. O host descarta `seq` antigo e solta todos os botões da vaga se ficar 1 s sem nada (o jogador caiu).
- [x] Sinais novos em `Signal` (`protocol.rs`): `ArcadeJoin { candidates, fingerprint }`, `ArcadeSeat { slot, candidates, fingerprint, relay }`, `ArcadeDeny { reason }`, `ArcadeLeave` e `ArcadeKbm { granted }`. Apps antigos ignoram sinais desconhecidos.
- [x] Worker: campo opcional `arcade: { game, slots, players } | null` no `status` (`services/signaling/src/protocol.ts` e `room-state.ts`), com testes. `PROTOCOL.md` e `docs/protocolo-malha.md` atualizados.
- [x] Testes de integração contra o Worker local: fliperama fechado → `ArcadeDeny`; abrir pede live e driver; e, com o ViGEmBus (`#[ignore]`), o fluxo inteiro no mesmo PC: o jogador ganha a vaga, aperta J e o controle virtual do host aperta A; tirar o jogador tira o controle do PC.
- **Publicar o Worker antes do app.**

## Etapa 3: sessão (`sv-session`)

- [x] `room/arcade.rs` (host):
  - abre e fecha o fliperama sobre a live atual;
  - usa a janela capturada para o controle total. **Pendente:** com o "seguir o jogo" trocando de janela, o controle total para de passar (a regra do primeiro plano compara com a janela da abertura). Falta atualizar a janela ou fixar a captura;
  - administra as vagas (dar, tirar, trocar a ordem);
  - aceita a conexão `Arcade` só de quem tem vaga (fingerprint conferida, como na voz).
- [x] Jogador: pede a vaga, abre a conexão `Arcade`, liga o `PadReader` e o teclado e o mouse, e para tudo ao sair. A interface assiste a live do host pelo caminho normal.
- [x] `RoomUpdate`s novos: `ArcadePlayers`, `ArcadeSeat`, `ArcadeLeft` (recusado, tirado, fechado ou caiu, com o motivo) e `ArcadeKbm`.
- [ ] Estatística nova: latência de entrada (média e p95) no host, no registro de desempenho. Fica para a Etapa 5.

## Etapa 4: app e interface

- [ ] **Host:**
  - [x] "Fliperama" no menu da nossa live abre o painel: driver, vagas (1 a 3), abrir e fechar;
  - [x] se o driver faltar, um aviso com o botão do instalador oficial;
  - [x] um painel com os jogadores (P, conectado ou não), tirar e o controle total por jogador, com o aviso do Ctrl+Alt+F12. Trocar a ordem ficou de fora: o P é a vaga que o Windows dá ao controle virtual.
- [ ] **Jogador:**
  - [x] "Entrar no fliperama (N vagas)", "Fliperama cheio" e "Sair do fliperama" no menu do membro (entrar também assiste a live);
  - [x] em vez de uma janela própria, o próprio quadro da live do host ganha o selo "Você é o P3" e a captura: clicar prende o ponteiro, as teclas e o mouse vão para o jogo, Esc solta, perder o foco solta tudo. Assim funciona igual em grade, em foco e em tela cheia, sem outra janela para o vídeo. A latência no selo fica para a Etapa 5;
  - [x] a página manda o teclado e o mouse ao Rust enquanto o quadro tem foco (o mouse uma vez por quadro da página).
- [x] **Configurações > Fliperama:**
  - mostra se o driver está instalado;
  - mapeamento do teclado;
  - botão **"Remover o driver de controle"**: chama o desinstalador registrado (aviso de administrador), confere se saiu e volta a "não instalado". Com o fliperama aberto, pede para fechar antes.
- [x] Testes de interface (14): painel com e sem driver, abrir, jogadores, controle total, tirar, entrar, cheio, sair, avisos, captura do teclado, Esc e foco, remover o driver e trocar uma tecla.
- [ ] Interface conforme `docs/design/ui.md`. Atualizar o README com prints (depois do teste real da Etapa 5).

## Etapa 5: medição e testes reais

- [ ] `perf-check` no host com o fliperama aberto e 3 jogadores: FPS do jogo, 1% low e uso do encoder.
- [ ] Latência do botão até a tela: no host, a de entrada (registro) somada à da live (captura até a tela, já medida); no jogador, a ponta a ponta com um teste de câmera, se der.
- [ ] Jogar de verdade com amigos em outra rede: um Bomberman, um jogo de luta e um party game. Anotar os que não funcionam (só teclado, só DirectInput).
- [ ] Registrar os números na ADR 0023 (e revisá-la se a meta não for atingida).

## Fora deste plano

- Rede local virtual entre os membros (o outro modo Lan House): vai ter uma ADR própria.
- Controle virtual de DualShock 4 (jogos que só leem DirectInput): depois, se aparecer jogo que precise.
- Vibração voltando ao jogador (o ViGEm avisa): depois da Etapa 5.
