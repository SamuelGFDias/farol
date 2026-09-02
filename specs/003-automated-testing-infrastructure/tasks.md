---

description: "Task list template for feature implementation"
---

# Tasks: Infraestrutura de Testes Automatizada

**Input**: Design documents from `/specs/003-automated-testing-infrastructure/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (4 documentos), quickstart.md

**Tests**: Esta feature **é**, por natureza, construção de infraestrutura de teste — praticamente
toda task abaixo é uma task de escrever teste/harness/CI. Não há um código de produto separado a
"testar depois". O código de produção tocado é pré-requisito estrutural, não funcionalidade nova a
validar: a extração de `program(boot)` em `crates/farol-core/src/main.rs` (T002) e — **revisado em
2026-09-01, achado N2** — a migração das APIs de `iced` 0.14 em `main.rs`/`update.rs`/
`plugin_worker.rs` (T001b), sem a qual o crate simplesmente não compila.

**Organization**: Tasks agrupadas por user story priorizada (`spec.md`: US1 P1 → US2 P2 → US3 P3 →
US4 P4). Uma decisão técnica central é pré-requisito bloqueante explícito da Fase Foundational: o
upgrade `iced` 0.13 → 0.14 + adoção de `iced_test` (`research.md` D1), **gated por um spike de
verificação** (T004) — nenhuma task de US1/US4 (que dependem de `iced_test`) começa antes do spike
confirmar que o `Emulator` roda o mecanismo real (`Subscription` → spawn → handshake → transição de
estado) de ponta a ponta. *O critério original do gate — reproduzir em runtime o padrão dos dois bugs
históricos de `Subscription::map` — foi refutado por N1 e substituído; ver a entrada de 2026-09-01 em
`research.md` D1.* A ordem de
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

> **Corrigido na execução de 2026-09-01** (achados N2/N3 da "Nota de execução" e a entrada revisada
> de D1 em `research.md`): `crates/farol-core` é um crate **só-bin** (sem `src/lib.rs`/target `lib`),
> então testes de integração em `crates/farol-core/tests/` não conseguem importar nada dele — a
> estrutura originalmente planejada ali é estruturalmente impossível. Os testes `iced_test` seguem o
> padrão que o crate já usava (`#[cfg(test)] mod` dentro do bin target). A migração `iced` 0.14
> também obrigou a tocar `update.rs`/`plugin_worker.rs` (não só `main.rs`) — ver § Notes.

```text
crates/farol-core/Cargo.toml         # iced "0.14"; + dev-dependencies iced_test, insta
crates/farol-core/src/main.rs        # program(boot) + Farol::with_plugins (T002); iced::application 0.14
crates/farol-core/src/plugin_worker.rs  # Subscription::run_with (N2); PluginSpawnConfig: +Hash
crates/farol-core/src/update.rs      # Subscription::run_with no timer de refresh (N2)
crates/farol-core/src/e2e_tests.rs   # NOVO — fixtures (T003) + Camada 1 do harness (D1/D5), como
                                     # `#[cfg(test)] mod e2e_tests;` declarado em main.rs
