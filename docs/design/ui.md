# Design da interface

A interface segue a **hierarquia** do Discord, porque é onde a galera já vive, com identidade própria (ver [ADR 0005](../adr/0005-ui-estilo-discord.md)). Os tokens em código ficam em `apps/desktop/src/styles/index.css`.

## Layout

```
┌────┬──────────────┬─────────────────────────────────────────┐
│ SV │ Nome da sala │  Grade de streams                       │
│    │──────────────│  ┌───────────────┐ ┌───────────────┐    │
│ 🟣 │ 🔴 Ana  AO VIVO│ │  Ana (1080p60) │ │ Beto (720p30) │    │
│ 🟢 │ ⚪ Beto        │ └───────────────┘ └───────────────┘    │
│ ＋ │ ⚪ Caio        │                                         │
│    │──────────────│  [ Transmitir minha tela ]              │
│    │ 👤 Você   ⚙  │                                         │
└────┴──────────────┴─────────────────────────────────────────┘
 72px     240px                    flexível
```

- **Barra de salas (72 px):** ícones redondos que viram quadrados arredondados no hover; o botão ＋ cria sala.
- **Barra de título (32 px):** própria (janela sem moldura do Windows), na cor da barra de salas, arrastável, com o nome da sala no meio e os botões minimizar, maximizar e fechar.
- **Barra lateral (240 px):** canto superior esquerdo arredondado e borda fina, como no Discord; nome da sala e a sala como "canal de voz" com o tempo conectado e os membros.
- **Card do usuário:** flutua no pé da barra de salas e da lateral (largura das duas), com "Conectado à sala" em cima e o usuário embaixo.
- **Área principal:** grade de streams; clicar num stream expande para foco, com os outros em miniatura.

## Cores

| Token           | Valor     | Uso                                     |
| --------------- | --------- | --------------------------------------- |
| `surface-float` | `#111214` | Menus e dicas                           |
| `surface-0`     | `#1e1f22` | Barra de título, barra de salas, inputs |
| `surface-1`     | `#2b2d31` | Barra lateral, cards em modais          |
| `surface-panel` | `#232428` | Card flutuante do usuário               |
| `surface-2`     | `#313338` | Área principal, modais                  |
| `surface-3`     | `#383a40` | Botões neutros                          |
| `surface-hover` | `#404249` | Hover de itens                          |
| `text`          | `#f2f3f5` | Texto principal                         |
| `text-muted`    | `#b5bac1` | Texto secundário                        |
| `text-faint`    | `#80848e` | Dicas, placeholders                     |
| `brand`         | `#6e4ff6` | Ações principais, marca                 |
| `live`          | `#f23f43` | Selo AO VIVO                            |
| `online`        | `#23a55a` | Conectado, criar sala                   |
| `warning`       | `#f0b232` | Conexão instável                        |

Contraste mínimo: 4,5:1 para texto normal (WCAG AA).

### Vidro

Em Configurações → Aparência, o "Efeito de vidro" (0–100%, padrão 40%) deixa a estrutura translúcida: título e barra de salas, lateral, área principal, card do usuário e modais usam os tokens `glass-*`, que misturam as cores acima com transparência conforme `--glass`. Acima de 0%, a janela liga o Acrylic do Windows, e o desfoque do que está atrás fica a cargo do DWM, fora do processo do app. Em 0% tudo fica sólido, igual às cores da tabela. A escolha fica salva em `localStorage` (`squadview.vidro`).

## Tipografia

- Fonte: **Noto Sans** (livre, embutida via `@fontsource-variable/noto-sans`), a mesma que o Discord usa de reserva quando a gg sans dele não carrega. A gg sans e a ABC Ginto são proprietárias e não entram no app. Segoe UI Variable fica de reserva.
- Aplicação no padrão do Discord:
  - Cabeçalhos de seção ("SALAS DE VOZ", "NA SALA"): 12 px, maiúsculas, peso 600, espaçamento 0,02em (`text-xs font-semibold tracking-wide uppercase`).
  - Nomes de salas e itens de navegação: 16 px, peso 500.
  - Nomes de quem está numa sala e de conversas: 14 px, peso 500.
  - Corpo e descrições: 14 px, peso 400; textos de apoio em 12 px.
  - Títulos de tela e de modal: 20–24 px, peso 700.
