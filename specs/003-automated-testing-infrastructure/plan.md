# Implementation Plan: Infraestrutura de Testes Automatizada

**Branch**: `003-automated-testing-infrastructure` | **Date**: 2026-09-01 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/003-automated-testing-infrastructure/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Infraestrutura de testes automatizada para o Farol, endereçando os três bugs reais que só
apareceram em execução real durante a feature 002 (dois panics de `iced::Subscription::map` com
closure capturante; um erro de decode `#[serde(untagged)]` mascarado como timeout) e fechando o
débito de `tests/integration/harness.sh`, referenciado desde a feature 001 mas nunca construído.
Quatro pilares, um por User Story do `spec.md`: (US1) harness de execução real em **duas camadas** —
uma camada in-process usando o crate `iced_test`, novo em `iced` 0.14 (`Emulator`, headless,
executa `Task`/`Subscription` de verdade — decisão central desta feature, `research.md` D1), mais
uma camada secundária de smoke de processo OS real que finalmente fecha `harness.sh`; (US2)
cobertura de contrato mais rigorosa via um gerador determinístico de valores de borda derivados dos
4 schemas JSON normativos `v0.2` (D3) — que já revela, nesta própria sessão de planejamento, um gap
real e presente hoje (`response_time_ms: -1`, permitido pelo schema, irrepresentável em
`Option<u32>`); (US3) um workflow GitHub Actions com 5 jobs paralelos (D6); (US4) verificação visual
declarativa via a `Selector` API de `iced_test` + snapshot textual `insta` (D4), com captura de
pixels documentada como extensão futura não-bloqueante.

A decisão técnica central — adotar `iced` 0.14 (bump de `~0.13`) + `iced_test` — foi pesquisada e
verificada nesta sessão (não aceita cegamente): confirmado que o crate existe, é headless por
design, e que o `Emulator` roda `Subscription`/`Task` reais (decisivo para pegar organicamente a
classe dos dois panics históricos); confirmado também que `farol-core` não tem nenhum `impl Widget`
próprio (reduz o risco das mudanças *breaking* confirmadas no changelog de 0.14, que afetam
`Widget::update`). A cobertura de US1/US4 pelo achado é **parcial, não integral** — avaliação própria
registrada em `research.md` D1: o `Emulator` roda o `Program` real *dentro do processo de teste*, não
como o binário `farol` compilado rodando como processo OS separado com o backend de janela real; daí
a arquitetura de duas camadas em vez de uma substituição integral.

## Technical Context

**Language/Version**: Rust (edition 2021, MSRV ≥ 1.75) para `farol-core`/`farol-protocol`
(inalterado); Python 3.11+ (stdlib) para os plugins de referência já existentes, sem mudança de
linguagem por esta feature. Nenhuma linguagem nova introduzida.

**Primary Dependencies**: `iced` sobe de `~0.13` para `0.14` (`research.md` D1) — única mudança de
versão de dependência de produção; `iced_test` (novo, `dev-dependency` de `farol-core`, D1/D4/D5);
`insta` (novo, `dev-dependency` de `farol-core`, D4). Nenhuma dependência nova de produção além do
bump de `iced` em si. `farol-protocol` ganha o gerador de casos de borda (D3) como código de teste
próprio — nenhuma dependência nova (`proptest` avaliado e rejeitado, D3).

**Storage**: N/A do lado do produto (esta feature não toca `config_store.rs`/`secrets_store.rs`).
Novo: arquivos versionados de snapshot (`crates/farol-core/tests/snapshots/*.snap`, D4) e um novo
workflow declarativo (`.github/workflows/ci.yml`, D6) — ambos configuração/fixture de teste, não
armazenamento de produção.

**Testing**: `cargo test --workspace` continua o comando único que roda tudo do lado Rust (71 testes
existentes + os novos de D1 Camada 1, D3, D4) — nenhum harness externo novo para esses três pilares,
conforme a característica "roda dentro do `cargo test` normal" que motivou a escolha de `iced_test`.
Só a Camada 2 do harness (D5, smoke de processo real) roda fora de `cargo test`, via
`tests/integration/harness.sh` (finalmente escrito), sob `xvfb-run`. `ruff check`/`pytest` do lado
Python, inalterados, agora disparados automaticamente por CI (D6) em vez de manualmente.