crates/farol-core/src/visual_snapshot_tests.rs  # NOVO (US4) — mesma forma: módulo `#[cfg(test)]`
crates/farol-core/src/snapshots/*.snap          # NOVO — referências insta (D4)
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

- [X] T001 Em `crates/farol-core/Cargo.toml`: subir `iced` de `{ version = "~0.13", features =
  ["tokio"] }` para `{ version = "0.14", features = ["tokio"] }`; adicionar `iced_test` e `insta`
  como `dev-dependencies` (versões exatas a fixar conforme disponíveis em `crates.io` no momento da
  implementação — `research.md` D1 confirma `iced` `0.14.0` publicado). Rodar `cargo build
  --workspace` e `cargo test --workspace` uma vez, só para confirmar que o bump sozinho não quebra
  compilação (nenhum `impl Widget` próprio existe em `farol-core`, per `research.md` D1 — risco já
  avaliado como baixo, mas confirmado aqui antes de investir no restante da feature).
  **Resultado real (2026-09-01)**: `iced = "0.14"` (0.14.0), `iced_test = "0.14"` (0.14.0), `insta =
  "1"`. O bump **quebrou** a compilação em 5 pontos — a avaliação de risco de `research.md` D1 estava
  errada; ver achado N2 da "Nota de execução" e T001b abaixo
- [X] T001b **[corretiva, decorrente de N2]** Migrar o código de produção para as APIs de `iced`
  0.14: `Subscription::run_with_id` → `Subscription::run_with` em
  `crates/farol-core/src/plugin_worker.rs` (novo `WorkerSubscriptionKey`, `PluginSpawnConfig` ganha
  `PartialEq, Eq, Hash`) e em `crates/farol-core/src/update.rs` (novo `RefreshSubscriptionKey`);
  anotação de tipo do `Sender` em `iced::stream::channel` nos dois arquivos; `iced::application`
  no formato `(boot, update, view)` + `.title(...)` em `crates/farol-core/src/main.rs`. **MUST**
  preservar o mecanismo de reconexão de D8 da feature 002 (a identidade da `Subscription` do worker
  muda quando `setup_attempt` muda) — agora garantido por `#[derive(Hash)]`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: A decisão central desta feature (`research.md` D1) só é segura de expandir depois de um
spike que a comprova contra o padrão exato dos bugs históricos — nenhuma task de US1 ou US4 (as duas
que dependem de `iced_test`) começa antes de T004 passar.

**⚠️ CRITICAL**: T004 é um gate, não uma formalidade — se o spike falhar (o `Emulator` não conseguir
rodar o mecanismo real, ou a migração 0.13→0.14 revelar quebra não contornável), a decisão de adotar
`iced_test` MUST ser revisitada antes de continuar US1/US4 (voltar a `research.md` D1 § Alternativas
consideradas).

- [X] T002 Em `crates/farol-core/src/main.rs`: extrair a construção do `Program` (hoje só dentro de
  `fn main()`) para uma função reutilizável, chamada tanto por `main()` quanto pelos testes
  `iced_test` (T004 em diante) — **sem mudança de comportamento observável** de `cargo run --bin
  farol` (`research.md` D5, `plan.md` § Project Structure).
  **Forma entregue**: `pub(crate) fn program(boot: impl Fn() -> Farol + 'static) -> iced::Application<impl
  iced::Program<...>>`, mais `Farol::with_plugins(Vec<PluginSpawnConfig>)` (de onde `Farol::default`
  passou a derivar). O `boot` é parâmetro porque `Farol::default()` deriva os plugins de
  `known_plugins()`, que usa caminhos **relativos** ao `cwd` da raiz do repo e traz os dois plugins
  conhecidos — sob `cargo test` isso seria não-determinístico duas vezes (ver a entrada revisada de
  D1 em `research.md`). `main()` passa `Farol::default`
- [X] T003 [P] Fixture determinística dos plugins de referência, em
  `crates/farol-core/src/e2e_tests.rs` (**não** `tests/support/fixtures.rs` — ver N3 e § Path
  Conventions): `HarnessFixture` cria um repositório git real (`git init` + um commit via
  `std::process::Command`, identidade/datas fixas, `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`
  neutralizados) e escreve `config.toml`/`secrets.toml` sintéticos dos dois plugins sob um diretório
  temporário que vira o `$XDG_CONFIG_HOME` do processo de teste — hermético para o core **e** para o
  processo Python do plugin, sem tocar o `~/.config/farol` real. API key sintética
  `farol-e2e-fixture-key`; `base_url` em `http://127.0.0.1:1`. Nenhuma credencial real, nenhuma rede
  além de `127.0.0.1`.
  **Pendente**: o duplo HTTP determinístico de `/metrics` (`tokio::net::TcpListener` + Basic Auth,
  `research.md` D2) — não é necessário para o gate (`Ready` depende só do handshake +
  `required_config`), é pré-requisito de T006 (assertivas sobre os *dados* do widget)
- [X] T004 **[GATE — critério revisado, ver entrada de 2026-09-01 em `research.md` D1]** Em
  `crates/farol-core/src/e2e_tests.rs`: teste(s) `iced_test` que provam que o `Emulator` roda
  `Farol::subscription()` **de verdade** e leva uma conexão de plugin **real** a um estado terminal
  — `Subscription` real → spawn de processo filho real (`tokio::process`) → handshake JSON-RPC/NDJSON
  real → transição real em `update.rs`.
  **Mudança de critério**: o desenho original (reintroduzir o 1º bug histórico e confirmar que o
  `Emulator` o pega em runtime) é **inexecutável** — em `iced` 0.14 aquele `debug_assert!` virou
  `const { check_zero_sized::<F>() }`, ou seja, erro de compilação (achado N1). Não há runtime a
  observar; a classe de bug deixou de poder existir num binário compilado. O gate passa a provar o
  *mecanismo*, que é o que sustenta US1 daqui em diante.
  **Entregue, dois cenários, ambos verdes**: (a)
  `emulator_runs_the_real_subscription_until_a_plugin_reaches_ready` — `uptime-kuma` alcança
  `PluginState::Ready` pelo `Emulator`, com `required_config` resolvido pela fixture pelo mecanismo
  de produção; (b) `emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state` —
  `git-local` contra a fixture de T003, terminando em `Unavailable{VersionIncompatible}`. Ambos
  verificados como não-vacuosos (sabotar a fixture muda o estado observado)

---

## Phase 3: User Story 1 - Harness detecta automaticamente que o app real não sobe corretamente (Priority: P1) 🎯 MVP

**Goal**: Confirmar automaticamente, sem observação manual, que o binário do core e os plugins
alcançam os estados esperados (incluindo `Ready`), com falha clara e sem processo remanescente
quando não alcançam.

**Independent Test** (`spec.md`): rodar o harness contra o estado atual do repositório e verificar
sucesso; introduzir uma regressão equivalente a um dos dois bugs históricos e verificar falha clara.

- [ ] T005 [US1] **🚧 BLOQUEADA por N4 (achado de 2026-09-01, ver `research.md` D1 revisado)** —
  cenário "git-local alcança Ready" é **impossível** enquanto `plugins/git-local/main.py` declarar
  `PROTOCOL_VERSION = "0.1"` contra um core que fala `"0.2"`: a comparação `MAJOR == 0` de
  `ProtocolVersion::is_compatible_with` torna `Unavailable{VersionIncompatible}` o único desfecho
  possível. Migrar `git-local` para v0.2 é o débito técnico #4, deliberadamente Fora de Escopo desta
  feature. **Decisão pendente do arquiteto** — três saídas: (a) puxar o débito #4 para dentro desta
  feature como pré-requisito de US1; (b) manter `uptime-kuma` como o único plugin que exercita
  `Ready` (T006) e reescrever T005 como "git-local alcança um estado terminal", que é o que o
  cenário (b) de T004 já entrega — nesse caso T005 vira redundante e sai; (c) escrever o plugin de
  teste dedicado registrado em `research.md` D2 § Alternativas
- [ ] T006 [US1] Em `crates/farol-core/src/e2e_tests.rs`: cenário "uptime-kuma alcança Ready **com
  dados de widget**" — o gate de T004 já cobre o `Ready` em si; o que falta aqui é a fixture HTTP
  determinística de `/metrics` (`research.md` D2, ainda não escrita) e as assertivas sobre os itens
  do widget depois do primeiro `widget/get` bem-sucedido. É este cenário que exercita o segundo bug
  histórico (o timer de refresh só é montado quando um plugin de fato chega a `Ready` — `AGENTS.md`
  § Armadilha, ponto 2; Acceptance Scenario 2 de US1)
- [ ] T007 [US1] Em `crates/farol-core/src/e2e_tests.rs`: estender o orçamento de tempo já
  implementado em T004 (`STATE_TIMEOUT` de 30s por verificação de estado, com mensagem de falha
  nomeando `plugin_name` e o `PluginState` observado — FR-003/FR-004 já satisfeitos) com o teto de
  120s por cenário inteiro (`## Clarifications`), e confirmar que nenhum processo filho remanesce ao
  final de cada cenário (`contracts/e2e-harness-contract.md` Camada 1)
- [ ] T008 [US1] Escrever `tests/integration/harness.sh` (Camada 2, smoke de processo real,
  `research.md` D5, `contracts/e2e-harness-contract.md` Camada 2): compila `target/debug/farol`,
  sobe sob `xvfb-run -a`, confirma que sobrevive a uma janela curta de observação, encerra limpo via
  `SIGTERM`, reporta qual das três condições falhou quando falhar
- [ ] T009 [US1] Atualizar `tests/integration/README.md` para apontar para `harness.sh` (T008)
  finalmente escrito, em vez de descrever um script que nunca existiu — fecha o débito citado em
  `AGENTS.md`/`spec.md`

### Validação da User Story 1 (Cenário 1 de `quickstart.md`)

- [ ] T010 [US1] Executar Cenário 1 de `quickstart.md`: caminho feliz (T005–T009 verdes) e a
  regressão deliberada — confirmar que o harness falha de forma clara (SC-001); reverter antes de
  prosseguir. **Revisado (N1, 2026-09-01)**: a regressão originalmente proposta (reintroduzir
  `Subscription::map` com closure capturante) **não compila mais** sob `iced` 0.14
  (`const { check_zero_sized::<F>() }` ⟹ `E0080`), então não há execução de harness a observar.
  Escolher uma regressão que seja de fato observável em runtime — ex.: apontar o `command` de um
  `PluginSpawnConfig` para um binário inexistente (deve virar `Unavailable{FailedToStart}`), ou
  remover um valor da fixture de `required_config` (deve virar `Unavailable{NotConfigured}`) — os
  dois já verificados como detectáveis durante T004

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
  `rust-test` (`cargo test --workspace` — cobre os 73 testes já existentes após T004 + T005–T007 + T011–T015,
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

- [ ] T023 [US4] Criar `crates/farol-core/src/visual_snapshot_tests.rs` (módulo `#[cfg(test)]`
  declarado em `main.rs`, mesma forma de `e2e_tests.rs` — ver § Path Conventions e N3): helper que extrai uma
  representação textual determinística de um `Element<Message>` via a `Selector` API de `iced_test`
  (todo texto visível, em ordem de composição estável — `contracts/visual-snapshot-contract.md`)
- [ ] T024 [US4] Em `visual_snapshot_tests.rs`: três construtores de estado `Farol` (reaproveitando os
  construtores de fixture de estado já existentes em `update.rs`, tornando-os `pub(crate)` onde
  necessário) para `DashboardReady`, `SetupForm` e `VersionIncompatible` (`data-model.md` §3); um
  `insta::assert_snapshot!` por `screen_id`, commitando os `.snap` de referência em
  `crates/farol-core/src/snapshots/`

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
- [ ] T028 Executar Cenário 5 de `quickstart.md` (`time cargo test --package farol-core e2e_tests` —
  **não** `--test e2e_harness`: não existe um target de teste de integração, ver N3/§ Path
  Conventions) —
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
- **Foundational (T001b–T004)**: dependem de T001. T001b (migração 0.14) bloqueia **tudo** — sem ela
  o crate não compila. T004 depende de T002 (usa `program()`) e de T003 (usa a fixture hermética
  para o `required_config` de `uptime-kuma` e para o repositório git de `git-local`). **T004 bloqueia
  toda a Fase 3 (US1) e a Fase 6 (US4)** — as duas únicas fases que usam `iced_test` de fato.
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
T002 - Extrair program(boot) em crates/farol-core/src/main.rs
T003 - Fixture determinística em crates/farol-core/src/e2e_tests.rs
(T004 só começa depois de T002 estar pronto)
```

## Parallel Example: User Story 1 (após T004)

```text
T005 - Cenário git-local em crates/farol-core/src/e2e_tests.rs (BLOQUEADA por N4)
T006 - Cenário uptime-kuma em crates/farol-core/src/e2e_tests.rs
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
- ~~Nenhuma task desta feature edita `crates/farol-core/src/{update,view,model,plugin_worker}.rs`~~
  — **revisado em 2026-09-01 (N2)**: a migração `iced` 0.13→0.14 quebra `update.rs`,
  `plugin_worker.rs` e `main.rs`; corrigi-los é pré-requisito de compilação, não opcional (T001b).
  `view.rs`, `model.rs`, `config_store.rs`, `secrets_store.rs`, `crates/farol-protocol/src/*.rs` e
  `plugins/*/` seguem **intocados** — o raio de mudança em código de produção é T001b + T002.
- Commitar depois de cada task ou grupo pequeno de tasks relacionadas, seguindo a convenção já usada
  no histórico do repositório.