- Texto com `-webkit-font-smoothing: antialiased` e `text-rendering: optimizeLegibility`, como no Discord.

## Componentes

- Primitivos acessíveis do **Radix UI** (Dialog, DropdownMenu, Tooltip, Slider, Switch) estilizados com Tailwind.
- Ícones **lucide-react**, 16–22 px.
- **Checkbox:** sempre `apps/desktop/src/ui/Checkbox.tsx`. Use `Checkbox` (com rótulo) em formulários e modais, e `CheckMark` (só a caixa) dentro dos `CheckboxItem` dos menus. Nunca `<input type="checkbox">` nativo nem caixa desenhada à mão. Ao marcar, a caixa enche com um pulo e o ✓ se desenha; ao desmarcar, o ✓ se apaga e a caixa esvazia.
- Raio: 4 px (botões pequenos), 8 px (botões), 12 px (cards), 16 px (ícones de sala).
- Animações de 120–200 ms com `ease-out`; nada que roube GPU enquanto alguém joga.

## Estados que toda tela precisa ter

- Vazio (ninguém transmitindo).
- Carregando (conectando à sala).
- Erro com explicação e ação ("Não deu para alcançar a Ana. Tentando por outro membro…").
- Conexão instável (selo amarelo com a camada atual).

## Acessibilidade

- Tudo navegável pelo teclado, com foco visível.
- Rótulos `aria-label` em botões que só têm ícone.
- Nada transmitido só por cor: o selo AO VIVO tem texto.

## Movimento

- Todo modal usa o componente `apps/desktop/src/ui/Modal.tsx`, que já traz as animações. Não crie modais com o Radix direto.
- **Abrir:** fundo escurece em 180 ms; a caixa sobe 12 px e cresce de 96% para 100% em 220 ms (`cubic-bezier(0.16, 1, 0.3, 1)`).
- **Fechar:** mais rápido (140 ms), no sentido inverso. O Radix só remove o modal depois que a animação termina.
- **Trocar de passo dentro do modal** (ex.: fonte → qualidade): o conteúdo novo entra deslizando 16 px da direita (`animate-step-in`), e a largura do modal se ajusta com transição.
- Quem ativou "Mostrar animações" desligado no Windows (`prefers-reduced-motion`) não vê animação nenhuma.
- Nada de animação contínua enquanto alguém joga, a não ser o carregamento das miniaturas (`animate-pulse`).

## Rolagem

- Toda área com rolagem usa `apps/desktop/src/ui/ScrollArea.tsx`, nunca `overflow-auto` com a barra nativa.
- A barra é sobreposta: fica por cima do conteúdo e não ocupa espaço. Tem 10 px de área de clique e um polegar de 6 px, arredondado, na cor `surface-0`.
- Aparece com o mouse em cima ou ao rolar (fade de 150 ms) e some 500 ms depois que o mouse sai (fade de 300 ms).

## Modal "Transmitir minha tela"

Segue o fluxo de dois passos do compartilhamento do Discord, com arte e textos próprios:

1. **Escolher a fonte:** abas "Aplicativos" (janelas) e "Telas" (monitores), com grade de miniaturas. Clicar num card vai para o passo 2. "Ficar ao vivo" fica desabilitado até escolher.
2. **Qualidade:** "O que você está transmitindo" (com "Trocar"), "Transmitindo para" e a qualidade, com resolução 720p / 1080p / 1440p / Fonte e taxa de quadros 15 / 30 / 60. **Tudo liberado, sem plano pago.** Embaixo, o teto estimado de upload por pessoa assistindo.
