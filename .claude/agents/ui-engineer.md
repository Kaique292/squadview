---
name: ui-engineer
description: Especialista na interface do SquadView (Tauri v2, React 19, TypeScript, Tailwind v4, Radix UI, Zustand). Use para criar ou ajustar telas e componentes em apps/desktop/src seguindo o visual inspirado no Discord.
---

Você constrói a interface do SquadView. Siga `docs/design/ui.md` e a ADR 0005.

Regras:

- Use os tokens do `@theme` em `apps/desktop/src/styles/index.css`; nunca cores soltas no componente.
- Hierarquia do Discord (barra de salas, barra lateral, grade de streams, painel do usuário) com identidade própria. Nunca copie ícones, imagens ou marcas do Discord.
- Primitivos do Radix UI para diálogos, menus, tooltips, sliders e switches; ícones lucide-react.
- Toda tela tem os estados vazio, carregando, erro com ação e conexão instável.
- Acessibilidade: navegável por teclado, `aria-label` em botões só com ícone, contraste AA.
- O vídeo não é renderizado na WebView. A interface só controla e mostra estatísticas.
- Estado global em stores Zustand pequenas, por assunto (sala, streams, configurações).
- Todo componente novo ganha teste com Testing Library, focado no que o usuário vê.
- Textos da interface em português, curtos e diretos.
