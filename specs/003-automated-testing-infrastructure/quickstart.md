# Quickstart: Validação da Infraestrutura de Testes Automatizada

**Feature**: `003-automated-testing-infrastructure` | **Data**: 2026-09-01

Guia para validar manualmente, ponta a ponta, as quatro User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de `specs/001-*`/`specs/002-*`.

## Pré-requisitos

- Rust estável ≥ 1.75, `cargo`, `iced` já em `0.14` (`research.md` D1 — pré-requisito da própria
  feature, não algo que o usuário precisa instalar à parte).
- Python 3.11+ no `PATH` (plugins de referência).
- `xvfb` instalado (`sudo apt-get install -y xvfb` ou equivalente da distro) — só necessário para o
  Cenário 1 (Camada 2 do harness, `contracts/e2e-harness-contract.md`); os demais cenários não
  precisam de display.
- `cargo insta` instalado (`cargo install cargo-insta`) — só necessário para revisar/aceitar um novo
  snapshot visual localmente (Cenário 4); não é pré-requisito para rodar a suíte.

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

## Cenário 1 — Harness de execução real detecta sucesso e falha (User Story 1, P1)

```bash
# Caminho feliz: harness completo, ambas as camadas
cargo test --workspace --test e2e_harness
xvfb-run -a target/debug/farol &   # ou: ./tests/integration/harness.sh
```

**Esperado**: os testes de `e2e_harness.rs` passam — cada plugin conhecido (`git-local`,
`uptime-kuma`, contra fixtures sintéticas, `data-model.md` §1) alcança o `PluginState` esperado
dentro de 30s; nenhum processo filho remanescente. `harness.sh` confirma que o binário real sobe,
sobrevive à janela de observação e encerra limpo.

**Regressão deliberada (prova que o harness pega o defeito, `research.md` D1 gate de spike)**:
reintroduzir manualmente um closure capturante em um dos dois pontos de `Farol::subscription()`
documentados em `AGENTS.md` (ex.: `.map(move |event| Message::Worker { plugin_name:
worker_plugin_name.clone(), event })` sem antes mover `plugin_name` para dentro do stream) e rodar
`cargo test --workspace --test e2e_harness` de novo:

**Esperado**: o teste correspondente falha, com o panic do `debug_assert!` de
`iced::Subscription::map` visível na saída — nenhuma investigação manual necessária para saber qual
verificação falhou (Acceptance Scenario 3 de US1, SC-001). Reverter a regressão antes de continuar.

## Cenário 2 — Verificação de contrato pega valor de borda que a implementação rejeita (User Story 2, P2)

```bash
cargo test -p farol-protocol
```

**Esperado**: o caso já conhecido nesta sessão de planejamento (`research.md` D3) **falha** na
primeira execução após esta feature ser implementada — `response_time_ms: -1` (permitido pelo
schema, `MonitorStatusItem.response_time_ms` sem `minimum`) rejeitado por `Option<u32>`. Este é o
resultado esperado e correto: a suíte prova que o mecanismo funciona (SC-002) revelando um gap real
já existente, cuja correção é um item separado (`## Out of Scope` de `spec.md`). Depois que esse
item for corrigido (fora desta feature), o mesmo teste passa a ficar verde sem nenhuma mudança no
gerador.

**Regressão deliberada** (depois que o gap acima já estiver corrigido em uma feature futura):
apertar deliberadamente um tipo Rust além do que o schema permite (ex.: trocar um campo `Option<T>`
nullable por `T` não-nullable, contrariando `"type": [..., "null"]` do schema) e confirmar que
`cargo test -p farol-protocol` falha, apontando `schema_file`/`json_pointer`/`boundary_kind`/`value`
exatos (`contracts/contract-boundary-testing.md`). Reverter antes de continuar.

## Cenário 3 — CI dispara automaticamente e sinaliza quebra (User Story 3, P3)

```bash
git checkout -b demo/quebra-deliberada
# introduzir uma quebra trivial, ex.: um `assert_eq!(1, 2)` num teste existente
git commit -am "demo: quebra deliberada para validar CI"
git push -u origin demo/quebra-deliberada
gh pr create --fill
```

**Esperado**: os 5 jobs de `.github/workflows/ci.yml` (`research.md` D6) disparam automaticamente
sem nenhuma ação manual além de abrir a mudança; o job `rust-test` fica vermelho, visível como check
da PR antes de qualquer revisão humana (Acceptance Scenario 2 de US3). Reverter a quebra, `git push`
de novo, confirmar que todos os jobs ficam verdes (Acceptance Scenario 3) — SC-003.

## Cenário 4 — Regressão visual é detectada sem inspeção manual (User Story 4, P4)

```bash
cargo test -p farol-core --test visual_snapshot
```

**Esperado (primeira vez / sem mudança)**: passa, snapshot estável (Acceptance Scenario 2 de US4).

**Regressão deliberada**: alterar um texto visível em `view.rs` para um dos três `screen_id`s
cobertos (`data-model.md` §3 — ex. o título de `view_setup_form`) e rodar de novo:

**Esperado**: o teste falha, `insta` mostra o diff textual exato do que mudou (Acceptance Scenario 3
de US4, SC-005) — sem abrir o Farol. Reverter a mudança, ou (se a mudança fosse intencional) rodar
`cargo insta review` e commitar o `.snap` atualizado.

## Cenário 5 — Confirmação de que nada trava indefinidamente (Edge Case do `spec.md`)

```bash
time cargo test --workspace --test e2e_harness
```

**Esperado**: conclui bem abaixo de 120s por cenário (timeout de `## Clarifications`) mesmo no pior
caso local; nenhum processo Python remanescente depois (`ps aux | grep plugins/`).
