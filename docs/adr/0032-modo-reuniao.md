# 0032. Modo reunião: transcrição local da voz, falas divididas pela sala e exportação para o Discord e PDF

- **Status:** aceita
- **Data:** 2026-11-05

## Contexto

A galera também usa a sala para combinar coisas: estratégia de clã, regras de um campeonato, o que cada um vai fazer na próxima sessão. Hoje, o que foi dito se perde quando a call acaba, e alguém tem que resumir à mão no Discord.

O pedido: um **modo reunião** que transcreve a conversa da sala numa aba, com a foto, o nome, a data e a hora de quem falou. No fim, a transcrição sai em dois formatos:

- texto pronto para colar no Discord, com e sem Nitro (sem Nitro, cada mensagem tem no máximo 2.000 caracteres; com Nitro, 4.000);
- um PDF com o visual do SquadView.

O projeto não tem servidor de mídia (ADR 0002) e roda ao lado de jogos: a GPU fica com o jogo e o encode, e nada pesado entra na thread do microfone (ADR 0010).

## Decisão

**Cada pessoa transcreve a própria voz, no próprio PC, com o Whisper. Só o texto sai do PC: a sinalização o difunde para a sala.**

### Transcrição

- **Onde:** no PC de quem fala, a partir do microfone dela, depois da supressão de ruído (ADR 0010).
  - Quem falou é sempre certo: é o dono do microfone, sem adivinhar pela voz.
  - O áudio vem limpo, sem a call misturada.
  - O áudio não sai do PC.
- **Trechos:** o detector de fala que já acende a borda verde corta as falas.
  - Uma fala termina depois de 600 ms de silêncio, ou aos 15 s se a pessoa não parar.
  - A thread do microfone só copia as amostras para um buffer circular reaproveitado. Ela não espera a transcrição nem aloca por pacote.
- **Motor:** whisper.cpp, pela crate `whisper-rs`, atrás de uma trait `Transcriber` em `sv-media`, para dar para trocar o motor depois.
  - **Só CPU:** a GPU continua com o jogo e o encode.
  - **Prioridade baixa:** a transcrição roda numa thread própria com `THREAD_PRIORITY_BELOW_NORMAL`, com até 2 threads de cálculo.
  - **Fila curta:** se a fila de falas passar de 30 s de áudio, as mais antigas viram "[trecho não transcrito]". A transcrição nunca segura o microfone nem a voz.
  - **Áudio:** os trechos descem de 48 kHz para 16 kHz mono, o formato do Whisper.
  - **Idioma:** português por padrão, com a opção de detectar sozinho.
- **Modelo:** baixado na primeira vez que a pessoa aceita ser transcrita, nunca no instalador, que continua com cerca de 5 MB.
  - Padrão: `small` quantizado, uns 190 MB. Para PC mais fraco, o `base`, uns 60 MB.
  - Vem do repositório oficial do whisper.cpp, com SHA-256 conferido antes de usar.
  - Fica na pasta de dados do app, e dá para apagar nas configurações.
- **Build:** o `whisper-rs` compila o whisper.cpp com CMake. O CI do Windows já tem CMake. O `docs/desenvolvimento.md` passa a pedir o CMake para compilar localmente.

### Falas na sala

- **Ligar e desligar:** qualquer membro liga o modo reunião (`meeting-start`) e quem ligou desliga (`meeting-stop`).
  - O Worker guarda o estado na sala, junto da presença: se o modo está ligado, quem ligou e quando.
  - Assim, quem entra no meio já sabe que a reunião está ligada.
  - Se quem ligou sair da sala, o modo passa para o membro mais antigo que aceitou ser transcrito.
- **Uma fala:** sai como `meeting-line { id, at, text }`, mandada **uma vez** ao Worker, que difunde para a sala inteira.
  - `id`: aleatório, para não repetir uma fala.
  - `at`: a hora de início da fala, em UTC, pelo relógio de quem falou. A tela mostra no horário local de quem lê.
  - O Worker põe o id de quem mandou: ninguém fala em nome de outro.
  - Uma fala de até 15 s tem poucas centenas de bytes, bem abaixo do limite de 8 KB por mensagem.
  - Um envio por fala, e não um `signal` por membro, mantém o custo igual com 3 ou com 300 pessoas, longe do limite de 100 mensagens a cada 10 s.
- **O que o Worker não faz:** não guarda o texto. Ele só repassa, como já faz com os outros sinais, nos quais já é de confiança (ADR 0020).
- **Quem entra no meio:** pede o histórico a quem ligou o modo (`meeting-history-request`), que responde por `signal` em pedaços de até 8 KB (`meeting-history`).
- **Apps antigos:** ignoram as mensagens novas, como já acontece com os outros sinais. A aba mostra quem está numa versão sem modo reunião.

### Consentimento

- Quando o modo liga, cada membro vê: "Fulano ligou o modo reunião. Transcrever a sua voz?". **Ninguém é transcrito sem responder sim.**
- Quem aceitou ganha um selo "transcrevendo" no quadro e no nome da lista. A aba mostra quem recusou.
- Dá para parar a própria transcrição a qualquer hora. O microfone mudo também não é transcrito.
- A resposta não é lembrada de uma reunião para a outra: cada vez, a pessoa decide de novo.

