---

description: "Task list template for feature implementation"
---

# Tasks: Infraestrutura de Testes Automatizada

**Input**: Design documents from `/specs/003-automated-testing-infrastructure/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (4 documentos), quickstart.md

**Tests**: Esta feature **é**, por natureza, construção de infraestrutura de teste — praticamente
toda task abaixo é uma task de escrever teste/harness/CI. Não há um código de produto separado a
"testar depois"; a única linha de código de produção tocada (`crates/farol-core/src/main.rs`,
extração de função, T003) é pré-requisito estrutural para os testes `iced_test`, não uma
funcionalidade nova a validar.

**Organization**: Tasks agrupadas por user story priorizada (`spec.md`: US1 P1 → US2 P2 → US3 P3 →
US4 P4). Uma decisão técnica central é pré-requisito bloqueante explícito da Fase Foundational: o
upgrade `iced` 0.13 → 0.14 + adoção de `iced_test` (`research.md` D1), **gated por um spike de
verificação** (T004) — nenhuma task de US1/US4 (que dependem de `iced_test`) começa antes do spike
confirmar que o mecanismo pega o padrão dos dois bugs históricos de `Subscription::map`. A ordem de
fase (US1 → US2 → US3 → US4) segue a prioridade do `spec.md` e também a dependência real: o job
`rust-smoke` do workflow de CI (US3) invoca `tests/integration/harness.sh`, que só existe depois de
US1; o job `rust-test` (US3) só cobre os testes de contrato/visuais depois que US2/US4 os
escreverem — por isso US3 vem depois das duas, mesmo sendo P3 antes de US4 (P4) na prioridade do
spec, o que já é a ordem natural aqui.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2, US3, US4) — ausente em Setup,
  Foundational, Polish e Débito técnico
- Caminho de arquivo exato em cada descrição

## Path Conventions

Estrutura definida em `plan.md` § Project Structure — inteiramente dentro do workspace Rust já
existente (`crates/farol-core`, `crates/farol-protocol`), mais `tests/integration/` (raiz do
workspace) e `.github/workflows/` (novo). Nenhum crate novo, nenhum diretório de plugin novo:

```text
crates/farol-core/Cargo.toml         # iced "0.14"; + dev-dependencies iced_test, insta
crates/farol-core/src/main.rs        # extração de função Program (T003) — único arquivo de produção tocado
crates/farol-core/tests/
├── support/fixtures.rs              # NOVO — fixtures sintéticas (D2): git-local, uptime-kuma
├── e2e_harness.rs                   # NOVO — Camada 1 do harness (D1/D5)
├── visual_snapshot.rs               # NOVO — verificação visual declarativa (D4)
└── snapshots/*.snap                 # NOVO — referências insta (D4)
crates/farol-protocol/tests/
├── contract_schema_validation.rs    # inalterado em forma — load_schemas() reaproveitado
└── schema_boundaries.rs             # NOVO — gerador determinístico de casos de borda (D3)
tests/integration/
├── README.md                        # atualizado — aponta para harness.sh finalmente escrito
└── harness.sh                       # NOVO — Camada 2 do harness (D5)
.github/workflows/ci.yml             # NOVO — workflow único, 5 jobs paralelos (D6)
AGENTS.md                            # atualizado ao final (Polish) — nova seção "Testes" reflete
                                      # a infraestrutura construída por esta feature
```

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Preparar a dependência central desta feature antes de qualquer código de teste que a
use.

- [ ] T001 Em `crates/farol-core/Cargo.toml`: subir `iced` de `{ version = "~0.13", features =
  ["tokio"] }` para `{ version = "0.14", features = ["tokio"] }`; adicionar `iced_test` e `insta`
  como `dev-dependencies` (versões exatas a fixar conforme disponíveis em `crates.io` no momento da
  implementação — `research.md` D1 confirma `iced` `0.14.0` publicado). Rodar `cargo build
  --workspace` e `cargo test --workspace` uma vez, só para confirmar que o bump sozinho não quebra
  compilação (nenhum `impl Widget` próprio existe em `farol-core`, per `research.md` D1 — risco já
  avaliado como baixo, mas confirmado aqui antes de investir no restante da feature)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: A decisão central desta feature (`research.md` D1) só é segura de expandir depois de um
spike que a comprova contra o padrão exato dos bugs históricos — nenhuma task de US1 ou US4 (as duas
que dependem de `iced_test`) começa antes de T004 passar.

**⚠️ CRITICAL**: T004 é um gate, não uma formalidade — se o spike falhar (`iced_test::Emulator` não
conseguir reproduzir o padrão do bug, ou a migração 0.13→0.14 revelar quebra de compilação não
prevista por `research.md`), a decisão de adotar `iced_test` MUST ser revisitada antes de continuar
US1/US4 (voltar a `research.md` D1 § Alternativas consideradas).

- [ ] T002 Em `crates/farol-core/src/main.rs`: extrair a construção do `Program` (hoje só dentro de
  `fn main()`, linha `iced::application("Farol", Farol::update, Farol::view)
  .subscription(Farol::subscription)`) para uma função reutilizável (ex. `pub(crate) fn program() ->
  impl ...`), chamada tanto por `main()` quanto pelos testes `iced_test` (T004 em diante) — **sem
  mudança de comportamento observável** de `cargo run --bin farol` (`research.md` D5, `plan.md` §
  Project Structure)
- [ ] T003 [P] Criar `crates/farol-core/tests/support/fixtures.rs`: helper para fixture `git-local`
  (cria um repositório git real em `TempDir` — `git init` + um commit — via `std::process::Command`
  ou crate `git2`, decisão de implementação) e helper para fixture `uptime-kuma` (servidor HTTP local
  efêmero, `tokio::net::TcpListener`, servindo `/metrics` sintético sob HTTP Basic Auth com API key
  fixa `farol-e2e-fixture-key`, nunca usada contra nenhuma instância real — `research.md` D2,
  `data-model.md` §1). Nenhuma credencial real, nenhuma rede além de `127.0.0.1`
- [ ] T004 **[GATE — spike de migração, `research.md` D1]** Em `crates/farol-core/tests/
  e2e_harness.rs`: escrever exatamente um teste `iced_test`-based que (a) usa `program()` (T002) e um
  estado que reintroduz deliberadamente o padrão do primeiro bug histórico (`Subscription::map`
  capturante em `Farol::subscription()`, `AGENTS.md` § Armadilha, ponto 1) e confirma que o teste
  **falha** com o `debug_assert!` de `iced::Subscription::map`; (b) reverte a reintrodução e confirma
  que o mesmo teste **passa**. Só depois de (a) e (b) confirmados, prosseguir para T005 em diante —
  se (a) não falhar como esperado (o `Emulator` não exercitou o branch), revisitar `research.md` D1
  antes de continuar

---

## Phase 3: User Story 1 - Harness detecta automaticamente que o app real não sobe corretamente (Priority: P1) 🎯 MVP

**Goal**: Confirmar automaticamente, sem observação manual, que o binário do core e os plugins
alcançam os estados esperados (incluindo `Ready`), com falha clara e sem processo remanescente
quando não alcançam.

**Independent Test** (`spec.md`): rodar o harness contra o estado atual do repositório e verificar
sucesso; introduzir uma regressão equivalente a um dos dois bugs históricos e verificar falha clara.

- [ ] T005 [US1] Em `crates/farol-core/tests/e2e_harness.rs`: cenário "git-local alcança Ready" —
  `iced_test::Emulator` dirigindo `program()` (T002) contra a fixture de T003, aguardando
  `PluginState::Ready` do plugin `git-local` dentro de 30s (`## Clarifications` de `spec.md`); nenhum
  processo filho remanescente ao final (confirma FR-001, Acceptance Scenario 1 de US1)
- [ ] T006 [US1] Em `crates/farol-core/tests/e2e_harness.rs`: cenário "uptime-kuma alcança Ready" —
  mesmo mecanismo de T005, contra a fixture HTTP de T003; este cenário, por exigir handshake +
  `required_config` resolvido + primeiro `widget/get` completo antes de `Ready`, é o que documenta
  explicitamente a cobertura do segundo bug histórico (o timer de refresh só é montado quando um
  plugin de fato chega a `Ready` — `AGENTS.md` § Armadilha, ponto 2; Acceptance Scenario 2 de US1)
- [ ] T007 [US1] Em `crates/farol-core/tests/e2e_harness.rs`: envolver cada cenário (T005/T006) em
  `tokio::time::timeout` — 30s por verificação de estado, 120s por cenário inteiro (`##
  Clarifications`); mensagem de falha MUST identificar `plugin_name` e o estado observado (ou
  "timeout excedido") sem exigir leitura de log bruto (FR-003/FR-004, `contracts/
  e2e-harness-contract.md` Camada 1)
- [ ] T008 [US1] Escrever `tests/integration/harness.sh` (Camada 2, smoke de processo real,
  `research.md` D5, `contracts/e2e-harness-contract.md` Camada 2): compila `target/debug/farol`,
  sobe sob `xvfb-run -a`, confirma que sobrevive a uma janela curta de observação, encerra limpo via
  `SIGTERM`, reporta qual das três condições falhou quando falhar
- [ ] T009 [US1] Atualizar `tests/integration/README.md` para apontar para `harness.sh` (T008)
  finalmente escrito, em vez de descrever um script que nunca existiu — fecha o débito citado em
  `AGENTS.md`/`spec.md`

### Validação da User Story 1 (Cenário 1 de `quickstart.md`)

- [ ] T010 [US1] Executar Cenário 1 de `quickstart.md`: caminho feliz (T005–T009 verdes) e a
  regressão deliberada (reintroduzir um dos dois padrões de `Subscription::map` capturante fora do
  spike de T004, ex. no ponto 2 da armadilha — timer de refresh — não coberto literalmente pelo
  spike) — confirmar que o harness falha de forma clara (SC-001); reverter antes de prosseguir

**Checkpoint**: US1 é entregável e testável de forma independente aqui — MVP da feature.

---

## Phase 4: User Story 2 - Casos de borda dos contratos são verificados automaticamente contra o próprio schema normativo (Priority: P2)

**Goal**: Todo valor de borda permitido pelos 4 schemas `v0.2` normativos (mínimos, máximos, nulos,
negativos onde o schema não declara mínimo) é exercitado automaticamente contra a implementação
Rust, sem exemplo manual isolado.

**Independent Test** (`spec.md`): apontar a verificação para um schema existente e confirmar que
exercita valores de borda automaticamente; introduzir uma implementação mais restritiva que o schema
permite e confirmar que a verificação falha apontando o campo/valor exatos.

- [ ] T011 [US2] Criar `crates/farol-protocol/tests/schema_boundaries.rs`: função geradora que, dado
  um schema já carregado (`serde_json::Value`, reaproveitando `load_schemas()` de
  `contract_schema_validation.rs`) e um `json_pointer`, deriva os casos `MinimumMinusOne`/
  `Minimum`/`MaximumPlusOne`/`Maximum`/`NoMinimumNegative`/`Null`/`MissingRequired` per
  `contracts/contract-boundary-testing.md` — a partir do conteúdo real do schema em runtime, nunca de
  uma constante Rust paralela
- [ ] T012 [P] [US2] Em `schema_boundaries.rs`: aplicar o gerador de T011 às propriedades numéricas/
  nuláveis de `handshake.schema.json` (`suggested_refresh_interval_ms`, `RequiredConfigItem.secret`
  como booleano — se aplicável ao vocabulário de `boundary_kind`) — cada caso gerado validado (1)
  contra o `Validator` do schema e (2) contra `serde_json::from_value::<T>` quando (1) é válido
  (FR-006)
- [ ] T013 [P] [US2] Em `schema_boundaries.rs`: aplicar o gerador às propriedades de
  `widget.schema.json`, incluindo obrigatoriamente `MonitorStatusItem.response_time_ms`
  (`NoMinimumNegative`, valor `-1`) — **caso já confirmado nesta sessão de planejamento como
  atualmente falho** (`research.md` D3: o schema permite, `Option<u32>` não representa). Este caso
  específico MUST ficar marcado `#[ignore = "débito rastreado — ver plan.md § Complexity Tracking /
  Débito técnico deste tasks.md"]` até a issue de débito (T029) ser resolvida — mantém o restante do
  gerador rodando (verde) em CI sem deixar uma falha pré-existente e não-relacionada bloqueando toda
  PR futura (SC-004), preservando ao mesmo tempo o registro explícito, executável e não-silencioso do
  gap (rodar manualmente com `cargo test -- --ignored` continua provando o mecanismo, per Cenário 2
  de `quickstart.md`)
- [ ] T014 [P] [US2] Em `schema_boundaries.rs`: aplicar o gerador às propriedades de
  `action.schema.json` (`timeout_hint_ms`)
- [ ] T015 [P] [US2] Em `schema_boundaries.rs`: aplicar o gerador às propriedades de
  `error.schema.json` (`code`, `data.reason` como string livre — confirmar que nenhum campo tem
  `minimum`/`maximum` relevante além do já coberto; se nenhum existir, documentar essa constatação em
  comentário em vez de forçar um caso artificial)

### Validação da User Story 2 (Cenário 2 de `quickstart.md`)

- [ ] T016 [US2] Executar Cenário 2 de `quickstart.md`: confirmar que T013 (`response_time_ms: -1`)
  falha antes de ser marcado `#[ignore]` (prova o mecanismo, SC-002) e que, com `#[ignore]` aplicado,
  `cargo test -p farol-protocol` fica verde; simular uma regressão adicional (apertar outro tipo Rust
  além do schema) e confirmar detecção

**Checkpoint**: US1 + US2 entregáveis juntos aqui — os dois pilares de maior valor comprovado do
`spec.md` (§ Why this priority de US1/US2).

---

## Phase 5: User Story 3 - Verificações rodam automaticamente a cada mudança enviada ao repositório (Priority: P3)

**Goal**: Toda mudança (push/PR) dispara automaticamente a suíte existente, o harness (US1), o
contrato (US2) e o lint — sem ação manual — e sinaliza falha antes de revisão humana.

**Independent Test** (`spec.md`): abrir uma PR e confirmar que as verificações disparam sozinhas;
introduzir uma quebra deliberada e confirmar que a PR é sinalizada como não pronta antes de revisão
manual.

- [ ] T017 [US3] Criar `.github/workflows/ci.yml` com gatilhos `push`/`pull_request` (FR-007) e o job
  `rust-test` (`cargo test --workspace` — cobre os 71 testes existentes + T005–T007 + T011–T015,
  todos sob o mesmo comando, `contracts/ci-workflow-contract.md`)
- [ ] T018 [P] [US3] Adicionar job `rust-lint` a `ci.yml` (`cargo clippy --workspace --all-targets --
  -D warnings`)
- [ ] T019 [US3] Adicionar job `rust-smoke` a `ci.yml` (`apt-get install -y xvfb` + `cargo build --bin
  farol` + `tests/integration/harness.sh`, T008) — depende de T008 existir
- [ ] T020 [P] [US3] Adicionar job `python-lint` a `ci.yml` (`ruff check` em cada `plugins/*/` via
  glob, sem lista hardcoded de diretórios — cobre `plugins/git-local/` mesmo sem `pyproject.toml`
  próprio, usando defaults do `ruff`, `research.md` D6)
- [ ] T021 [P] [US3] Adicionar job `python-test` a `ci.yml` (`pytest` em cada `plugins/*/` que tiver
  teste — hoje só `plugins/uptime-kuma/`)

### Validação da User Story 3 (Cenário 3 de `quickstart.md`)

- [ ] T022 [US3] Executar Cenário 3 de `quickstart.md`: abrir PR com quebra deliberada, confirmar
  check vermelho antes de revisão manual (SC-003, Acceptance Scenario 2 de US3); reverter, confirmar
  todos os 5 jobs verdes (Acceptance Scenario 3); medir tempo total (SC-004, alvo 10 minutos)

**Checkpoint**: US1+US2+US3 entregáveis juntos — infraestrutura de teste completa e automática, só
falta a verificação visual (US4).

---

## Phase 6: User Story 4 - Regressões visuais na interface são detectadas sem depender de inspeção manual (Priority: P4)

**Goal**: Uma mudança visível em uma tela conhecida do Farol é detectada por comparação declarativa,
sem captura de tela nem inspeção manual.

**Independent Test** (`spec.md`): gerar uma captura de uma tela conhecida; introduzir uma mudança
visível; verificar que a comparação aponta a diferença.

- [ ] T023 [US4] Criar `crates/farol-core/tests/visual_snapshot.rs`: helper que extrai uma
  representação textual determinística de um `Element<Message>` via a `Selector` API de `iced_test`
  (todo texto visível, em ordem de composição estável — `contracts/visual-snapshot-contract.md`)
- [ ] T024 [US4] Em `visual_snapshot.rs`: três construtores de estado `Farol` (reaproveitando os
  construtores de fixture de estado já existentes em `update.rs`, tornando-os `pub(crate)` onde
  necessário) para `DashboardReady`, `SetupForm` e `VersionIncompatible` (`data-model.md` §3); um
  `insta::assert_snapshot!` por `screen_id`, commitando os `.snap` de referência em
  `crates/farol-core/tests/snapshots/`

### Validação da User Story 4 (Cenário 4 de `quickstart.md`)

- [ ] T025 [US4] Executar Cenário 4 de `quickstart.md`: confirmar estabilidade sem mudança
  (Acceptance Scenario 2 de US4); alterar deliberadamente um texto visível em `view.rs` para um dos
  três `screen_id`s e confirmar que `insta` aponta o diff exato (Acceptance Scenario 3, SC-005);
  reverter antes de prosseguir

**Checkpoint**: Todas as quatro user stories entregáveis — feature completa.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Consolidação — nenhuma task desta fase é específica de uma user story.

- [ ] T026 [P] Atualizar `AGENTS.md` § Testes: registrar a suíte agora coberta por CI automático
  (T017–T021), o harness em duas camadas (T005–T009, `tests/integration/harness.sh` deixa de ser um
  stub), o gerador de casos de borda (T011–T015) e a verificação visual declarativa (T023–T024) —
  substitui as linhas hoje desatualizadas sobre `harness.sh` nunca construído e sobre validação
  manual via `eprintln!`
- [ ] T027 [P] Confirmar `cargo clippy --workspace --all-targets` limpo (sem warning) depois de todo
  o código de teste novo desta feature (T001–T025) — mesmo padrão já exigido pelo `AGENTS.md`
- [ ] T028 Executar Cenário 5 de `quickstart.md` (`time cargo test --workspace --test e2e_harness`) —
  confirmar que nada trava indefinidamente e que nenhum processo Python remanesce (Edge Case do
  `spec.md`)

---

## Débito técnico (issues a criar)

Diferente do padrão já usado pela feature 002 (onde as issues de débito já existiam antes do
`tasks.md`), os dois itens abaixo são achados **desta própria sessão de planejamento**
(`research.md` D3/D6, `plan.md` § Complexity Tracking) — nenhuma issue existe ainda. Cada task
abaixo **MUST** primeiro criar a issue correspondente (`gh issue create`), só então referenciá-la;
nenhuma delas é executada nesta sessão (`/speckit-tasks` não cria issues). T029/T030 são
independentes entre si e do resto da feature — podem rodar a qualquer momento, inclusive antes da
Fase 1 —, mas **MUST** estar concluídas (issue criada, ainda que não corrigida) antes de esta
feature (003) ser considerada encerrada, per a regra "Dívida técnica rastreável" da constitution
v1.0.0.

- [ ] T029 **[Débito]** Criar issue no tracker do projeto: `MonitorStatusItem.response_time_ms`
  (`protocol/schema/v0.2/widget.schema.json`) permite qualquer inteiro (sem `minimum`, logo inclui
  negativo) mas `Option<u32>` (`crates/farol-protocol/src/messages.rs`) não representa valor
  negativo algum — gap revelado por T013 (`#[ignore]`d até esta issue ser resolvida). A issue MUST
  cobrir: (a) decidir se o tipo Rust é alargado (`i32`/`i64`) ou se o schema ganha `minimum: 0`; (b)
  referenciar `research.md` D3 e o teste `#[ignore]`d de T013 como o que primeiro tornou o gap
  visível de forma automatizada
- [ ] T030 **[Débito]** Criar issue no tracker do projeto: `plugins/git-local/` não tem
  `pyproject.toml`/configuração `ruff` própria, diferente de `plugins/uptime-kuma/` — inconsistência
  entre os dois plugins de referência, descoberta em `research.md` D6. A issue MUST cobrir a criação
  de `plugins/git-local/pyproject.toml` no mesmo padrão de `plugins/uptime-kuma/pyproject.toml`

---

## Dependencies & Execution Order

### Dependências entre fases

- **Setup (T001)**: sem dependências — pode começar imediatamente.
- **Foundational (T002–T004)**: depende de T001 (dependências já no `Cargo.toml`). T002 e T003 são
  paralelizáveis entre si (arquivos diferentes: `main.rs` vs. `tests/support/fixtures.rs`); T004
  depende de T002 (usa `program()`) mas não de T003 (o spike usa um estado construído manualmente,
  não a fixture de git/HTTP). **T004 bloqueia toda a Fase 3 (US1) e a Fase 6 (US4)** — as duas únicas
  fases que usam `iced_test` de fato.
- **User Story 1 (Fase 3)**: depende de T004 (gate) e T003 (fixtures). T005/T006 podem rodar em
  paralelo entre si (cenários independentes dentro do mesmo arquivo, mas sem dependência de dado
  compartilhado); T007 depende de T005/T006 existirem (envolve os cenários já escritos); T008/T009
  independentes de T005–T007 (Camada 2 é um script separado) — podem rodar em paralelo com T005–T007.
- **User Story 2 (Fase 4)**: independente de US1 — depende só de Foundational (T001) e do crate
  `farol-protocol`, que esta feature não modifica em código de produção. T011 bloqueia T012–T015
  (todos usam a função geradora); T012–T015 são paralelizáveis entre si (schemas/arquivos distintos
  dentro do mesmo arquivo de teste — paralelizável como trabalho, não necessariamente como commit
  simultâneo do mesmo arquivo).
- **User Story 3 (Fase 5)**: depende de US1 (T008, para o job `rust-smoke`) e beneficia-se de US2 já
  existir (para que `rust-test` cubra os novos testes de contrato) — não bloqueia estritamente em
  US2 (o job roda `cargo test --workspace` de qualquer forma, cobrindo o que existir no momento),
  mas a ordem US1→US2→US3 evita um job `rust-smoke` referenciando um script inexistente.
- **User Story 4 (Fase 6)**: depende de T004 (gate) e T002 (`program()`) — não depende de US1/US2/US3.
  Poderia, em tese, rodar em paralelo com as Fases 4/5 (nenhum arquivo compartilhado) — ordenada por
  último aqui só por seguir a prioridade P4 do `spec.md`.
- **Polish (Fase 7)**: depende de todas as user stories desejadas estarem completas.
- **Débito técnico (T029/T030)**: independentes de toda a feature — podem rodar a qualquer momento;
  T029 referencia T013, então fica mais natural depois de T013 existir (mesmo não sendo uma
  dependência técnica de arquivo).

### Dentro de cada user story

- US1: T005/T006 (cenários) → T007 (timeout/mensagem de falha envolvendo os dois) → validação T010;
  T008/T009 (Camada 2) em paralelo com a cadeia acima.
- US2: T011 (gerador) → T012–T015 (aplicação por schema, paralelizáveis) → validação T016.
- US3: T017 (workflow base + `rust-test`) → T018–T021 (demais jobs, paralelizáveis entre si, T019
  depende de T008) → validação T022.
- US4: T023 (helper de extração) → T024 (construtores de estado + snapshots) → validação T025.

### Oportunidades de paralelismo

- T002/T003 (Foundational) em paralelo.
- Dentro de US1: T005/T006 em paralelo; T008/T009 em paralelo com T005–T007.
- Dentro de US2: T012–T015 em paralelo, todos depois de T011.
- Dentro de US3: T018/T020/T021 em paralelo entre si (T019 depende de T008, já satisfeito antes da
  Fase 5 começar).
- T029/T030 (Débito técnico) em paralelo entre si e com qualquer outra fase.
- T026/T027 (Polish) em paralelo entre si; T028 depende de todo o resto (mede o conjunto completo).

---

## Parallel Example: Foundational (após T001)

```text
T002 - Extrair program() em crates/farol-core/src/main.rs
T003 - Criar crates/farol-core/tests/support/fixtures.rs
(T004 só começa depois de T002 estar pronto)
```

## Parallel Example: User Story 1 (após T004)

```text
T005 - Cenário git-local em crates/farol-core/tests/e2e_harness.rs
T006 - Cenário uptime-kuma em crates/farol-core/tests/e2e_harness.rs
T008 - tests/integration/harness.sh
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Fase 1 (T001) + Fase 2 (T002–T004, incluindo o gate).
2. Completar Fase 3 (T005–T010).
3. **PARAR e validar**: rodar Cenário 1 de `quickstart.md` de forma independente — harness de
   execução real funcionando, MVP da feature entregue (SC-001 alcançável já aqui).

### Entrega Incremental

1. Setup + Foundational → Fundação pronta, nenhuma user story ainda testável.
2. US1 (Fase 3) → MVP: harness de execução real, o pilar de maior valor comprovado.
3. US2 (Fase 4) → Cobertura de contrato mais rigorosa, entregável junto ou depois de US1.
4. US3 (Fase 5) → CI automatizado, multiplica o valor de US1+US2 (só faz sentido depois delas
   existirem — `spec.md` § Why this priority de US3).
5. US4 (Fase 6) → Verificação visual declarativa, o pilar de natureza mais exploratória (`spec.md`
   § Why this priority de US4) — pode ser adiado sem comprometer os outros três.
6. Polish (Fase 7) + Débito técnico (T029/T030) → fecham a feature.

### Estratégia de Equipe Paralela

Depois de Foundational (T004) completo: uma frente em US1 (Fase 3), outra em US2 (Fase 4) —
totalmente independentes entre si, sem arquivo compartilhado. US3 (Fase 5) só começa depois que US1
produzir `harness.sh` (T008). US4 (Fase 6) pode começar em paralelo com US1/US2/US3 assim que T004
passar (só depende de Foundational).

## Notes

- `[P]` tasks = arquivos diferentes, sem dependência entre si.
- Cada user story é independentemente completável e testável, per `spec.md` § Independent Test de
  cada uma.
- Nenhuma task desta feature edita `crates/farol-core/src/{update,view,model,plugin_worker}.rs`,
  `crates/farol-protocol/src/*.rs`, nem qualquer arquivo de `plugins/*/` — o "raio de mudança" em
  código de produção é deliberadamente T002 apenas (`plan.md` § Structure Decision).
- Commitar depois de cada task ou grupo pequeno de tasks relacionadas, seguindo a convenção já usada
  no histórico do repositório.
