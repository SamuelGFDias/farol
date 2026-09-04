---

description: "Task list template for feature implementation"
---

# Tasks: Plugin de Containers Docker

**Input**: Design documents from `/specs/005-docker-containers-plugin/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (2 documentos), quickstart.md

**Tests**: Sem TDD explicitamente requisitado pela spec, mas o projeto já estabeleceu (features
001-004) que testes automatizados fazem parte do "pronto" de cada task de comportamento — as tasks
de teste abaixo seguem esse padrão já em vigor (`AGENTS.md` § Testes), não um requisito novo desta
feature. **Uma restrição é nova**: as ações desta feature são mutantes sobre recursos reais do
usuário, então **nenhum teste automatizado pode tocar num daemon Docker real** — toda cobertura roda
contra a fixture determinística de T026 (`quickstart.md` § nota de abertura).

**Organization**: Tasks agrupadas por user story priorizada (P1 → P2, `spec.md`). A Fase Foundational
concentra tudo que é pré-requisito bloqueante compartilhado pelas duas stories — em especial o bump
de protocolo `0.3` → `0.4` (aditivo, mas que exige migrar os **três** plugins existentes para `"0.4"`
na mesma fase, `research.md` D2, mantendo o padrão que a feature 004 corrigiu em vez de reabrir o
débito #4 da feature 002).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2) — ausente em Setup, Foundational e
  Polish
- Caminho de arquivo exato em cada descrição

## Path Conventions

Estrutura definida em `plan.md` § Project Structure — mesma estrutura de workspace das features
001-004 (`crates/farol-core`, `crates/farol-protocol`, `protocol/`, `plugins/`), sem crate novo:

```text
protocol/SPEC.md                    # versão corrente "0.4"; novo kind "container-status-grid";
                                    # duas linhas novas no catálogo de erro
protocol/schema/v0.3/               # RETIDO como registro histórico — não editado nesta feature
protocol/schema/v0.4/               # NOVO — ContainerState/ContainerStatusItem, 4ª opção de
                                    # WidgetGetResult.items, 3ª opção de ActionInvokeResult
crates/farol-protocol/src/messages.rs    # +ContainerState/+ContainerStatusItem,
                                         # WidgetItems::Container, ActionInvokeResult::Container
