---

description: "Task list template for feature implementation"
---

# Tasks: Plugin de Status de VPN (openfortivpn-gui)

**Input**: Design documents from `/specs/004-vpn-status-plugin/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (2 documentos), quickstart.md

**Tests**: Sem TDD explicitamente requisitado pela spec, mas o projeto já estabeleceu (features
001-003) que testes automatizados fazem parte do "pronto" de cada task de comportamento — as tasks
de teste abaixo seguem esse padrão já em vigor (`AGENTS.md` § Testes), não um requisito novo desta
feature.

**Organization**: Tasks agrupadas por user story priorizada (P1 → P2 → P3, `spec.md`). A Fase
Foundational concentra tudo que é pré-requisito bloqueante compartilhado por todas as stories — em
especial o bump de protocolo `0.2` → `0.3` (aditivo, mas que exige migrar `git-local`/`uptime-kuma`
para `"0.3"` na mesma fase, `research.md` D2, para não repetir o padrão de dívida técnica da
migração `0.1→0.2`, débito #4) — e uma generalização de nome descoberta durante o planejamento desta
feature (T019 abaixo).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2, US3) — ausente em Setup, Foundational
  e Polish
- Caminho de arquivo exato em cada descrição

## Path Conventions

Estrutura definida em `plan.md` § Project Structure — mesma estrutura de workspace das features
001-003 (`crates/farol-core`, `crates/farol-protocol`, `protocol/`), sem crate novo:

```text
protocol/SPEC.md                   # título/versão passam a descrever "0.3"; novo kind "vpn-status"
protocol/schema/v0.2/               # RETIDO como registro histórico — não editado nesta feature
protocol/schema/v0.3/               # NOVO — VpnConnectionState/VpnProfile/VpnStatusItem,
                                     # ActionInvokeResult vira oneOf, catálogo de erro estendido
crates/farol-protocol/src/messages.rs   # +VpnConnectionState/VpnProfile/VpnStatusItem,
                                         # WidgetItems::Vpn, ActionInvokeResult vira enum untagged
