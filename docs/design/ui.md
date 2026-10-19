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
- **Barra lateral (240 px):** nome da sala, membros com status (ao vivo, assistindo, repassando) e o painel do usuário embaixo.
- **Área principal:** grade de streams; clicar num stream expande para foco, com os outros em miniatura.

## Cores

| Token           | Valor     | Uso                     |
| --------------- | --------- | ----------------------- |
| `surface-0`     | `#111214` | Barra de salas          |
| `surface-1`     | `#1a1b1e` | Barra lateral           |
| `surface-2`     | `#232428` | Área principal          |
| `surface-3`     | `#2e3035` | Cards, inputs           |
| `surface-hover` | `#35373c` | Hover de itens          |
| `text`          | `#f2f3f5` | Texto principal         |
| `text-muted`    | `#b5bac1` | Texto secundário        |
| `text-faint`    | `#80848e` | Dicas, placeholders     |
| `brand`         | `#6e4ff6` | Ações principais, marca |
| `live`          | `#f23f43` | Selo AO VIVO            |
| `online`        | `#23a55a` | Conectado, criar sala   |
| `warning`       | `#f0b232` | Conexão instável        |

Contraste mínimo: 4,5:1 para texto normal (WCAG AA).

## Tipografia

- Fonte: Inter, com Segoe UI Variable de reserva.
- Tamanhos: 12 (rótulos), 14 (corpo), 16 (títulos de seção), 20–24 (títulos de tela).
- Pesos: 400, 500 e 600. 700 só no logo.

## Componentes

- Primitivos acessíveis do **Radix UI** (Dialog, DropdownMenu, Tooltip, Slider, Switch) estilizados com Tailwind.
- Ícones **lucide-react**, 16–22 px.
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