crates/farol-protocol/tests/*.rs         # contract_schema_validation.rs/schema_boundaries.rs
                                         # apontam para v0.4 + exemplos/fronteiras novos
crates/farol-core/src/plugin_worker.rs   # CORE_PROTOCOL_VERSION 0.3 → 0.4; known_plugins()
crates/farol-core/src/{model,update,view}.rs   # ver Fases 2-4
plugins/git-local/main.py           # PROTOCOL_VERSION "0.3" → "0.4" (mecânico, sem mudança de wire)
plugins/uptime-kuma/main.py         # idem
plugins/openfortivpn-vpn/main.py    # idem
plugins/docker-containers/          # NOVO plugin de referência (main.py, docker_cli.py,
                                    # pyproject.toml, test_docker_cli.py)
tests/fixtures/fake-docker/         # NOVO — binário `docker` de teste, sem daemon real
tests/integration/harness.sh        # Camada 2 — nova condição para docker-containers
```

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Esqueleto de arquivos do novo plugin e do novo diretório de schema — sem lógica de
protocolo nem de negócio ainda.

- [X] T001 [P] Criar esqueleto de `plugins/docker-containers/` (`main.py`, `docker_cli.py` — stubs,
  apenas stdlib: `json`/`sys`/`subprocess`/`shutil`) e `pyproject.toml` com config `ruff`, mesmo
  padrão de `plugins/openfortivpn-vpn/`
- [X] T002 [P] Criar `protocol/schema/v0.4/` copiando os 4 arquivos de `protocol/schema/v0.3/` como
  ponto de partida (ainda idênticos a `v0.3/`, prontos para edição na Fase 2) — `v0.3/` permanece
  congelado, mesmo tratamento já dado a `v0.1/`/`v0.2/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Bump de protocolo `0.3 → 0.4` (com a migração dos três plugins existentes), tipos novos
do `farol-protocol` e plumbing genérico do core — tudo que AMBAS as user stories dependem.

**⚠️ CRITICAL**: Nenhuma task de US1/US2 pode começar antes desta fase completar.

### Protocolo

- [X] T003 Em `protocol/schema/v0.4/widget.schema.json`: adicionar `$defs.ContainerState` (7 estados
  do Docker + `unknown`), `$defs.ContainerStatusItem` (`id` com `"pattern": "^[0-9a-f]{64}$"`,
  `name`, `image`, `state`, `status_text`, e os três `*_action`), e a quarta opção de
  `WidgetGetResult.items` (`contracts/protocol-delta-v0.4.md` § widget.schema.json)
- [X] T004 Em `protocol/schema/v0.4/action.schema.json`: acrescentar a terceira opção do `oneOf` de
  `ActionInvokeResult` (`{"container": ContainerStatusItem}`) — chaves de topo disjuntas das duas
  existentes (`contracts/protocol-delta-v0.4.md` § action.schema.json)
- [X] T005 Em `protocol/schema/v0.4/error.schema.json`: estender o catálogo textual com `-32010`/
  `docker_unavailable` (com `data.detail.condition`) e `-32011`/`container_action_failed` (com
  `data.detail.docker_condition`) — `data.reason` já é string aberta, sem mudança de forma
  (`contracts/protocol-delta-v0.4.md` § error.schema.json)
- [X] T006 [P] Em `protocol/schema/v0.4/handshake.schema.json`: só o comentário de topo
  (`description`) muda, citando `"0.4"`, o novo `kind` e o novo plugin de referência — sem mudança de
  forma; `ActionDeclaration`/`ActionTarget` ficam intactos
  (`contracts/protocol-delta-v0.4.md` § handshake.schema.json)
- [X] T007 Atualizar `protocol/SPEC.md`: §5.2.1 (linha de `container-status-grid` na tabela de
  `kind`s), §6.4 (versão corrente `"0.4"`), §7.2 (nota sobre `timeout_hint_ms` explícito das ações de
  container), §8.2 (duas linhas novas do catálogo de erro), §11 (`v0.4/` corrente, `v0.3/` congelada)
  — `contracts/protocol-delta-v0.4.md` § protocol/SPEC.md

### `farol-protocol` (Rust)

- [X] T008 [P] Em `crates/farol-protocol/src/messages.rs`: adicionar `ContainerState` (enum de 8
  variantes, `Unknown` como fallback de FR-012) e `ContainerStatusItem` (`id`/`name`/`image`/`state`/
  `status_text`/`start_action`/`stop_action`/`restart_action`) — `data-model.md` §1.2-§1.3
- [X] T009 Em `crates/farol-protocol/src/messages.rs`: `WidgetItems` ganha variante
  `Container(Vec<ContainerStatusItem>)` (depende de T008) — `data-model.md` §1.5. Confirmar por
  teste a análise de disjunção de `research.md` D12 (um `ContainerStatusItem` não desserializa como
  `Git`/`Monitor`/`Vpn`, e vice-versa)
- [X] T010 Em `crates/farol-protocol/src/messages.rs`: `ActionInvokeResult` ganha a terceira variante
  `Container { container: ContainerStatusItem }` (depende de T008) — `data-model.md` §1.6; nenhum
  ponto de construção existente muda (as chaves de topo são disjuntas)
- [X] T011 [P] Em `crates/farol-protocol/tests/contract_schema_validation.rs`: apontar os 4
  `include_str!` e os 4 `$id` de `v0.3` para `v0.4`, e adicionar exemplos válidos de
  `ContainerStatusItem` (um por estado relevante, incluindo `unknown`) e de
  `ActionInvokeResult::Container` — mesmo padrão dos exemplos já existentes de
  `GitRepository`/`MonitorStatusItem`/`VpnStatusItem`
- [X] T012 [P] Em `crates/farol-protocol/tests/schema_boundaries.rs`: apontar os 4 `include_str!` e
  `$id` para `v0.4`, e adicionar casos de fronteira de `ContainerStatusItem` — `id` com 63/64/65
  caracteres e com maiúscula (o `pattern` de 64 hex minúsculos), `state` fora do enum, e
  `additionalProperties: false` — mesmo padrão de
  `widget_monitor_status_item_response_time_ms_minimum_boundaries` (issue #5)

### Bump de versão e migração dos plugins existentes (`research.md` D2)

- [X] T013 Em `crates/farol-core/src/plugin_worker.rs`: `CORE_PROTOCOL_VERSION` de
  `ProtocolVersion { major: 0, minor: 3 }` para `{ major: 0, minor: 4 }`, e atualizar as mensagens de
  asserção de `crates/farol-core/src/e2e_tests.rs` que citam a versão literal em texto (ex.:
  "handshake 0.3") para não passarem a mentir — a quebra real vem de T014-T016 não estarem aplicadas
  (`research.md` D2, § Consequência de teste)
- [X] T014 [P] Em `plugins/git-local/main.py`: `PROTOCOL_VERSION` `"0.3"` → `"0.4"` (mudança mecânica
  de uma linha — nenhum campo novo usado por este plugin)
- [X] T015 [P] Em `plugins/uptime-kuma/main.py`: idem T014
- [X] T016 [P] Em `plugins/openfortivpn-vpn/main.py`: idem T014

### `farol-core` (Rust) — plumbing genérico

- [X] T017 Em `crates/farol-core/src/plugin_worker.rs::known_plugins()`: adicionar
  `PluginSpawnConfig { plugin_name: "docker-containers", command: "python3", args:
  ["plugins/docker-containers/main.py"] }` (`research.md` D8)
- [X] T018 Em `crates/farol-core/src/model.rs`: adicionar `ContainerActionKind`
  (`Start`/`Stop`/`Restart`, só de UI), `ContainerViewModel`
  (`item`/`action_in_flight`/`last_action_error` — o erro de ação é **por container**, não do widget
  inteiro, `data-model.md` §2.2), `DockerWidgetViewModel` (`containers`/`last_error`/`loaded`, com
  `Default`) e `PluginConnection::docker_widget` — `data-model.md` §2.1-§2.3. O campo `loaded`
  existe para distinguir "ainda não li" de "li e está vazio" (FR-011)
- [X] T019 Em `crates/farol-core/src/update.rs`: `WidgetKind` ganha `Container` e
  `normalize_widget_items` ganha o quarto braço para o `kind` `"container-status-grid"` — necessário
  porque `items: []` é ambíguo sob `#[serde(untagged)]` e desserializaria como `Git` (débito #5,
  issue #7); sem isso, FR-011 (lista vazia legítima) quebra silenciosamente

### Plugin `docker-containers` — handshake

- [X] T020 [P] Em `plugins/docker-containers/main.py`: `handle_handshake_hello` — declara o widget
  (`id: "docker-containers"`, `kind: "container-status-grid"`, `title: "Containers Docker"`),
  `capabilities: [{"kind": "exec"}]`, `required_config: []` (sem tela de setup, `research.md` D9),
  `actions: []` (as três ações são descobertas por item em `widget/get`, mesmo padrão de `git-local`)

**Checkpoint**: o protocolo fala `"0.4"`; `git-local`/`uptime-kuma`/`openfortivpn-vpn` continuam
chegando a `Ready` normalmente; `docker-containers` completa o handshake mas `widget/get`/
`action/invoke` ainda não fazem nada real (cobertos pelas fases de user story abaixo).

---

## Phase 3: User Story 1 - Ver o estado dos containers sem abrir terminal (Priority: P1) 🎯 MVP

**Goal**: O widget "Containers Docker" lista **todos** os containers locais (inclusive os parados)
com nome, imagem e estado, em ordem estável, atualizando por polling — sem exigir nenhuma ação do
usuário.

**Independent Test**: Com o Docker instalado e containers em estados diferentes (ao menos um rodando
e um parado), abrir o Farol e conferir que o widget lista todos eles corretamente, sem nenhuma outra
interação (`quickstart.md` Cenários 1-3, 8-10).

### Implementation for User Story 1

- [X] T021 [US1] Em `plugins/docker-containers/docker_cli.py`: `find_binary()` (via `shutil.which`) e
  `list_containers()` — invoca `docker ps --all --no-trunc --format '{{json .}}'` com timeout de
  **3 s** (FR-014), parseia NDJSON, mapeia os campos `ID`/`Names`/`Image`/`State`/`Status` para o
  shape de `ContainerStatusItem` (`contracts/docker-cli-mapping.md` § Mapeamento de campos), **monta
  as três `ActionDeclaration` de cada item** (`docker.container.start`/`.stop`/`.restart`, os três com
  `target: {type: "docker-container", id: <id do próprio item>}`, `timeout_hint_ms`
  `20000`/`35000`/`45000`, e `enabled` calculado pela matriz de FR-008 — `data-model.md` §1.3
  invariantes 3-6; é o plugin que decide `enabled`, nunca o core), traduz
  estado desconhecido para `unknown` sem invalidar as demais linhas (FR-012), ordena por
  `(name, id)` (`research.md` D10), e classifica falhas em `-32003`/`exec_unavailable` (binário
  ausente) ou `-32010`/`docker_unavailable` com `condition` ∈
  `{daemon_unreachable, permission_denied, timeout, cli_error}` — **na ordem normativa da tabela**
  (`permission_denied` testado ANTES de `daemon_unreachable`)
- [X] T022 [US1] Em `plugins/docker-containers/main.py`: `handle_widget_get` para o `widget_id`
  `"docker-containers"`, delegando a `docker_cli.list_containers()`; lista vazia devolve **sucesso**
  com `items: []` (FR-011), nunca erro
- [X] T023 [US1] [P] Em `plugins/docker-containers/test_docker_cli.py`: testes de `list_containers` —
  lista multi-estado com os três `enabled` corretos por linha (matriz de FR-008), lista vazia,
  estado fora do vocabulário virando `unknown` sem derrubar as demais linhas, ordenação estável,
  linha não parseável, e as quatro condições de falha, **incluindo um caso cujo stderr contém
  simultaneamente "permission denied" e um texto de falha de conexão**, provando a ordem de
  classificação (`research.md` D5.1 — é a regra mais fácil de regredir)
- [X] T024 [US1] Em `crates/farol-core/src/update.rs`: `handle_widget_outcome` roteia
  `WidgetItems::Container` para `PluginConnection::docker_widget` (mesmo padrão já usado para
  `monitor_widget`/`vpn_widget`) — sucesso substitui `containers` e marca `loaded = true`; falha
  popula `last_error` **preservando a última lista conhecida** (FR-006), nunca zerando.
  Inclui a generalização de `merge_widget_items`/`MergedWidgetItems` descrita em `data-model.md`
  §2.4: hoje a função só enxerga `previous: &[RepositoryViewModel]`, e precisa enxergar também os
  `ContainerViewModel` anteriores para o merge por `id` — a preservação de `action_in_flight` que
  isso habilita só é exercida em US2 (T031), mas a mudança de assinatura pertence a esta task, onde
  a variante é introduzida
- [X] T025 [US1] Em `crates/farol-core/src/view.rs`: renderização somente leitura do widget
  `container-status-grid` — uma linha por container com nome, imagem e estado legível
  (`data-model.md` §1.2), indicação explícita de "nenhum container" quando `loaded && containers
  vazio` (FR-011), e mensagem de erro quando `last_error` presente. **Sem botões nesta fase**
  (iniciar/parar/reiniciar é US2) — coerente com a justificativa de prioridade de US1 no `spec.md`
- [X] T026 [US1] [P] Criar `tests/fixtures/fake-docker/` — binário `docker` de teste (Python)
  controlável por variável de ambiente (ex.: `FAKE_DOCKER_SCENARIO`), cobrindo lista multi-estado
  (incluindo um `unknown`), lista vazia, os três modos de falha de `docker ps`, travamento (para
  exercitar o timeout de 3 s) e as ações de US2; e adicionar em `crates/farol-core/src/e2e_tests.rs`
  o cenário `docker_containers_reaches_ready_and_populates_the_container_grid`, com o diretório da
  fixture no início do `PATH` via `PathPrefixGuard` (mesmo padrão de
  `tests/fixtures/fake-openfortivpn-gui/`). **Nenhum teste toca num daemon Docker real**
- [X] T027 [US1] [P] Em `crates/farol-core/src/visual_snapshot_tests.rs`: novo snapshot cobrindo o
  widget populado com containers em estados diferentes, mais o estado "nenhum container", mesmo
  padrão de `dashboard_ready_state`

**Checkpoint**: User Story 1 completa e testável de forma independente — MVP, já satisfaz SC-001.

---

## Phase 4: User Story 2 - Iniciar, parar e reiniciar um container pelo widget (Priority: P2)

**Goal**: O usuário inicia, para ou reinicia um container a partir do próprio widget, com feedback de
operação em curso, controles coerentes com o estado e tradução legível de qualquer falha.

**Independent Test**: A partir do widget com um container parado, acionar iniciar e confirmar que ele
passa a rodando; a partir de um rodando, acionar parar e reiniciar e confirmar os estados
resultantes (`quickstart.md` Cenários 4-7).

### Implementation for User Story 2

- [X] T028 [US2] [P] Em `plugins/docker-containers/docker_cli.py`: `start(id)`, `stop(id)` e
  `restart(id)` — invocam `docker start|stop|restart <id>` com os timeouts de subprocess de
  15 s/30 s/40 s (`research.md` D6, sem passar `--time` para encurtar a graça do `stop`, FR-015);
  em sucesso, fazem a **releitura pontual** `docker ps --all --no-trunc --filter id=<id>` e devolvem
  o `ContainerStatusItem` inteiro (`research.md` D11 — `enabled` mudou com o estado, e §5.3 do
  `protocol/SPEC.md` proíbe o core recalculá-lo); releitura vazia vira `-32011` com
  `docker_condition: "container_gone"`; falha vira `-32011` com `docker_condition` classificado
  **nesta ordem**: `no_such_container` → `permission_denied` → `daemon_unreachable` → `cli_error`
  (`contracts/docker-cli-mapping.md` § action/invoke)
- [X] T029 [US2] Em `plugins/docker-containers/main.py`: `handle_action_invoke` — dispatch por
  `action_id` (`"docker.container.start"`/`".stop"`/`".restart"`), validando
  `target.type == "docker-container"` e usando `target.id` (nunca o nome, FR-013), delegando a
  `docker_cli.start`/`stop`/`restart`
- [X] T030 [US2] [P] Em `plugins/docker-containers/test_docker_cli.py`: testes de `start`/`stop`/
  `restart` — sucesso com releitura devolvendo `enabled` recalculado, releitura vazia
  (`container_gone`), e cada uma das quatro condições de falha mais `timeout`, com a mensagem PT-BR
  esperada da tabela de `contracts/docker-cli-mapping.md`
- [X] T031 [US2] Em `crates/farol-core/src/update.rs`: tratar a resposta de
  `Message::ActionInvokeRequested`/`Message::Worker` para as três ações — sucesso substitui **apenas
  o item correspondente** da lista, casando por `id`; `action_in_flight` é setado ao disparar e
  limpo ao receber resposta (sucesso ou erro); erro popula o `last_action_error` **do
  `ContainerViewModel` daquele container** (`data-model.md` §2.2 — por item, não do widget), sem
  derrubar a lista (FR-009). O merge de um `widget/get` que chegue durante a operação MUST preservar
  `action_in_flight` por `id`, e MUST limpá-lo se o container sumir da lista (FR-017,
  `data-model.md` §2.4)
- [X] T032 [US2] Em `crates/farol-core/src/view.rs`: três botões por linha, disparando
  `Message::ActionInvokeRequested` com a `ActionDeclaration` correspondente do próprio item;
  habilitados quando `action.enabled && !action_in_flight` — o core pode **recusar** o que o plugin
  habilitou (FR-017), mas nunca **habilitar** o que o plugin desabilitou (`research.md` D7,
  Princípio III); indicador visual de operação em curso naquela linha, com as demais linhas
  permanecendo acionáveis; o `last_action_error` **daquela linha** exibido quando presente
- [X] T033 [US2] [P] Em `crates/farol-core/src/e2e_tests.rs`: cenários usando a fixture de T026 —
  `docker.container.start` bem-sucedido leva aquela linha a "rodando" com os `enabled` invertidos, e
  uma falha simulada (`no_such_container`) resulta em mensagem traduzida visível sem derrubar o core
  nem apagar a lista
- [X] T034 [US2] [P] Em `crates/farol-core/src/visual_snapshot_tests.rs`: snapshot cobrindo a linha
  com os três botões em estados de habilitação diferentes (matriz de FR-008) e uma linha com operação
  em curso

**Checkpoint**: User Stories 1 e 2 funcionam, cada uma de forma independente.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Verificação final, regressão (SC-005) e higiene documental.

- [X] T035 [P] Rodar `ruff check` dentro de `plugins/docker-containers/` — deve ficar limpo
- [X] T036 [P] Rodar `cargo clippy --workspace --all-targets` — deve ficar limpo, sem warning novo
- [X] T037 Rodar `cargo test --workspace` — confirmar toda a suíte passando, incluindo os cenários
  novos de T011/T012/T026/T027/T033/T034, e que `git-local`/`uptime-kuma`/`openfortivpn-vpn`
  continuam chegando a `Ready` sob `"0.4"` (SC-005)
- [X] T038 Estender `tests/integration/harness.sh` (Camada 2) com uma condição adicional confirmando
  que `docker-containers` também chega a `Ready` sob Xvfb, reusando a fixture de T026 (o harness
  **não** pode depender de um Docker instalado na máquina de CI)
- [X] T039 Validar manualmente os 11 cenários de `quickstart.md` — ou confirmar, cenário a cenário,
  qual já tem equivalente automatizado registrado nas tasks acima, mesmo padrão da nota de
  `specs/004-vpn-status-plugin/quickstart.md`. Os Cenários 3, 5, 9 e 11 (parte visual) permanecem
  dependentes de validação manual por natureza
- [X] T040 Atualizar `README.md` se o roadmap/status do produto mudar de forma material — em
  particular, marcar a metade "containers up/down" da integração Docker como entregue e a metade
  "logs" como adiada com link para a issue #10 (regra de governance da constitution)
- [X] T041 Atualizar `AGENTS.md` marcando a feature 005 como completa, com o resumo final (plugin
  `docker-containers`, protocolo `"0.4"`, migração dos três plugins anteriores, novo `kind`
  `container-status-grid`), mesmo padrão das features 001-004

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: sem dependências — pode começar imediatamente
- **Foundational (Phase 2)**: depende de Setup — BLOQUEIA todas as user stories
- **User Stories (Phase 3+)**: ambas dependem da conclusão de Foundational
  - US1 (P1) pode começar assim que Foundational completar
  - US2 (P2) depende tecnicamente de US1 em dois pontos concretos, não só por ordem de prioridade:
    T031 mexe no mesmo roteamento de estado que T024 cria, e T032 acrescenta botões às linhas que
    T025 desenha. Não é uma dependência artificial
- **Polish (Phase 5)**: depende de todas as user stories desejadas estarem completas

### Dentro da Fase Foundational

- T003-T007 (schema/SPEC) antes de T011/T012 (que validam contra os arquivos editados)
- T008 antes de T009 e T010 (as duas variantes referenciam `ContainerStatusItem`)
- T013 depende de T014-T016 para a suíte voltar ao verde — na prática as quatro devem ser aplicadas
  e verificadas juntas, não em commits isolados (`research.md` D2)

### Dentro de cada User Story

- Mapeamento Python (`docker_cli.py`) antes do dispatch (`main.py`)
- `update.rs` (roteamento de estado) antes de `view.rs` (renderização) — a `view` lê o `Model` que
  `update.rs` popula
- Testes de plugin (`test_docker_cli.py`) podem rodar em paralelo à implementação Rust da mesma
  story (arquivos/linguagens diferentes)

### Parallel Opportunities

- Todas as tasks `[P]` de Setup e Foundational (T001-T002, T006, T008, T011/T012, T014-T016, T020)
- Dentro de US1: T023 (testes Python), T026 (fixture + e2e) e T027 (snapshot) são independentes entre
  si depois que T021/T024/T025 estabelecem o comportamento
- Dentro de US2: T028 e T030 (Python) em paralelo com T033/T034 (Rust)

---

## Parallel Example: Foundational (protocolo)

```bash
Task: "Adicionar ContainerState/ContainerStatusItem em crates/farol-protocol/src/messages.rs (T008)"
Task: "Atualizar handshake.schema.json v0.4 (descrição only) (T006)"
Task: "PROTOCOL_VERSION 0.4 em plugins/git-local/main.py (T014)"
Task: "PROTOCOL_VERSION 0.4 em plugins/uptime-kuma/main.py (T015)"
Task: "PROTOCOL_VERSION 0.4 em plugins/openfortivpn-vpn/main.py (T016)"
```

## Parallel Example: User Story 1

```bash
Task: "Testes de list_containers em plugins/docker-containers/test_docker_cli.py (T023)"
Task: "Fixture fake-docker + cenário e2e (T026)"
Task: "Snapshot visual do widget container-status-grid (T027)"
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Phase 1: Setup
2. Completar Phase 2: Foundational (CRITICAL — bloqueia ambas as stories; o bump `0.4` só fica
   consistente com os três plugins migrados)
3. Completar Phase 3: User Story 1
4. **PARAR e VALIDAR**: testar User Story 1 de forma independente (`quickstart.md` Cenários 1-3,
   8-10)
5. Entregar/demonstrar se pronto — já satisfaz SC-001, SC-002, SC-004 (na parte de leitura) e SC-006

### Incremental Delivery

1. Setup + Foundational → protocolo `"0.4"` pronto, os quatro plugins chegam a `Ready`
2. + User Story 1 → testar independentemente → MVP (visibilidade, verbo "Ver")
3. + User Story 2 → testar independentemente → iniciar/parar/reiniciar pelo widget (verbo "Agir")
4. Polish → regressão confirmada (SC-005), documentação atualizada

---

## Notas

- **Dívidas já registradas** (Governance da constitution — dívida deliberada MUST virar issue):
  issue #9 (desambiguação untagged de `WidgetItems` depende de disjunção incidental de campos —
  mitigada nesta feature pelo nome `status_text` e pelo teste de T009, resolvida de vez só por uma
  feature própria de protocolo) e issue #10 (superfície de detalhe/drill-down no core, pré-requisito
  para logs de container). Nenhuma das duas bloqueia qualquer task acima.
- **Acoplamento consciente**: a classificação de erro por substring de stderr (T021, T028) é frágil
  por natureza — a CLI do Docker já reformulou essas mensagens entre versões maiores. Está aceita e
  mitigada em três camadas (ordem de teste, `fallback` `cli_error`, `data.detail.raw` sempre
  preenchido), registrada em `plan.md` § Complexity Tracking. T023 e T030 existem em boa parte para
  travar essa mitigação.
- Se, durante a implementação, surgir alguma dívida técnica deliberadamente adiada, registrar como
  issue no tracker do projeto antes de considerar a mudança correspondente concluída (regra de
  governance da constitution).