crates/farol-protocol/tests/*.rs        # contract_schema_validation.rs/schema_boundaries.rs
                                         # apontam para v0.3 + fixtures/boundaries novos
crates/farol-core/src/{main,model,update,view,plugin_worker}.rs   # todos estendidos (ver Fases
                                                                    # 2 e 3-5 para o que muda em cada)
plugins/git-local/main.py           # PROTOCOL_VERSION "0.2" → "0.3" (mecânico, sem mudança de wire)
plugins/uptime-kuma/main.py         # idem
plugins/openfortivpn-vpn/           # NOVO plugin de referência (main.py, vpn_cli.py,
                                     # pyproject.toml, test_vpn_cli.py)
tests/fixtures/fake-openfortivpn-gui/   # NOVO — binário de teste simulando o contrato da CLI
                                         # (status/connect/disconnect --json), sem abrir túnel real
tests/integration/harness.sh        # Camada 2 — nova condição para openfortivpn-vpn
```

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Esqueleto de arquivos do novo plugin e do novo diretório de schema — sem lógica de
protocolo nem de negócio ainda.

- [X] T001 [P] Criar esqueleto de `plugins/openfortivpn-vpn/` (`main.py`, `vpn_cli.py` — stubs,
  apenas stdlib) e `pyproject.toml` com config `ruff`, mesmo padrão de `plugins/uptime-kuma/`
- [X] T002 [P] Criar `protocol/schema/v0.3/` copiando os 4 arquivos de `protocol/schema/v0.2/`
  como ponto de partida (ainda idênticos a `v0.2/`, prontos para edição nas Fases seguintes)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Bump de protocolo `0.2 → 0.3`, plumbing genérico do core, e a generalização de nome
descoberta nesta feature — tudo que TODAS as user stories dependem.

**⚠️ CRITICAL**: Nenhuma task de US1/US2/US3 pode começar antes desta fase completar.

### Protocolo

- [X] T003 Em `protocol/schema/v0.3/widget.schema.json`: adicionar `$defs.VpnConnectionState`,
  `$defs.VpnProfile`, `$defs.VpnStatusItem`, e a terceira opção de `WidgetGetResult.items`
  (`contracts/protocol-delta-v0.3.md` § widget.schema.json)
- [X] T004 Em `protocol/schema/v0.3/action.schema.json`: `ActionInvokeResult` vira `oneOf`
  (`repo` | `vpn_status`) (`contracts/protocol-delta-v0.3.md` § action.schema.json)
- [X] T005 Em `protocol/schema/v0.3/error.schema.json`: estender o catálogo textual com `-32008`/
  `vpn_status_unavailable` e `-32009`/`vpn_action_failed` (`contracts/protocol-delta-v0.3.md` §
  error.schema.json) — `data.reason` já é string aberta, sem mudança de forma
- [X] T006 [P] Em `protocol/schema/v0.3/handshake.schema.json`: só o comentário de topo
  (`description`) muda, citando `"0.3"` e o novo `kind` — sem mudança de forma
  (`contracts/protocol-delta-v0.3.md` § handshake.schema.json)
- [X] T007 Atualizar `protocol/SPEC.md`: versão atual `"0.3"`, registrar `kind: "vpn-status"` ao
  lado de `status-grid`/`monitor-status-grid`, e as duas linhas novas da tabela de erro
  (`contracts/protocol-delta-v0.3.md` § protocol/SPEC.md)

### `farol-protocol` (Rust)

- [X] T008 [P] Em `crates/farol-protocol/src/messages.rs`: adicionar `VpnConnectionState`
  (enum `Disconnected`/`Connecting`/`Connected`), `VpnProfile` (`name` + `connect_action`),
  `VpnStatusItem` (`state`/`active_profile`/`elapsed_seconds`/`available_profiles`/
  `disconnect_action`) — `data-model.md` §1.1-§1.3
- [X] T009 Em `crates/farol-protocol/src/messages.rs`: `WidgetItems` ganha variante
  `Vpn(Vec<VpnStatusItem>)` (depende de T008) — `data-model.md` §1.4
- [X] T010 Em `crates/farol-protocol/src/messages.rs`: `ActionInvokeResult` deixa de ser struct
  única (`{ repo: GitRepository }`) e vira `#[serde(untagged)] enum { Git { repo }, Vpn {
  vpn_status } }` (depende de T008) — `data-model.md` §1.5; atualizar todos os pontos de
  construção/leitura existentes (`plugin_worker.rs`, testes de `git-local`) para o novo shape
  `ActionInvokeResult::Git { repo }`
- [X] T011 [P] Em `crates/farol-protocol/tests/contract_schema_validation.rs`: apontar os 4
  `include_str!`/`$id` de `v0.2` para `v0.3`, e adicionar exemplos válidos de
  `VpnStatusItem`/`ActionInvokeResult::Vpn` (mesmo padrão dos exemplos já existentes de
  `GitRepository`/`MonitorStatusItem`)
- [X] T012 [P] Em `crates/farol-protocol/tests/schema_boundaries.rs`: apontar os 4 `include_str!`
  para `v0.3`, e adicionar caso de fronteira para `VpnStatusItem.elapsed_seconds` (`minimum: 0`) —
  mesmo padrão de `widget_monitor_status_item_response_time_ms_minimum_boundaries` (issue #5,
  débito já corrigido para `response_time_ms`; não repetir a lacuna para o campo novo)

### `farol-core` (Rust)

- [X] T013 Em `crates/farol-core/src/plugin_worker.rs`: `PROTOCOL_VERSION` `"0.2"` → `"0.3"`
- [X] T014 [P] Em `plugins/git-local/main.py`: `PROTOCOL_VERSION` `"0.2"` → `"0.3"` (mudança
  mecânica de uma linha — nenhum campo novo usado por este plugin, `research.md` D2)
- [X] T015 [P] Em `plugins/uptime-kuma/main.py`: idem T014
- [X] T016 Em `crates/farol-core/src/plugin_worker.rs::known_plugins()`: adicionar
  `PluginSpawnConfig { plugin_name: "openfortivpn-vpn", command: "python3", args:
  ["plugins/openfortivpn-vpn/main.py"] }`
- [X] T017 Em `crates/farol-core/src/model.rs`: adicionar `VpnWidgetViewModel` (`status`/
  `last_error`/`connect_in_flight`/`disconnect_in_flight`/`last_action_error`, todos com
  `Default`) e `PluginConnection::vpn_widget: VpnWidgetViewModel` — `data-model.md` §2.1
- [X] T018 Em `crates/farol-core/src/update.rs`: `normalize_widget_items` reconhece o `kind`
  `"vpn-status"` (mesma correção já aplicada para `"status-grid"`/`"monitor-status-grid"`, débito
  #5) — necessário mesmo com só uma variante possível de item, para consistência com o mecanismo
  existente
- [X] T019 Em `crates/farol-core/src/main.rs`/`update.rs`/`view.rs`: renomear
  `Message::FetchRequested` → `Message::ActionInvokeRequested` (campos idênticos —
  `plugin_name`/`action_id`/`target`/`timeout_hint_ms`) e todos os pontos de disparo/consumo do
  botão "Fetch" de `git-local`. **Achado desta feature**: o nome atual é específico de
  `git.fetch`, mas o shape já é genérico (`ActionTarget` livre) — sem essa renomeação, US2 desta
  feature precisaria de uma segunda variante de `Message` quase idêntica só para
  `vpn.connect`/`vpn.disconnect`. Puramente uma generalização de nome, sem mudança de
  comportamento observável para `git-local`.

### Plugin `openfortivpn-vpn` — handshake

- [X] T020 [P] Em `plugins/openfortivpn-vpn/main.py`: `handle_handshake_hello` — declara o widget
  `vpn-status` (`id: "vpn-connection"`, `kind: "vpn-status"`, `title: "VPN"`),
  `capabilities: [{"kind": "exec"}]`, `required_config: []`, `actions: []` (descobertas só em
  `widget/get`, mesmo padrão de `git-local`)

**Checkpoint**: protocolo fala `"0.3"`; `git-local`/`uptime-kuma` continuam chegando a `Ready`
normalmente; `openfortivpn-vpn` completa o handshake mas `widget/get`/`action/invoke` ainda não
fazem nada real (cobertos pelas fases de user story abaixo).

---

## Phase 3: User Story 1 - Ver o estado da VPN sem trocar de janela (Priority: P1) 🎯 MVP

**Goal**: O widget "VPN" mostra o estado atual (desconectado/conectando/conectado, perfil ativo,
perfis disponíveis) sem exigir nenhuma ação do usuário, atualizando por polling.

**Independent Test**: Com o `openfortivpn-gui` instalado e uma conexão manual (fora do Farol) ativa
ou inativa, abrir o Farol e conferir que o widget reflete o estado real, sem nenhuma outra
interação (`quickstart.md` Cenários 1-2).

### Implementation for User Story 1

- [X] T021 [US1] Em `plugins/openfortivpn-vpn/vpn_cli.py`: `find_binary()` (via `shutil.which`) e
  `query_status()` — invoca `openfortivpn-gui status --json`, mapeia `StatusPayload` para o shape
  de `VpnStatusItem` completo (`data-model.md` §3), incluindo `connect_action`/`disconnect_action`
  com `enabled` correto (`research.md` D4); erros mapeados para `-32003`/`exec_unavailable` (binário
  ausente) ou `-32008`/`vpn_status_unavailable` (`contracts/openfortivpn-cli-mapping.md`)
- [X] T022 [US1] Em `plugins/openfortivpn-vpn/main.py`: `handle_widget_get` para o `widget_id`
  `"vpn-connection"`, delegando a `vpn_cli.query_status()`
- [X] T023 [US1] [P] Em `plugins/openfortivpn-vpn/test_vpn_cli.py`: testes de `query_status` —
  conectado, desconectado, `profiles: []`, binário ausente (`-32003`), `internal_error`/saída
  inválida (`-32008`) — mesmo padrão colocado-junto-do-código de `plugins/uptime-kuma/test_*.py`
  (D7 de `research.md` da feature 002)
- [X] T024 [US1] Em `crates/farol-core/src/update.rs`: `handle_widget_outcome` roteia
  `WidgetItems::Vpn` para `PluginConnection::vpn_widget` (`status`/`last_error`), mesmo padrão já
  usado para `monitor_widget`
- [X] T025 [US1] Em `crates/farol-core/src/view.rs`: renderização somente leitura do widget
  `vpn-status` — texto de estado, perfil ativo quando conectado, lista de perfis disponíveis
  (ou indicação explícita de lista vazia, FR-003), mensagem de erro quando `last_error` presente.
  **Sem botões nesta fase** (conectar/desconectar é US2) — coerente com a própria justificativa de
  prioridade de US1 no `spec.md` ("entrega valor completo... mesmo sem nenhuma ação de
  conectar/desconectar")
- [X] T026 [US1] [P] Criar `tests/fixtures/fake-openfortivpn-gui/` — script executável (Python)
  simulando o contrato da CLI real (`status`/`connect`/`disconnect --json`,
  `contracts/openfortivpn-cli-mapping.md`) controlável por variável de ambiente (ex.:
  `FAKE_OPENFORTIVPN_STATE`, `FAKE_OPENFORTIVPN_ERROR`), sem abrir nenhum túnel de verdade; e
  adicionar em `crates/farol-core/src/e2e_tests.rs` o cenário `openfortivpn_vpn_reaches_ready_
  and_populates_the_vpn_widget` (mesmo padrão de
  `uptime_kuma_reaches_ready_and_populates_the_monitor_grid`), prepend do diretório da fixture ao
  `PATH` do processo filho
- [X] T027 [US1] [P] Em `crates/farol-core/src/visual_snapshot_tests.rs`: novo snapshot cobrindo o
  widget `vpn-status` populado (estado desconectado com perfis, e estado conectado), mesmo padrão
  de `dashboard_ready_state`

**Checkpoint**: User Story 1 completa e testável de forma independente — MVP.

---

## Phase 4: User Story 2 - Conectar e desconectar sem sair do Farol (Priority: P2)

**Goal**: Usuário inicia/encerra a conexão VPN a partir do próprio widget, com feedback de
"conectando" e tradução legível de qualquer erro de domínio.

**Independent Test**: A partir do widget em "desconectado", disparar conectar a um perfil e
confirmar transição para "conectado"; a partir de "conectado", desconectar e confirmar retorno a
"desconectado" (`quickstart.md` Cenários 3-4).

### Implementation for User Story 2

- [X] T028 [US2] [P] Em `plugins/openfortivpn-vpn/vpn_cli.py`: `connect(profile)` e `disconnect()`
  — invocam `openfortivpn-gui connect <perfil> --json`/`disconnect --json`; sucesso mapeia para
  `VpnStatusItem`; falha mapeia `ErrorPayload.error.code` para `-32009`/`vpn_action_failed` com
  `data.detail = {cli_code, cli_message}` e `message` traduzido (tabela de
  `contracts/openfortivpn-cli-mapping.md`)
- [X] T029 [US2] Em `plugins/openfortivpn-vpn/main.py`: `handle_action_invoke` — dispatch por
  `action_id` (`"vpn.connect"` com `target.type == "vpn-profile"`, `"vpn.disconnect"` com
  `target.type == "vpn-connection"`), delegando a `vpn_cli.connect`/`vpn_cli.disconnect`
- [X] T030 [US2] [P] Em `plugins/openfortivpn-vpn/test_vpn_cli.py`: testes de `connect`/
  `disconnect` — sucesso, e cada um dos seis `error.code` possíveis
  (`profile_not_found`/`already_connected`/`not_connected`/`connect_timeout`/`sudo_denied`/
  `internal_error`) com a mensagem traduzida esperada
- [X] T031 [US2] Em `crates/farol-core/src/update.rs`: tratar a resposta de
  `Message::ActionInvokeRequested`/`Message::Worker` para `vpn.connect`/`vpn.disconnect` —
  sucesso substitui `vpn_widget.status` diretamente pelo `VpnStatusItem` retornado (sem `widget/get`
  extra, mesmo padrão de `git.fetch` → `RepositoryViewModel.repo`); seta/limpa
  `connect_in_flight`/`disconnect_in_flight` ao disparar/receber resposta; erro popula
  `vpn_widget.last_action_error` sem derrubar a conexão (FR-007)
- [X] T032 [US2] Em `crates/farol-core/src/view.rs`: um botão por `VpnProfile` disponível
  (`enabled = connect_action.enabled`) disparando `Message::ActionInvokeRequested` com o
  `connect_action` correspondente (seletor de perfil, FR-008 — nenhum campo de protocolo novo
  necessário, `research.md` D4); botão de desconectar análogo; exibir "conectando"/"desconectando"
  durante `*_in_flight` (D7) e `last_action_error` quando presente
- [X] T033 [US2] [P] Em `crates/farol-core/src/e2e_tests.rs`: cenários usando a fixture de T026 —
  `vpn.connect` bem-sucedido leva o widget a `"connected"` com o perfil correto; um `error.code`
  simulado (ex.: `already_connected`) resulta em mensagem traduzida visível sem derrubar o core
  (mesmo padrão dos cenários T036-T042 da feature 002)

**Checkpoint**: User Stories 1 e 2 funcionam, cada uma de forma independente.

---

## Phase 5: User Story 3 - Não perder o rastro de uma conexão VPN esquecida (Priority: P3)

**Goal**: O widget mostra há quanto tempo a sessão VPN atual está ativa.

**Independent Test**: Com uma conexão VPN ativa há um tempo conhecido, abrir o Farol e confirmar
que o widget mostra a duração decorrida sem nenhuma ação adicional (`quickstart.md` Cenário 5).

### Implementation for User Story 3

- [X] T034 [US3] Em `crates/farol-core/src/view.rs`: exibir `elapsed_seconds` formatado (ex.: "há
  1h 23min") quando `state == Connected` — o campo já é populado desde T021 (US1); esta task só
  adiciona a renderização
- [X] T035 [US3] [P] Em `crates/farol-core/src/visual_snapshot_tests.rs`: snapshot cobrindo a
  exibição do tempo de sessão no estado conectado

**Checkpoint**: as três user stories funcionam, cada uma de forma independente.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Verificação final, regressão (SC-004) e higiene documental.

- [X] T036 [P] Rodar `ruff check` dentro de `plugins/openfortivpn-vpn/` — deve ficar limpo
- [X] T037 [P] Rodar `cargo clippy --workspace --all-targets` — deve ficar limpo, sem warning novo
- [X] T038 Rodar `cargo test --workspace` — confirmar toda a suíte passando, incluindo os cenários
  novos de T011/T012/T026/T027/T033/T035, e que `git-local`/`uptime-kuma` continuam chegando a
  `Ready` sob `"0.3"` (SC-004)
- [X] T039 Estender `tests/integration/harness.sh` (Camada 2) com uma quinta/sexta condição
  confirmando que `openfortivpn-vpn` também chega a `Ready` sob Xvfb, reusando a fixture de T026
- [X] T040 Validar manualmente os 7 cenários de `quickstart.md` (ou confirmar que cada um já tem
  equivalente automatizado registrado nas tasks acima, mesmo padrão da nota de
  `specs/002-uptime-kuma-plugin/quickstart.md`)
- [X] T041 Atualizar `README.md` se o roadmap/status do produto mudar de forma material (regra de
  governance da constitution — `.specify/memory/constitution.md` § Governance)
- [X] T042 Atualizar `AGENTS.md` marcando a feature 004 como completa, com o resumo final (plugin
  `openfortivpn-vpn`, protocolo `"0.3"`, migração de `git-local`/`uptime-kuma`), mesmo padrão das
  features 001-003

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: sem dependências — pode começar imediatamente
- **Foundational (Phase 2)**: depende de Setup — BLOQUEIA todas as user stories
- **User Stories (Phase 3+)**: todas dependem da conclusão de Foundational
  - US1 (P1) pode começar assim que Foundational completar
  - US2 (P2) depende de US1 apenas por ordem de prioridade de entrega — tecnicamente só precisa de
    Foundational (o `VpnStatusItem` já existe desde T008/T021); ainda assim, US2 é mais fácil de
    validar com US1 já funcionando (o widget precisa existir para clicar em algo)
  - US3 (P3) depende só de Foundational + T021 (mapeamento de `elapsed_seconds`, já feito em US1) —
    pode ser implementada em paralelo a US2 se houver capacidade
- **Polish (Phase 6)**: depende de todas as user stories desejadas estarem completas

### Dentro de cada User Story

- Mapeamento Python (`vpn_cli.py`) antes do dispatch (`main.py`)
- `update.rs` (roteamento de estado) antes de `view.rs` (renderização) — a `view` lê o `Model` que
  `update.rs` popula
- Testes de plugin (`test_vpn_cli.py`) podem rodar em paralelo à implementação Rust da mesma story
  (arquivos/linguagens diferentes)

### Parallel Opportunities

- Todas as tasks `[P]` de Setup e Foundational (T001-T002, T006, T008/T011/T012, T014/T015)
- Depois de Foundational completo, US1/US3 têm baixo acoplamento entre si (US3 só depende de um
  campo que US1 já mapeia) — mas US1 deve completar primeiro na prática, pois US3 renderiza dentro
  do mesmo widget que US1 cria
- Tasks de teste marcadas `[P]` dentro de cada story

---

## Parallel Example: Foundational (protocolo)

```bash
Task: "Adicionar VpnConnectionState/VpnProfile/VpnStatusItem em crates/farol-protocol/src/messages.rs (T008)"
Task: "Atualizar handshake.schema.json v0.3 (descrição only) (T006)"
```

## Parallel Example: User Story 1

```bash
Task: "Testes de query_status em plugins/openfortivpn-vpn/test_vpn_cli.py (T023)"
Task: "Fixture fake-openfortivpn-gui + cenário e2e (T026)"
Task: "Snapshot visual do widget vpn-status (T027)"
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Phase 1: Setup
2. Completar Phase 2: Foundational (CRITICAL — bloqueia todas as stories)
3. Completar Phase 3: User Story 1
4. **PARAR e VALIDAR**: testar User Story 1 de forma independente (`quickstart.md` Cenários 1-2)
5. Entregar/demonstrar se pronto — já satisfaz SC-001

### Incremental Delivery

1. Setup + Foundational → protocolo `"0.3"` pronto, plugin chega a `Ready`
2. + User Story 1 → testar independentemente → MVP (visibilidade, "Ver")
3. + User Story 2 → testar independentemente → conectar/desconectar pelo widget ("Agir")
4. + User Story 3 → testar independentemente → duração de sessão visível ("Lembrar")
5. Polish → regressão confirmada (SC-004), documentação atualizada

---

## Notas

- Nenhuma dívida técnica nova antecipada por este `tasks.md` — a única generalização de nome
  identificada (T019, `FetchRequested` → `ActionInvokeRequested`) é resolvida dentro da própria
  Fase Foundational desta feature, não deixada como débito.
- Se, durante a implementação, surgir alguma dívida técnica deliberadamente adiada, registrar como
  issue no tracker do projeto antes de considerar a mudança correspondente concluída (regra de
  governance da constitution).
