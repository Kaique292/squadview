# 0003. Sinalização num Cloudflare Worker

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

Antes de se conectar direto, os membros precisam se encontrar e trocar endereços. Sem algum ponto de encontro na internet isso não é possível.

## Decisão

Um **Cloudflare Worker** com um **Durable Object por sala** (`services/signaling`). Ele cria códigos de sala e repassa candidatos de endereço por WebSocket. **Nenhum vídeo passa por ele.**

## Consequências

- Cabe no plano gratuito: são poucos KB por entrada em sala.
- Quem quiser pode publicar o próprio Worker e apontar o app para ele (configuração).
- Dependemos da Cloudflare para entrar em salas, mas não para o stream em andamento.

## Alternativas consideradas

- **DHT/libp2p:** sem ponto central, mas entrar numa sala fica lento e complexo.
- **Código de convite com SDP colado à mão:** zero infraestrutura, mas péssima experiência.
