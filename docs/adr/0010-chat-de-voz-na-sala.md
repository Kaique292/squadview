# 0010. Chat de voz na sala: malha completa de QUIC, Opus mono e supressão de ruído local

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

Até aqui o SquadView dependia da call do Discord para a galera se ouvir. A ideia é que, ao entrar numa sala, todos já se ouçam, com os controles do Discord: silenciar o microfone, ensurdecer, volume e silenciar por pessoa (só para mim), escolher microfone e saída e ver quem está falando. As salas têm no máximo 8 pessoas e o projeto não tem servidor de mídia (ADR 0002).

## Decisão

- **Topologia:** malha completa. Cada par de membros abre uma conexão QUIC só para voz, no máximo 28 numa sala cheia. Voz é leve (~32 kbps por pessoa), então mandar para 7 amigos custa menos de 250 kbps de upload, e não há o atraso de um repasse.
- **Quem chama:** entre dois membros, quem tem o `peerId` menor manda `voice-offer`, o outro responde `voice-answer`, e quem chamou conecta. É o mesmo combinado de candidatos, fingerprints e hole punching do `watch` (ADR 0008).
- **Propósito da conexão:** ALPN próprio, `squadview-voice/1`, ao lado do `squadview/1` do vídeo. Quem aceita sabe pela ALPN se a conexão é de voz e, pela fingerprint do certificado, de quem é. Ninguém se identifica por uma mensagem que pode ser forjada.
- **Áudio:**
  - Microfone pelo WASAPI compartilhado, em 48 kHz mono.
  - Supressão de ruído com RNNoise (`nnnoiseless`, em Rust puro, roda local).
  - Detecção de voz: quem está em silêncio não manda pacotes.
  - Opus mono no modo VOIP, a 32 kbps, com quadros de 20 ms e FEC na ida.
  - Cada pacote é um datagrama QUIC, no mesmo formato do áudio do jogo.
- **Do outro lado:**
  - Um jitter buffer e um decoder por pessoa.
  - Um mixer só, com volume e silenciar por pessoa, que toca numa única saída WASAPI escolhida pelo usuário.
  - "Ensurdecer" corta a saída e o microfone, como no Discord.
- **Estado de voz visível para todos:** silenciado e ensurdecido vão no `status` da sinalização, como o `live`. Quem está falando sai do nível de cada pacote recebido, sem mensagem extra.

## Consequências

- A voz não passa por servidor nenhum, e a sala inteira se ouve com o app aberto.
- Sem cancelamento de eco nesta versão: quem usa caixa de som pode gerar eco. O app recomenda fone, e o eco fica para depois.
- Quem não consegue conexão direta com um membro (NAT simétrico) não ouve essa pessoa. A ponte por outro membro (Fase 3) também resolve a voz.
- Apps 0.1.0 ignoram `voice-offer` e continuam funcionando, só que sem voz.

## Alternativas consideradas

- **Um membro mixando para todos:** menos conexões, mas centraliza o upload num PC e soma atraso.
- **Reaproveitar a conexão do vídeo:** só existe quando alguém assiste alguém; a voz precisa estar sempre ligada entre todos.
- **WebRTC na WebView:** traria AEC e supressão prontos, mas duplicaria a pilha de rede e de NAT que o projeto já tem em QUIC.
