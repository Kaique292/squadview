# SquadView — instruções para o Claude

App de Windows para compartilhar a tela entre amigos em call, com qualidade de jogo e **sem servidor de mídia**: a própria sala distribui o vídeo. Leia `docs/arquitetura.md` e `docs/roadmap.md` antes de mudanças grandes.

## Regras que não se negociam

- **Nunca** adicione `Co-authored-by:` (nem qualquer assinatura de IA) em commits ou PRs. O projeto tem um único autor; o commitlint recusa o commit.
- **Não commite, não dê push, não abra PR e não altere configurações do GitHub sem o Kaique pedir explicitamente.**
- Commits seguem `docs/git-e-versionamento.md` (tipo, escopo da lista, resumo em português no imperativo).
- PRs de trabalho vão para a **`dev`**. A `main` só recebe PR vindo da `dev`.
- Nunca edite a versão à mão (`Cargo.toml`, `package.json`, `tauri.conf.json`): o release-please cuida disso.
- Decisão de arquitetura nova ou mudança no protocolo da malha → nova ADR em `docs/adr/` (skill `adr`).

## Performance primeiro

O app roda ao lado de jogos. Em código de captura, encode, rede e renderização:

- A imagem não sai da GPU (nada de copiar textura para a CPU).
- Nada de alocação por frame no caminho quente; reutilize buffers.
- Nada de bloquear a thread de mídia com I/O ou locks disputados.
- Toda mudança nesses crates diz no PR como foi medida (skill `perf-check`).

## Mapa do código

| Caminho              | O quê                                 |
| -------------------- | ------------------------------------- |
| `crates/sv-core`     | Tipos comuns                          |
| `crates/sv-capture`  | Captura de tela e áudio (WGC, WASAPI) |
| `crates/sv-encode`   | Trait `Encoder` + NVENC / AMF         |
| `crates/sv-media`    | Decode D3D11VA, render, Opus          |
| `crates/sv-net`      | QUIC P2P, hole punching               |
| `crates/sv-mesh`     | Árvore de repasse (lógica pura)       |
| `apps/desktop`       | Tauri v2 + React 19 + Tailwind v4     |
| `services/signaling` | Cloudflare Worker de sinalização      |

## Comandos

- `yarn lint`, `yarn test`, `yarn dev`, `yarn build` (ver `docs/desenvolvimento.md`).
- Rode `cargo` pelo **PowerShell**: no Git Bash o `link` do MSYS esconde o `link.exe` do MSVC.

## Estilo

- Código, nomes de tipos e funções em inglês; comentários, docs, testes (nomes) e mensagens ao usuário em **português**.
- Interface segue `docs/design/ui.md` (hierarquia do Discord, identidade própria, sem assets do Discord).
- Rust: `cargo fmt` e `clippy -D warnings` limpos. TS: ESLint + Prettier limpos.

## Agentes e skills do projeto

- Agentes em `.claude/agents/`: `media-engineer`, `network-engineer`, `ui-engineer`, `qa-engineer`, `reviewer`.
- Skills em `.claude/skills/`: `commit`, `pr`, `release`, `adr`, `fase`, `perf-check`.
