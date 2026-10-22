# Testes

## Pirâmide

| Nível             | Ferramenta                | O que cobre                                                                  |
| ----------------- | ------------------------- | ---------------------------------------------------------------------------- |
| Unidade (Rust)    | `cargo test`              | Lógica dos crates, principalmente `sv-mesh` (pura) e configurações de encode |
| Unidade (TS)      | Vitest + Testing Library  | Componentes e estado da interface; rotas e códigos da sinalização            |
| Integração (Rust) | `cargo test` em `tests/`  | Dois `sv-net` falando em loopback; simulador de sala para a malha            |
| Hardware          | `cargo test -- --ignored` | Captura, NVENC e controles virtuais (ViGEmBus) reais. Nunca no CI            |
| Performance       | skill `perf-check`        | Impacto no FPS do jogo, latência, uso do encoder                             |

## Metas de cobertura (linhas)

| Parte                                                    | Meta  |
| -------------------------------------------------------- | ----- |
| `sv-mesh`, `sv-core`                                     | ≥ 90% |
| Demais crates (sem os trechos que só rodam com hardware) | ≥ 70% |
| Interface                                                | ≥ 80% |
| Sinalização                                              | ≥ 90% |

A badge do README mostra a cobertura total somada (Rust + TypeScript).

## Convenções

- Nomes de teste em português, descrevendo o comportamento: `quem_esta_jogando_repassa_no_maximo_um`.
- Testes que precisam de GPU levam `#[ignore = "precisa de GPU NVIDIA"]`.
- Rede ruim é simulada no simulador de sala (perda, jitter, membro saindo), nunca com `sleep`.
- Todo bug corrigido ganha um teste que falharia antes da correção.

## Cenários obrigatórios da malha (Fase 3)

- Membro com upload alto entra e vira repassador.
- Repassador sai sem aviso: os filhos trocam de pai em menos de 1 s.
- Membro começa a jogar: deixa de ser repassador principal.
- Membro com 4G fraco recebe a camada baixa e os outros continuam na alta.
- Dois membros atrás de CGNAT se conectam através de um terceiro.

## Teste de performance (resumo)

Ver a skill `.claude/skills/perf-check`. Em resumo: medir o FPS médio e o 1% low de um jogo por 2 min sem transmitir e depois transmitindo em 1080p60. A diferença precisa ficar abaixo de 5%.
