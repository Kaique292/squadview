# 0020. Voz repassada por um membro da sala quando dois membros não se alcançam

- **Status:** aceita
- **Data:** 2026-10-21

## Contexto

A voz da sala é uma malha: cada membro tem uma conexão QUIC de voz com cada outro ([ADR 0010](0010-chat-de-voz-na-sala.md)). A [ADR 0014](0014-conexao-em-camadas.md) pôs a conexão em camadas: direto por IPv4 e IPv6 e, depois, o repassador TURN. O repasse por um membro da sala ficou só para o vídeo ("a voz fica com o direto e o repassador").

Na prática, o repassador TURN está desligado (nenhum provedor configurado no Worker) e vai continuar assim: o projeto quer ficar de graça e sem servidor de mídia. Então, quando dois membros estão atrás de CGNAT simétrico e sem IPv6 em comum, a voz entre eles não abre. Foi o que aconteceu com o Lobo e o Nino em out/2026. Os dois falavam com o Kaique, que tem rede aberta, e o Kaique ouvia os dois, mas um não ouvia o outro. A mensagem ainda dizia que "o repassador da sala não respondeu", sem existir repassador.

## Decisão

Quando a voz entre dois membros não abre, **um terceiro membro que já fala com os dois repassa os pacotes de voz entre eles**, como já acontece com o vídeo. Os pacotes passam como estão, sem decodificar nem misturar, por uma conexão de repasse separada, e cada pacote leva o id de quem falou.

Como funciona:

1. **Quem pede.** Entre A e B, quem chama (o id menor, ADR 0010) é quem fica sabendo da falha. Nesse momento, A manda `voice-relay-request { to: B }` a um membro H com quem já tem voz. A ordem é do menor ping para o maior. Se H recusar ou não responder em 4 s, A tenta o próximo. Sem ninguém, a falha chega à interface como hoje.
2. **Quem ajuda.** H aceita se tiver voz aberta com B e vaga: cada membro repassa no máximo **6 pares**. A voz é leve (Opus, ~32 kbps por sentido), então 6 pares gastam menos de 400 kbps de upload. Ao aceitar, H responde `voice-relay-accept { to: B }` a A e manda `voice-relay-start { from: A }` a B. Se não puder, responde `voice-relay-deny { to: B, reason }`.
3. **A conexão de repasse.** H abre com A e com B uma **segunda conexão QUIC**, com ALPN próprio (`sv-voz-repasse`), no mesmo socket e para o mesmo endereço da conexão de voz que já existe. O caminho no NAT já está aberto, e a fingerprint de H já é conhecida. Os datagramas dessa conexão são `origem: 8 bytes (id do membro) | datagrama de áudio de sempre`.
4. **O repasse.** Cada pacote do microfone de A que chega a H pela voz normal é copiado para a conexão de repasse com B, com a origem A, e o de B vai para A do mesmo jeito. H continua ouvindo A e B normalmente. O repasse não passa pelo mixer de H, só copia bytes, sem alocar por pacote (um buffer por conexão).
5. **Quem recebe.** B só aceita na conexão de repasse pacotes de uma origem que H anunciou com `voice-relay-start`. Esses pacotes entram no mixer como a voz de A: o volume, o silenciar, o "falando" e o efeito de fala de A funcionam igual. Para a interface, A aparece conectado na voz, com a dica "pela conexão de H".
6. **Quando cai.** Se H sair da sala, ou perder a voz com A ou com B, a conexão de repasse fecha. A pede de novo a outro membro, e B espera. Se a conexão direta entre A e B abrir depois (por exemplo, numa nova tentativa ao reconectar), ela substitui o repasse, e o repasse do par fecha.

Ordem das camadas para a voz: **direto (IPv4 e IPv6) → TURN, se configurado → repasse por um membro da sala.**

Nada muda no Worker: os `signal` já passam como estão, e o tamanho cabe no limite.

Junto, a mensagem de falha diz a causa de verdade. Sem repassador configurado, ela diz que nenhuma conexão fechou, em vez de "o repassador da sala não respondeu", e só cita o repassador quando ele existe.

## Consequências

- Em qualquer sala com pelo menos um membro de rede aberta (ou com IPv6 alcançável pelos dois), todo mundo se ouve, sem TURN e sem custo.
- Quem repassa gasta um pouco de upload e de CPU (cópia de pacotes, sem decodificar). O limite de 6 pares mantém isso abaixo de ~400 kbps. Não mexe na thread de áudio nem na de mídia: o repasse roda na tarefa de rede que já lê os datagramas.
- A voz repassada chega com o atraso de um salto a mais: o ping de A até H somado ao de H até B. Em salas de amigos na mesma cidade, isso dá dezenas de ms.
- Se todos estiverem atrás de CGNAT, ninguém consegue repassar. Aí só o IPv6 ou um TURN resolvem, e a mensagem de falha diz isso.
- **Confiança:** H já ouve A e B (está na mesma sala), então não vê nada novo. Mas H poderia mandar a B um áudio falso com a origem A. B só aceita origens anunciadas por H e só durante o repasse, o que limita o falso aos pares que H repassa. Como a sala é de amigos que se convidam, aceitamos esse risco. Assinar cada pacote custaria CPU e banda por pacote.
- Cada trecho (A–H e H–B) continua cifrado pelo QUIC.
- **Compatibilidade:** apps antigos ignoram as mensagens novas. Um H antigo não responde, e A tenta o próximo. Um B antigo não aceita o ALPN de repasse, e H responde `voice-relay-deny` a A.
- A [ADR 0014](0014-conexao-em-camadas.md) deixa de ter "a voz fica com o direto e o repassador": a voz ganha a camada do meio.

## Alternativas consideradas

- **Ligar o TURN da Cloudflare:** resolve todos os casos, inclusive a sala em que todos estão atrás de CGNAT, e a voz caberia na cota grátis. Mas depende de servidor e conta de terceiros, e pode pedir cartão. O projeto quer funcionar de graça e sem servidor de mídia. Fica como opção para quem hospedar o Worker.
- **Repasse pelos mesmos datagramas da conexão de voz, com um byte de tipo:** dispensa a segunda conexão, mas muda o formato do datagrama de voz. Apps antigos tocariam o pacote repassado como se fosse a voz de quem repassa (o datagrama de hoje não tem tipo). A conexão separada deixa o formato atual intocado.
- **Misturar o áudio em quem repassa (como um servidor de conferência):** economiza upload em salas grandes, mas decodifica e codifica de novo (CPU, atraso e qualidade) e tira de quem ouve o volume e o silenciar por pessoa.
- **Repassar pelo Worker (WebSocket):** o Worker não fala UDP, e a voz por WebSocket (50 pacotes por segundo por pessoa) estouraria o limite grátis de pedidos em poucas horas de call.
- **Adivinhar portas do NAT simétrico (port prediction):** a ADR 0014 já descartou. Falha muito com dois CGNAT e gera rajadas de pacotes.