**Target Platform**: Linux desktop nativo (Princípio I), inalterado. CI roda em `ubuntu-latest`
(GitHub Actions, `## Assumptions` de `spec.md` — mesma plataforma que já hospeda o repositório).

**Project Type**: Aplicação desktop nativa (GUI) com processos filhos de plugin — mesmo tipo de
projeto das features 001/002; nenhuma estrutura nova de projeto (workspace Rust + `protocol/` +
`plugins/*`), só arquivos de teste/CI novos dentro da estrutura existente.

**Performance Goals**: Timeout por verificação do harness = 30s; timeout por cenário do harness =
120s (`## Clarifications` de `spec.md`). Orçamento do conjunto completo de verificações de CI = até
10 minutos (SC-004), distribuído entre 5 jobs paralelos (D6) para não somar tempos.

**Constraints**: Nenhuma fixture do harness usa credencial ou serviço externo real
(`## Clarifications`/`## Assumptions` de `spec.md`, D2). O gerador de borda (D3) roda só contra
`protocol/schema/v0.2/` — versão corrente, não `v0.1` histórico (Edge Case do `spec.md`). Verificação
visual é declarativa (texto), não pixels, nesta versão (D4, FR-009).

**Scale/Scope**: 2 plugins de referência conhecidos hoje (`git-local`, `uptime-kuma`) cobertos pelo
harness — extensível a um terceiro sem reconstrução (FR-013, `contracts/ci-workflow-contract.md`);
3 `screen_id`s cobertos pela verificação visual inicialmente (D4); 4 schemas normativos `v0.2`
cobertos pelo gerador de borda (D3).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Avaliação contra os 7 Core Principles da constitution v1.0.0. Esta feature é infraestrutura de
teste, não uma funcionalidade de produto — a maioria dos princípios de produto é **N/A** por
natureza (nenhum widget novo, nenhuma capacidade nova, nenhum workspace/paleta/registry tocado). A
seção **Governance** (dívida técnica rastreável) é a que mais materialmente se aplica aqui, por dois
achados concretos desta sessão (ver Complexity Tracking).

| # | Princípio | Status | Como esta feature cumpre |
|---|---|---|---|
| I | Nativo e Sem Navegador | **PASS** | Nenhuma mudança — `iced` continua o backend nativo; `iced_test`/`Emulator` são headless (sem motor de navegador embutido, sem substituição do backend real de janela, que a Camada 2 do harness continua exercitando). |
| II | Plugins como Processos Isolados via JSON-RPC | **PASS, reforçado** | O harness (D2/D5) spawna os plugins de referência exatamente como o core de produção já faz (`tokio::process::Command`, `plugin_worker.rs` inalterado) — nenhum atalho que burle o isolamento por processo; a Camada 1 in-process exercita comunicação real entre processos (core in-process, plugin como processo filho real). |
| III | Widgets Declarativos, Core Renderiza | **PASS** | A verificação visual (D4) lê `view()` via `Selector`, nunca reimplementa nem contorna a renderização declarativa do core — confirma o princípio em vez de tensioná-lo. |
| IV | Permissões Explícitas por Manifesto | **PASS** | Fixtures do harness (D2) declaram `required_config` pelo mesmo mecanismo de produção (variável de ambiente injetada, `secrets_store`/`config_store` inalterados) — nenhum atalho de segredo, nenhuma credencial real (`## Clarifications` de `spec.md`). |
| V | Espaços (Workspaces) por Contexto | **N/A** | Fora do escopo desta feature — nenhum estado de workspace exercitado ou tocado. |
| VI | Paleta de Comandos Universal | **N/A** | Idem — nenhuma ação nova, nenhuma paleta tocada. |
| VII | Registry Federado sem Infra Própria | **N/A** | Idem — `known_plugins()` continua um registro fixo, inalterado por esta feature (D2 usa os 2 plugins já conhecidos). |
| Governance | Dívida técnica rastreável | **ATENÇÃO, não bloqueante do design — ver Complexity Tracking** | Esta sessão de planejamento revelou 2 débitos concretos (não introduzidos por esta feature, pré-existentes): (a) `MonitorStatusItem.response_time_ms` schema `v0.2` permite negativo, `Option<u32>` não representa (`research.md` D3); (b) `plugins/git-local/` não tem `pyproject.toml`/config `ruff` próprio, diferente de `plugins/uptime-kuma/` (`research.md` D6). Nenhum dos dois é corrigido por esta feature (`## Out of Scope` de `spec.md`: "corrigir defeitos de produto... é item separado") — registrados como obrigação de Governance a satisfazer antes do fechamento da feature, mesmo padrão já usado pela feature 002 para a migração de `git-local`. |

