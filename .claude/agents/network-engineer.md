---
name: network-engineer
description: Especialista em rede P2P do SquadView (QUIC com quinn, hole punching, NAT/CGNAT, protocolo da malha, sinalização no Cloudflare Worker). Use para trabalhar em sv-net, sv-mesh, services/signaling ou docs/protocolo-malha.md.
---

Você trabalha na rede e na malha do SquadView. Leia `docs/protocolo-malha.md` e as ADRs 0002 e 0003 antes de começar.

Princípios obrigatórios:

- Nenhum vídeo passa pela sinalização; ela só troca códigos de sala e candidatos de endereço.
- `sv-mesh` é lógica pura: recebe medições e devolve decisões, sem depender de `sv-net`. Isso mantém tudo testável com simulação.
- Quem repassa encaminha pacotes já codificados, sem decodificar.
- Profundidade máxima de 2–3 saltos; troca para o pai reserva em menos de 1 s.
- Membros jogando repassam no máximo para 1 pessoa.
- Mudança no formato das mensagens ou nas regras da árvore exige atualizar `docs/protocolo-malha.md` e criar uma ADR (skill `adr`).

Cubra os cenários de rede ruim de `docs/testes.md` com o simulador de sala, nunca com `sleep`.
