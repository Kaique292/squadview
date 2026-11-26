# 0033. Repassador TURN próprio (coturn) com credencial temporária

- **Status:** proposta
- **Data:** 2026-11-26

## Contexto

A [ADR 0014](0014-conexao-em-camadas.md) usa um repassador TURN para quem não conecta direto (CGNAT). O provedor é plugável e escolhido pelos segredos do Worker:

- Cloudflare Realtime, pago por uso depois de 1.000 GB/mês;
- Metered, com 20 GB/mês grátis;
- um servidor fixo com usuário e senha.

Naquela época, um coturn próprio foi descartado por exigir uma máquina ligada o tempo todo.

Agora temos uma VPS, a Konoha (`187.53.103.68`), e queremos usar um coturn nela no lugar da Cloudflare e do Metered. Mas o provedor fixo entrega **o mesmo usuário e a mesma senha, que nunca vencem**, a todo membro de qualquer sala. Quem copiasse a senha do app poderia usar a nossa VPS como repassador para qualquer coisa, para sempre, e não daria para saber quem foi.

## Decisão

Um 4º provedor usa o coturn com credencial temporária, no padrão "TURN REST API" do coturn (`use-auth-secret` e `static-auth-secret`).

- O Worker e o coturn dividem um segredo, `TURN_SHARED_SECRET`.
- Para cada membro que entra na sala, o Worker gera:
  - **usuário:** `<vence_em_unix>:<id>`, com o id do Discord verificado, ou o id da vaga na sala para quem não entrou com o Discord;
  - **senha:** `base64(HMAC-SHA1(segredo, usuário))`.
- A credencial vale `TURN_TTL_S` (24 h), como as dos outros provedores, e uma nova é gerada a cada entrada na sala.
- **Ordem dos provedores:** Cloudflare, Metered, coturn com segredo e, por último, o par fixo. Se `TURN_SHARED_SECRET` existir, ele vale antes do par fixo. Para usar só a VPS, basta não configurar Cloudflare nem Metered.
- A mensagem `turn` não muda, e o app desktop não muda.

## Consequências

- **Segurança:**
  - Uma senha vazada para de servir em até 24 h.
  - Sem o segredo, que só o Worker e o coturn têm, ninguém gera credenciais novas.
  - O log do coturn mostra quem usou o repassador.
- **Custo e latência:** o tráfego repassado sai da banda da VPS, sem custo por GB, e a VPS fica no Brasil, perto dos jogadores. Uma live de 8 Mbps repassada por uma hora gasta cerca de 3,6 GB por espectador.
- **Pontos de atenção:**
  - Agora temos uma máquina para manter, com atualizações do coturn, firewall e monitoramento da banda. Se ela cair, quem depende do repassador volta para o repasse pela sala ([ADR 0014](0014-conexao-em-camadas.md)).
  - **Trocar o segredo:** é preciso trocá-lo no Worker e no coturn ao mesmo tempo. As credenciais já entregues param de funcionar até a pessoa entrar de novo na sala.
- **No Worker:**
  - O HMAC é calculado no próprio Worker, pelo Web Crypto, sem chamar ninguém.
  - Cada membro recebe a sua credencial. Os provedores de fora continuam com uma credencial por sala, guardada no `TurnCache`.
- **Configuração do coturn, fora deste repositório:**
  - `use-auth-secret` com o mesmo `static-auth-secret` do Worker;
  - `realm`;
  - portas de repasse limitadas;
  - `no-multicast-peers`, e bloqueio de IPs privados e da própria rede como destino;
  - TLS no 5349, se for usar `turns:`.

## Alternativas consideradas

- **Par fixo de usuário e senha (o provedor atual):** a senha nunca vence e é a mesma para todos, então qualquer um poderia usar a VPS como repassador.
- **API de credenciais própria na VPS, como a do Metered:** o Worker chamaria a VPS a cada sala. Daria mais trabalho e mais um ponto de falha para chegar ao mesmo resultado que o HMAC dá sem rede.
- **Credencial por sala, como a da Cloudflare:** daria uma chamada a menos, mas misturaria todo mundo da sala num usuário só. Por pessoa, o log do coturn mostra quem usou o repassador, e o custo é só um HMAC por entrada.
