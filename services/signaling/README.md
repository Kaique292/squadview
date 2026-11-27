# Sinalização (Cloudflare Worker)

O Worker que troca endereços entre os membros, guarda salas, amigos e servidores, e entrega as credenciais do repassador TURN ([ADR 0003](../../docs/adr/0003-sinalizacao-cloudflare-worker.md)). O vídeo e a voz nunca passam por aqui. As mensagens estão em [PROTOCOL.md](PROTOCOL.md).

```powershell
yarn workspace @squadview/signaling test   # vitest com cobertura
yarn workspace @squadview/signaling lint   # tsc
```

## Segredos

Os segredos ficam fora do repositório: `wrangler secret put <NOME>`.

| Segredo          | Para quê                                                    |
| ---------------- | ----------------------------------------------------------- |
| `SESSION_SECRET` | Assina as sessões do login com o Discord (ADR 0009 e 0011). |

## Repassador TURN

O repassador é opcional e só entra para quem não conecta direto, por exemplo atrás de CGNAT ([ADR 0014](../../docs/adr/0014-conexao-em-camadas.md)). O Worker usa o primeiro provedor completo, nesta ordem:

| Provedor                                                                                         | Segredos                                        | Credencial                                  |
| ------------------------------------------------------------------------------------------------ | ----------------------------------------------- | ------------------------------------------- |
| Cloudflare Realtime                                                                              | `TURN_KEY_ID`, `TURN_KEY_API_TOKEN`             | Da Cloudflare, uma por sala, vale 24 h      |
| Metered                                                                                          | `METERED_DOMAIN`, `METERED_API_KEY`             | Do Metered, uma por sala                    |
| **coturn próprio** ([ADR 0033](../../docs/adr/0033-coturn-proprio-com-credencial-temporaria.md)) | `TURN_URLS`, `TURN_SHARED_SECRET`               | Gerada no Worker, uma por pessoa, vale 24 h |
| Servidor fixo                                                                                    | `TURN_URLS`, `TURN_USERNAME`, `TURN_CREDENTIAL` | A mesma para todos, nunca vence             |

Sem nenhum provedor, a sala funciona só com conexão direta e com o repasse pela sala.

### coturn próprio com credencial temporária

O padrão é o "TURN REST API" do coturn. O Worker e o coturn dividem um segredo, e para cada membro que entra o Worker gera:

- **usuário:** `<vence_em_unix>:<id>`, com o id do Discord verificado ou o id da vaga na sala;
- **senha:** `base64(HMAC-SHA1(TURN_SHARED_SECRET, usuário))`.

O coturn confere a senha com o mesmo segredo e recusa a credencial depois do vencimento. A validade é `TURN_TTL_S`, em `src/turn.ts`.

**No Worker:**

```powershell
wrangler secret put TURN_URLS            # ex.: turn:187.53.103.68:3478?transport=udp,turn:187.53.103.68:3478?transport=tcp
wrangler secret put TURN_SHARED_SECRET   # o mesmo static-auth-secret do coturn
```

Para usar só o coturn, não configure os segredos da Cloudflare nem do Metered, que vêm antes. Se `TURN_SHARED_SECRET` existir, ele vale antes do par fixo (`TURN_USERNAME`/`TURN_CREDENTIAL`).

**No coturn (`turnserver.conf`), o mínimo que combina com o Worker:**

```ini
use-auth-secret
static-auth-secret=<o mesmo TURN_SHARED_SECRET>
realm=squadview
# Não deixe o repassador alcançar a rede interna da VPS.
no-multicast-peers
denied-peer-ip=10.0.0.0-10.255.255.255
denied-peer-ip=172.16.0.0-172.31.255.255
denied-peer-ip=192.168.0.0-192.168.255.255
denied-peer-ip=127.0.0.0-127.255.255.255
```

**Para trocar o segredo**, troque-o no Worker e no coturn ao mesmo tempo. As credenciais já entregues param de funcionar até a pessoa entrar de novo na sala.