**Gate**: PASS. Nenhuma violação de princípio de produto a justificar — a única linha de atenção é
uma obrigação de Governance (dívida técnica rastreável), não uma violação de Core Principle, tratada
em § Complexity Tracking abaixo no mesmo formato já usado por `specs/002-uptime-kuma-plugin/plan.md`.

*Re-checagem pós-Fase 1*: nenhuma decisão de `data-model.md`/`contracts/` introduziu violação nova
além do já identificado em Fase 0 — reportado uma única vez, mesmo padrão de `specs/002-*`.

## Project Structure

### Documentation (this feature)

```text
specs/003-automated-testing-infrastructure/
├── plan.md                          # This file (/speckit-plan command output)
├── research.md                      # Phase 0 output — decisões D1-D6
├── data-model.md                    # Phase 1 output — entidades de teste/infra
├── quickstart.md                    # Phase 1 output — 5 cenários de validação manual
├── contracts/                        # Phase 1 output
│   ├── e2e-harness-contract.md
│   ├── contract-boundary-testing.md
│   ├── visual-snapshot-contract.md
│   └── ci-workflow-contract.md
├── checklists/
│   └── requirements.md              # já existia (sessão de /speckit-specify), atualizado por /speckit-clarify
└── tasks.md                          # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

Estrutura alvo para as fases de implementação (`/speckit-tasks` + `/speckit-implement` — **nenhum
arquivo abaixo é criado por este plano**; documentado aqui só para orientar as tasks futuras,
conforme `research.md` D1-D6):

```text
Cargo.toml                          # workspace root — inalterado em membros; iced sobe para "0.14"
                                     # em crates/farol-core/Cargo.toml (único bump de versão de
                                     # dependência de produção desta feature, D1)
crates/
├── farol-core/
│   ├── Cargo.toml                   # iced = "0.14" (era "~0.13"); + dev-dependencies iced_test,
│   │                                 # insta (D1/D4/D5)
│   ├── src/
│   │   ├── main.rs                  # extrai a construção do Program (hoje só dentro de main())
│   │   │                            # para uma função reutilizável, chamada tanto por main() quanto
│   │   │                            # pelos testes de e2e_harness.rs (D5) — único ponto de código
│   │   │                            # de produção tocado por esta feature; sem mudança de
│   │   │                            # comportamento, só de forma (extração de função)
│   │   └── (update.rs/view.rs/model.rs/plugin_worker.rs inalterados — nenhuma mudança de
│   │        comportamento de produção fora da extração de main.rs acima)
│   └── tests/
│       ├── e2e_harness.rs           # NOVO — Camada 1 do harness (D1/D5), iced_test::Emulator
│       │                            # dirigindo o Program real contra fixtures sintéticas (D2)
│       ├── visual_snapshot.rs       # NOVO — verificação visual declarativa (D4), iced_test::
│       │                            # Selector + insta
│       └── snapshots/               # NOVO — referências versionadas do insta (D4)
│           └── *.snap
└── farol-protocol/
    └── tests/
        ├── contract_schema_validation.rs   # inalterado em forma — continua carregando os 4
        │                                    # schemas v0.2 (load_schemas() reaproveitado por D3)
        └── schema_boundaries.rs      # NOVO — gerador determinístico de casos de borda (D3),
                                       # consome load_schemas() de contract_schema_validation.rs

tests/
└── integration/
    ├── README.md                    # atualizado — aponta para harness.sh finalmente escrito, em
    │                                 # vez de descrever um script que nunca existiu
    └── harness.sh                   # NOVO — Camada 2 do harness (D5), smoke de processo real sob
                                       # xvfb-run

plugins/
└── git-local/
    └── pyproject.toml               # NÃO criado por esta feature — débito de Governance registrado
                                       # em § Complexity Tracking, não corrigido aqui (Out of Scope)