### A aba e o que fica salvo

- Uma aba **Reunião** na sala, com as falas em ordem: foto, nome, data e hora, e texto. Ela rola sozinha, a não ser que a pessoa tenha subido para ler.
- A transcrição fica salva só no PC de cada um, em `reunioes/` na pasta de dados do app. Ficam as últimas 20, que dá para abrir e apagar no app. Nada vai para outro lugar.

### Exportação

- **Discord:**
  - Cabeçalho com a sala, a data, a duração e quem participou.
  - Cada fala no formato `**Nome** · 14:32 — texto`, com a hora no horário local de quem exporta.
  - **Partes:** de no máximo 2.000 caracteres sem Nitro e 4.000 com Nitro, cada uma com o seu botão de copiar ("1/3", "2/3"…). Nenhuma fala é cortada ao meio; uma fala maior que o limite é quebrada no fim de uma frase ou palavra.
  - **Escape:** o texto falado é escapado antes de ir para o Markdown. `*`, `_`, `` ` ``, `~`, `|` e `>` não viram formatação, e `@everyone`, `@here` e menções ficam neutralizados.
  - **Sem fotos:** o Discord não mostra imagem em mensagem comum.
- **PDF:**
  - Uma página HTML no visual do SquadView: tokens de `docs/design/ui.md`, Noto Sans, fotos embutidas, cabeçalho com a sala, a data, a duração e quem participou.
  - Vira PDF pelo `PrintToPdf` do WebView2, que o app já usa, sem dependência nova, e é salvo onde a pessoa escolher.

## Consequências

- O que foi dito na sala não se perde mais, e o resumo para o Discord sai pronto.
- A privacidade é a mais forte possível sem servidor: o áudio não sai do PC, ninguém é transcrito sem aceitar, e o texto só fica guardado nos PCs da sala.
- **CPU ao lado do jogo:** o Whisper `small` no processador pode custar FPS em jogos que dependem de CPU. Precisa ser medido com o `perf-check` antes de lançar, e o lançamento precisa de dois números:
  - o fator de tempo real, com a meta de transcrever 1 s de fala em menos de 0,5 s, num processador de 6 núcleos;
  - a perda de FPS do jogo com uma reunião de 4 pessoas falando, com a meta de menos de 3%.
  - Se não der, o padrão vira o `base`.
- **Qualidade:** gíria, nomes de jogo e falas sobrepostas vão sair com erros. Como cada um transcreve só o próprio microfone, falas sobrepostas atrapalham menos do que numa gravação da call inteira.
- **Download grande:** o primeiro uso baixa o modelo, de 60 a 190 MB, com progresso na tela.
- **Atraso:** uma fala aparece na aba alguns segundos depois de terminar.
- **Build:** CMake passa a ser necessário para compilar o app.
- O Worker ganha 5 mensagens novas, `meeting-start`, `meeting-stop`, `meeting-line`, `meeting-history-request` e `meeting-history`, e o estado da reunião na sala. Elas entram no `docs/protocolo-malha.md`.

## Alternativas consideradas

- **Gravar a call inteira em um PC e transcrever lá:** quem falou teria que ser adivinhado pela voz, o que falha com vozes parecidas e falas ao mesmo tempo, e o áudio de todos passaria a ficar num PC só.
- **Whisper na nuvem (Workers AI da Cloudflare, onde a sinalização já roda):** mais rápido e sem custo de CPU, mas o áudio sairia do PC e cada minuto custaria dinheiro. Pode voltar como opção para PC fraco, sempre escolhida por quem fala.
- **Reconhecimento de fala do Windows (`Windows.Media.SpeechRecognition`):** sem download, mas a qualidade em português é bem pior que a do Whisper, e o ditado contínuo depende do serviço online da Microsoft.
- **Whisper em Rust puro (`candle`):** sem CMake, mas mais lento que o whisper.cpp na CPU, e a CPU é exatamente o recurso disputado com o jogo. Fica atrás da mesma trait, se a medição mostrar que vale.
- **Whisper na GPU:** mais rápido, mas disputa a GPU com o jogo e com o encode, o que o projeto evita.
- **Mandar as falas pelas conexões de voz (P2P):** o texto nem passaria pelo Worker. Mas a voz repassada (ADR 0020) só leva datagramas de áudio cifrados por par, e quem entra no meio ainda precisaria da sinalização para o histórico. Como o Worker já vê os sinais e não guarda nada, a difusão por ele sai muito mais simples, pelo mesmo nível de confiança.
- **Mandar cada fala por `signal` a cada membro:** em sala grande, uma fala viraria centenas de mensagens e bateria no limite de 100 a cada 10 s.
- **Webhook do Discord, para sair com as fotos:** exige que alguém da guilda crie e cole um webhook, e manda o texto para o Discord sem a pessoa revisar. Fica para depois, se pedirem.
