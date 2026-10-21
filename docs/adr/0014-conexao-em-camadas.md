# 0014. Conexão em camadas: direto (IPv4 e IPv6), depois o repassador TURN de quem aceita

- **Status:** proposta
- **Data:** 2026-10-20

## Contexto

Usuários atrás de CGNAT (internet compartilhada da operadora, comum em fibra e 4G no Brasil) não conseguiam assistir nem falar com alguns amigos. O app descobre o endereço público por STUN e "fura" o NAT, mas o CGNAT costuma ser **simétrico**: ele usa uma porta diferente para cada destino, e o furo não fecha. A ADR [0012](0012-malha-em-enxame.md) já previa a ponte por um membro com rede aberta e um repassador na nuvem como último recurso, mas nenhum dos dois existia: a conexão só falhava com uma mensagem. Em salas pequenas (2 a 4 amigos) é comum ninguém ter rede aberta.

## Decisão

A conexão entre dois membros (vídeo e voz) vai em camadas: **primeiro direto, por IPv4 e IPv6 ao mesmo tempo**; se não fechar em 1,5 s, tenta também pelo **repassador TURN** (provedor plugável), pelo endereço que **quem aceita** a conexão pegou emprestado lá. Se nem assim conectar (ou não houver repassador configurado), quem quer assistir pede a um **membro da sala que já assiste** o mesmo stream para repassar.

**Repasse pela sala** (vídeo e som do jogo; a voz fica com o direto e o repassador): quem já assiste recebe os frames codificados e os reenvia como estão, sem decodificar de novo, como na malha da Fase 3. Quem pede manda `relay-request` (com o id de quem transmite, seus endereços e fingerprint) a um membro cujo `watching` é aquele stream; quem ajuda responde com o mesmo `watch-accept` de quem transmite, e os pedidos de keyframe sobem até quem transmite. Se ele recusar ou não responder em 6 s, tenta o próximo; sem ninguém, a falha chega com o motivo da conexão direta. Cada membro repassa para no máximo 3 pessoas, porque é o upload dele que paga.

Ordem hoje: direto e repassador TURN (1,5 s depois) correm juntos; o repasse pela sala entra quando os dois falham. Com o repassador ligado, ele vem antes do repasse pela sala, porque é mais rápido de tentar. Se o custo do TURN pesar, dá para inverter (pedir repasse a um membro antes de usar o TURN).

**IPv6** é a saída grátis: o CGNAT só existe no IPv4, e muitas operadoras que usam CGNAT já entregam IPv6, em que cada PC tem endereço público. O app abre um segundo socket só IPv6 (o caminho IPv4 não muda), manda o endereço IPv6 global do PC como mais um candidato, e os dois lados mandam o "furo" um para o outro, o que abre o firewall do roteador.

Como funciona:

- O Worker consegue credenciais TURN no provedor configurado e manda uma mensagem `turn` para quem acabou de entrar na sala. Só membros da sala recebem. O provedor é **plugável** e escolhido pelos segredos: Cloudflare Realtime (`TURN_KEY_ID`/`TURN_KEY_API_TOKEN`), Metered (`METERED_DOMAIN`/`METERED_API_KEY`, plano grátis de 20 GB/mês) ou um servidor fixo, como um coturn próprio (`TURN_URLS`/`TURN_USERNAME`/`TURN_CREDENTIAL`). Sem nenhum configurado, nada muda.
- Cada app pede um endereço emprestado (Allocate) num **socket UDP separado**. O QUIC desse lado roda num socket próprio que embrulha cada pacote numa Send indication e desembrulha as Data indications. O caminho direto continua igual, com GSO e sem cabeçalho extra.
- Quem aceita (quem transmite, ou quem responde à voz) dá permissão no repassador aos IPs de quem pediu e manda o endereço emprestado no campo opcional `relay` do `watch-accept` e do `voice-answer`.
- Quem conecta tenta os endereços diretos e, depois de 1,5 s, também o `relay`, mandando UDP comum para lá. Isso atravessa qualquer NAT, porque é tráfego de saída. Fica a primeira conexão que fechar.
- Basta um dos lados ter endereço no repassador, então dois membros atrás de CGNAT se conectam.
- Como quem conecta tenta vários caminhos ao mesmo tempo, às vezes dois completam. Quem aceita segura cada conexão nova por 250 ms e descarta a que fecha nesse tempo (a perdedora). Sem isso, a voz trocava a conexão boa pela que ia fechar.

## Consequências

- Com IPv6 dos dois lados, quem está atrás de CGNAT conecta direto, de graça e sem desvio.
- Numa sala com alguém que já assiste e é alcançável, quem está atrás de CGNAT assiste de graça pelo repasse. O vídeo chega com o atraso de um salto a mais, e quem ajuda gasta upload (no máximo 3 repasses). Se quem ajuda parar de assistir, quem recebia por ele cai e precisa pedir de novo.
- Com o repassador configurado, todo mundo conecta, inclusive em sala de 2 e com os dois atrás de CGNAT. A voz também.
- O repassador só leva tráfego quando o direto falha. Quem conecta direto não paga nada nem sente diferença.
- **Custo:** a Cloudflare cobra a saída do repassador para quem recebe: os primeiros 1.000 GB do mês são grátis (cota dividida com o SFU do Realtime) e depois são US$ 0,05/GB. Uma live de 8 Mbps por uma hora gasta ~3,6 GB por espectador repassado. Precisamos acompanhar o consumo no painel. A camada do meio (repasse por membro, Fase 3) existe para reduzir esse custo.
- O vídeo de quem está atrás de CGNAT passa por terceiros. Continua cifrado de ponta a ponta pelo QUIC (TLS 1.3 com fingerprint combinada); o repassador só vê pacotes cifrados.
- Pelo repassador, os pacotes ficam em 1200 bytes, sem procurar MTU maior, por causa dos 36 bytes de cabeçalho do TURN. Isso dá um pouco menos de vazão por pacote que o direto.
- A credencial vale 24 h. Uma sala aberta por mais tempo perde o repassador. A conexão direta continua, e quem já está conectado pelo repassador continua até a alocação vencer.
- Compatível com apps antigos: o campo `relay` é opcional e a mensagem `turn` é ignorada por quem não a conhece.
- Para ligar, quem hospeda o Worker configura os segredos de um dos provedores. Por enquanto nenhum está configurado: o código fica pronto e desligado, e a sala usa só a conexão direta (IPv4 e IPv6).

## Alternativas consideradas

- **Só repasse por um membro da sala:** custo zero e mantém "sem servidor de mídia", mas não resolve a sala de 2 nem a sala em que todos estão atrás de CGNAT, que é o caso mais comum de quem reclamou. Fica como camada do meio na Fase 3.
- **Furar NAT simétrico adivinhando portas (port prediction):** funciona às vezes, falha muito em CGNAT de operadora e gera rajadas de pacotes. Não vale a complexidade.
- **TURN no mesmo socket do QUIC:** evitaria o socket extra, mas o caminho direto perderia GSO/GRO (quinn-udp agrupa pacotes de mesmo tamanho), ou teria de separar pacotes do repassador no meio do caminho quente. Um socket só para o repassador mantém o direto intocado.
- **Servidor TURN próprio (coturn):** dá controle total, mas exige uma máquina ligada o tempo todo, banda e manutenção. A Cloudflare já está na sinalização e cobra só pelo uso.
