# Desenvolvimento

## Pré-requisitos

| Ferramenta                | Versão                                             | Como instalar                                                                                                                                   |
| ------------------------- | -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows                   | 10 (2004+) ou 11                                   | —                                                                                                                                               |
| Visual Studio Build Tools | 2022+ com **Desenvolvimento para desktop com C++** | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |
| Rust                      | 1.95 (fixado em `rust-toolchain.toml`)             | <https://rustup.rs> — o rustup baixa a versão certa sozinho                                                                                     |
| Node.js                   | 24                                                 | <https://nodejs.org>                                                                                                                            |
| Yarn                      | 4 (via corepack)                                   | `corepack enable`                                                                                                                               |
| WebView2                  | já vem no Windows 11                               | —                                                                                                                                               |
| cargo-llvm-cov            | última                                             | `cargo install cargo-llvm-cov`                                                                                                                  |
| cargo-deny                | última                                             | `cargo install cargo-deny`                                                                                                                      |

A partir da Fase 1, uma placa NVIDIA (GTX 10xx ou mais nova) é necessária para testar o encoder. O SDK do NVENC é carregado em tempo de execução pelo driver; não precisa instalar nada além do driver atualizado.

> **Git Bash:** o `link` do Git Bash esconde o `link.exe` do MSVC e quebra o build do Rust. Rode `cargo` e `yarn tauri` pelo **PowerShell** ou pelo terminal do VS Code com PowerShell.

## Primeiros passos

```powershell
git clone https://github.com/Kaique292/squadview.git
cd squadview
git switch dev
corepack enable
yarn install      # também instala os hooks do git (husky)
yarn dev          # abre o app em modo desenvolvimento
```

## Comandos

| Comando                                   | O que faz                                                                                                             |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `yarn dev`                                | Abre o app com recarga automática da interface                                                                        |
| `yarn build`                              | Gera o instalador NSIS em `target/release/bundle/nsis/`                                                               |
| `yarn build:release`                      | Como o `build`, mas assina para a atualização automática (precisa de `TAURI_SIGNING_PRIVATE_KEY`; usado pela esteira) |
| `yarn lint`                               | Prettier, ESLint, TypeScript, `cargo fmt` e `cargo clippy`                                                            |
| `yarn format`                             | Formata tudo (Prettier + rustfmt)                                                                                     |
| `yarn test`                               | Testes de Rust e TypeScript com cobertura                                                                             |
| `yarn test:rs`                            | Só Rust (`cargo llvm-cov`)                                                                                            |
| `yarn test:js`                            | Só interface e sinalização (Vitest)                                                                                   |
| `yarn coverage:badge`                     | Junta as coberturas e gera `build/badges/coverage.json`                                                               |
| `yarn workspace @squadview/signaling dev` | Sobe o Worker de sinalização localmente                                                                               |

## Estrutura

```
apps/desktop/         app Tauri (src = React, src-tauri = Rust)
crates/sv-*           núcleo em Rust (ver docs/arquitetura.md)
services/signaling/   Cloudflare Worker de sinalização
scripts/              scripts de apoio (badge de cobertura)
docs/                 documentação, ADRs, design e imagens
.claude/              agentes e skills do Claude para o projeto
```

## Fluxo do dia a dia

1. Atualize a `dev`: `git switch dev && git pull`.
2. Crie a branch: `git switch -c feat/captura-de-janela`.
3. Faça commits pequenos no padrão semântico (o hook confere). Veja [git-e-versionamento.md](git-e-versionamento.md).
4. Rode `yarn lint` e `yarn test`.
5. Abra o PR para a **`dev`** com título semântico.

## Capturas do README e imagem de prévia do site

- **Capturas das telas:** com o `yarn dev` rodando, abra `http://localhost:1420/#demo=sala` (ou `#demo=transmitir`, `#demo=config`). O modo de demonstração (`apps/desktop/src/dev/demo.ts`, só no dev) preenche a interface com uma sala de exemplo. Para gerar o PNG sem abrir janela:

  ```powershell
  & "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --headless=new --hide-scrollbars --window-size=1280,800 --virtual-time-budget=6000 --screenshot=docs\images\tela-sala.png "http://localhost:1420/#demo=sala"
  ```

- **Imagem de prévia do site** (`site/og.png`, 1200×630, o card que aparece ao colar o link no Discord): a fonte é `site/og/card.html`. Gere com o mesmo comando, usando `--window-size=1200,630` e o caminho do arquivo no lugar da URL.
