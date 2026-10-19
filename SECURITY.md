# Segurança

## Como reportar uma vulnerabilidade

Encontrou uma falha de segurança? **Não abra uma issue pública.** Use o botão **Report a vulnerability** na aba [Security](https://github.com/Kaique292/squadview/security) do repositório: a mensagem chega só para o mantenedor. Descreva o problema, como reproduzir e o impacto. A resposta vem em até 7 dias.

Apenas a versão mais recente recebe correções.

## Princípios de segurança do SquadView

> O projeto está em desenvolvimento. Os itens abaixo são as regras de projeto que cada fase precisa cumprir, e esta seção será atualizada conforme forem implementados.

### Seus dados

- **Sem telemetria.** O app não envia estatísticas nem dados de uso.
- O app fala só com: o serviço de sinalização (para entrar em salas), os outros membros da sala e o GitHub (para conferir atualizações).
- **Nenhum vídeo passa pelo serviço de sinalização.** Ele só vê o código da sala e os endereços de rede necessários para os membros se conectarem.

### Rede

- Toda conexão entre membros é cifrada (QUIC com TLS 1.3).
- A identidade de cada membro é conferida pela sala antes de qualquer troca de mídia.
- Tudo que chega de outro membro (tamanhos, índices, mensagens) é validado antes de ser usado; mensagem inválida derruba a conexão daquele membro, não o app.
- Meta da Fase 3: quem repassa o stream encaminha a mídia cifrada pelo streamer, sem conseguir alterá-la.

### Limites conhecidos

- **Membros da sala veem o seu endereço IP**, como em qualquer conexão P2P (jogos online, chamadas diretas). Entre só em salas de pessoas em quem você confia.
- **Quem está na sala vê o que você transmite.** Confira qual janela ou monitor está sendo compartilhado antes de começar.
- **O app não tem assinatura digital paga**, por isso o Windows pode mostrar um aviso do SmartScreen ao instalar. Confira o SHA-256 do instalador com o `SHA256SUMS.txt` da release.

### Atualizações e dependências

- Atualizações só vêm das releases deste repositório.
- As GitHub Actions são fixadas por commit, o `cargo-deny` confere licenças e vulnerabilidades conhecidas a cada PR, e o Dependabot avisa sobre atualizações.
