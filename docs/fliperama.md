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

- [ ] `PadState`: botões (u16 no layout do XInput), gatilhos (u8), analógicos (i16) e `seq` (u32). Também `encode`/`decode` de tamanho fixo (~16 bytes) com teste de ida e volta. O estado (sem `seq`) e a conversão para o controle virtual já existem desde a Etapa 0.
- [ ] `PadReader`: thread que lê o XInput a 250 Hz e entrega só quando o estado muda (com zona morta). Sem alocação no laço.
- [ ] `KeyMap`: teclado → `PadState`, com o mapeamento padrão (WASD/setas, J K L I, Enter, Esc) editável e salvo em `arcade.json`.
- [ ] `VirtualPads` (só host): cria e remove os controles X360, aplica o estado, devolve o `user index`, e fecha tudo no `Drop`.
- [ ] Testes: o formato e o mapeamento rodam na CI. O driver fica em testes `#[ignore]`, que precisam do ViGEmBus.

## Etapa 2: rede (`sv-net`) e protocolo

- [ ] `Purpose::Arcade` com a ALPN `squadview-arcade/1` em `crates/sv-net/src/endpoint.rs` e `lib.rs`.
- [ ] Datagrama de entrada: `[tipo u8 = 1][seq u32][host_us u64][PadState]`. O `host_us` é o horário do clique no relógio do host (pelo `ClockEstimator` que o jogador já tem da live), para medir a latência de entrada no host.
- [ ] Mandar quando muda e, parado, a cada 100 ms. O host descarta `seq` antigo e solta todos os botões da vaga se ficar 1 s sem nada (o jogador caiu).
- [ ] Sinais novos em `Signal` (`protocol.rs`): `ArcadeJoin { candidates, fingerprint }`, `ArcadeSeat { slot }`, `ArcadeDeny { reason }`, `ArcadeLeave`. Apps antigos ignoram sinais desconhecidos.
- [ ] Worker: campo opcional `arcade: { game, slots, players } | null` no `status` (`services/signaling/src/protocol.ts` e `room-state.ts`), com testes. Atualizar o `PROTOCOL.md` e o `docs/protocolo-malha.md`.
- [ ] Testes de integração contra o Worker local: pedir vaga, receber `ArcadeSeat`, vaga cheia → `ArcadeDeny`, sair libera a vaga.
- **Publicar o Worker antes do app.**

## Etapa 3: sessão (`sv-session`)

- [ ] `arcade.rs` (host):
  - abre e fecha o fliperama sobre a live atual;
  - fixa a captura na janela escolhida (sem "seguir o jogo");
  - administra as vagas (dar, tirar, trocar a ordem);
  - aceita a conexão `Arcade` só de quem tem vaga (fingerprint conferida, como na voz).
- [ ] Jogador: pede a vaga, abre a live do host e a conexão `Arcade`, liga o `PadReader` e o teclado, e para tudo ao sair.
- [ ] `RoomUpdate`s novos: vaga recebida ou recusada, jogadores do fliperama, fliperama fechado.
- [ ] Estatística nova: latência de entrada (média e p95) no host, no registro de desempenho.

## Etapa 4: app e interface

- [ ] **Host:**
  - no `StreamDialog`, a opção "Abrir fliperama" (janela do jogo e número de vagas);
  - se o driver faltar, um aviso com o link do instalador oficial;
  - um painel do fliperama com os jogadores, P2 a P4, tirar e trocar a ordem.
- [ ] **Jogador:**
  - "Entrar no fliperama" no menu do membro e no palco;
  - janela própria (Tauri, como o picture in picture) com o vídeo por baixo, o selo "Você é o P3", a latência e o atalho para sair;
  - a página manda o teclado ao Rust enquanto tem foco.
- [ ] **Configurações > Fliperama:**
  - mostra se o driver está instalado;
  - mapeamento do teclado;
  - botão **"Remover o driver de controle"**: chama o desinstalador registrado (aviso de administrador), confere se saiu e volta a "não instalado". Com o fliperama aberto, pede para fechar antes.
- [ ] Testes de interface: abrir, vaga cheia, sair, remover o driver.
- [ ] Interface conforme `docs/design/ui.md`. Atualizar o README com prints.

## Etapa 5: medição e testes reais

- [ ] `perf-check` no host com o fliperama aberto e 3 jogadores: FPS do jogo, 1% low e uso do encoder.
- [ ] Latência do botão até a tela: no host, a de entrada (registro) somada à da live (captura até a tela, já medida); no jogador, a ponta a ponta com um teste de câmera, se der.
- [ ] Jogar de verdade com amigos em outra rede: um Bomberman, um jogo de luta e um party game. Anotar os que não funcionam (só teclado, só DirectInput).
- [ ] Registrar os números na ADR 0023 (e revisá-la se a meta não for atingida).

## Fora deste plano

- Rede local virtual entre os membros (o outro modo Lan House): vai ter uma ADR própria.
- Controle virtual de DualShock 4 (jogos que só leem DirectInput): depois, se aparecer jogo que precise.
- Vibração voltando ao jogador (o ViGEm avisa): depois da Etapa 5.