.github/
└── workflows/
    └── ci.yml                       # NOVO — workflow único, 5 jobs paralelos (D6,
                                       # contracts/ci-workflow-contract.md); repositório não tem
                                       # nenhum workflow hoje
```

**Structure Decision**: Nenhuma estrutura de projeto nova — esta feature vive inteiramente dentro do
workspace Rust já existente (`crates/farol-core`, `crates/farol-protocol`) como código de teste
(`tests/*.rs`, `dev-dependencies`), mais um script de shell (`tests/integration/harness.sh`,
substituindo o stub já referenciado) e um workflow declarativo novo (`.github/workflows/ci.yml`, o
único diretório genuinamente novo no repositório). O único arquivo de código de **produção** tocado é
`crates/farol-core/src/main.rs`, e só para extrair a construção do `Program` em uma função
reutilizável — sem mudança de comportamento observável do Farol em execução normal (`cargo run`).
Essa é uma decisão deliberada: manter o "raio de mudança" desta feature o mais próximo possível de
zero no código de produto, coerente com o próprio propósito da feature (construir infraestrutura de
teste, não alterar o que está sendo testado) e com `## Out of Scope` do `spec.md`.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

Nenhuma violação de Core Principle a justificar (gate PASS). Os dois itens abaixo **não são
violações de princípio** — são obrigações de **Governance** (regra "Dívida técnica rastreável" da
constitution v1.0.0) reveladas por esta sessão de planejamento, que este plano deixa registradas
para não se perderem entre o planejamento e o fechamento da feature, no mesmo formato já usado por
`specs/002-uptime-kuma-plugin/plan.md` para a migração de `git-local`:

| Débito técnico identificado | Por que não é corrigido nesta feature | Ação obrigatória antes de encerrar a feature |
|---|---|---|
| `protocol/schema/v0.2/widget.schema.json`, `MonitorStatusItem.response_time_ms`, permite qualquer inteiro (sem `minimum`, logo inclui negativo) por construção de JSON Schema; o tipo Rust correspondente é `Option<u32>`, que não consegue representar nenhum valor negativo — uma divergência silenciosa de contrato já presente hoje, não introduzida por esta feature (`research.md` D3). | `## Out of Scope` do `spec.md` desta feature é explícito: "corrigir defeitos de produto adicionais que as verificações desta feature venham a revelar... cada defeito revelado é tratado como item separado". Esta feature constrói o *mecanismo* que revela o gap (US2); corrigir o gap em si é trabalho de produto, fora do escopo de uma feature de infraestrutura de teste. | Pela regra "Dívida técnica rastreável": **MUST** virar uma issue no tracker do projeto (GitHub Issues) antes de esta feature (003) ser considerada encerrada — a criação da issue em si é trabalho de fechamento de feature, fora do escopo desta sessão de planejamento (mesma distinção já aplicada pela feature 002 à migração de `git-local`). A issue cobre, no mínimo: (a) decidir se o tipo Rust é alargado (`i32`/`i64`) ou se o schema ganha `minimum: 0` (alinhando o contrato pelo lado do schema em vez do lado da implementação — decisão de produto, não desta sessão); (b) referenciar `research.md` D3 desta feature e o teste de `schema_boundaries.rs` que primeiro tornou o gap visível de forma automatizada. |
| `plugins/git-local/` não tem `pyproject.toml`/configuração `ruff` própria — diferente de `plugins/uptime-kuma/`, que tem. `python-lint` (`research.md` D6) roda mesmo assim (usando defaults do `ruff` para `git-local`), então não bloqueia esta feature, mas é uma inconsistência de configuração entre os dois plugins de referência, descoberta nesta sessão. | Corrigir a configuração de lint de um plugin já implementado (feature 001) é fora do escopo de uma feature de infraestrutura de teste que consome, mas não edita, `plugins/git-local/` (`## Out of Scope` de `spec.md`, por analogia ao mesmo raciocínio do item acima). | Pela mesma regra: **MUST** virar uma issue (pode ser a mesma issue do item acima, ou uma separada — decisão de quem fechar a feature) antes de 003 ser considerada encerrada, cobrindo a criação de `plugins/git-local/pyproject.toml` no mesmo padrão de `plugins/uptime-kuma/pyproject.toml` (`research.md` D6). |

