---
name: adr
description: Registra uma decisão de arquitetura do SquadView como ADR numerada em docs/adr. Use quando uma escolha técnica importante for feita ou mudada.
---

# Nova ADR

1. Descubra o próximo número: maior `NNNN` em `docs/adr/` + 1.
2. Copie `docs/adr/0000-template.md` para `docs/adr/NNNN-titulo-em-kebab-case.md`.
3. Preencha: status (`proposta` até o Kaique aprovar, depois `aceita`), data de hoje, contexto, decisão em 1–2 frases, consequências (boas e ruins) e alternativas com o motivo de não terem sido escolhidas.
4. Se substituir uma ADR antiga, mude o status da antiga para `substituída por [NNNN](...)`.
5. Adicione a linha na tabela "Decisões registradas" de `docs/README.md`.
6. Se a decisão mexe no protocolo da malha, atualize também `docs/protocolo-malha.md`.
