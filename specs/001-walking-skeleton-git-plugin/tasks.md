---

description: "Task list template for feature implementation"
---

# Tasks: Walking Skeleton — Core, Protocolo de Plugin e Plugin de Referência Git Local

**Input**: Design documents from `/specs/001-walking-skeleton-git-plugin/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (6 documentos), quickstart.md

**Tests**: Sem TDD explicitamente requisitado pela spec. As tasks de teste geradas aqui vêm de
`plan.md` § Testing (que já descreve `cargo test`/`pytest`/harness de integração como parte do
Technical Context desta feature, não como pedido de "tests first") e do `quickstart.md` (7
cenários de validação manual, mapeados como tasks de verificação ao final de cada user story).

**Organization**: Tasks agrupadas por user story priorizada (P1 → P2 → P3, `spec.md`), para
implementação e teste independentes de cada uma. `protocol/` é pré-requisito explícito tanto do
core quanto do plugin (D1 de `research.md`) e vem antes de qualquer um dos dois.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2, US3) — ausente em Setup,
  Foundational e Polish
- Caminho de arquivo exato em cada descrição

## Path Conventions

Estrutura definida em `plan.md` § Project Structure (aplicação desktop nativa + processo filho de
plugin — não é "single project"/"web app"/"mobile" genérico):

```text
Cargo.toml                        # workspace root
crates/farol-core/src/            # main.rs, model.rs, update.rs, view.rs, plugin_worker.rs
crates/farol-protocol/src/        # framing.rs, version.rs, messages.rs
protocol/SPEC.md                  # fonte da verdade do protocolo (D1)
protocol/schema/v0.1/*.schema.json
plugins/git-local/                # main.py, scan.py, config.py (Python stdlib, D3)
tests/{contract,integration,unit}/
```

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Inicialização do workspace e das duas árvores de código (Rust e Python) — só
estrutura, sem lógica de protocolo nem de negócio.

- [x] T001 Criar `Cargo.toml` de workspace na raiz do repositório, com membros `crates/farol-core` e `crates/farol-protocol`, per `plan.md` § Project Structure
- [x] T002 [P] Inicializar `crates/farol-core/Cargo.toml` (binário) com dependências `iced` ~0.13 (feature `tokio`), `tokio`, `serde`, `serde_json`, per `plan.md` § Technical Context
- [x] T003 [P] Inicializar `crates/farol-protocol/Cargo.toml` (biblioteca) com dependências `serde`, `serde_json`, `thiserror`, per `plan.md` § Technical Context
- [x] T004 [P] Criar esqueleto de `plugins/git-local/` (`main.py`, `scan.py`, `config.py` — stubs, apenas stdlib), per `plan.md` § Project Structure e D3 de `research.md`
- [x] T005 [P] Configurar lint/format do workspace Rust (`rustfmt.toml`, config clippy) na raiz do repositório
- [x] T006 [P] Configurar lint/format de `plugins/git-local` (Python, stdlib only — sem dependência de runtime externa)
- [x] T007 Criar esqueleto de `tests/` (`tests/contract/`, `tests/integration/`, `tests/unit/`) per `plan.md` § Project Structure, documentando o propósito de cada pasta (contrato = validação contra `protocol/schema/`; integração = cenários de `quickstart.md`; unidade = lógica pura)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Infraestrutura que MUST estar completa antes de qualquer user story. Dividida em duas
sub-fases porque a decisão D1 de `research.md` torna a especificação do protocolo pré-requisito
explícito tanto do core (Rust) quanto do plugin (Python) — nenhum dos dois pode começar antes.

**⚠️ CRITICAL**: Nenhuma task de user story pode começar antes desta fase estar completa.

### Phase 2a — Especificação do Protocolo (pré-requisito de core E plugin — D1)

- [x] T008 Escrever `protocol/SPEC.md` — prosa normativa cobrindo transporte stdin/stdout, framing NDJSON (D2 de `research.md`), sequência de handshake, versionamento `MAJOR.MINOR` (D7), correlação de `id`, e modelo de erro; consolida `contracts/framing-and-versioning.md`, `contracts/handshake.md` e `contracts/error-model.md`
- [x] T009 [P] Criar `protocol/schema/v0.1/handshake.schema.json` (JSON Schema Draft 2020-12) para `handshake/hello` request/response, per `contracts/handshake.md` e `data-model.md` §1.2–1.4, §1.6
- [x] T010 [P] Criar `protocol/schema/v0.1/widget.schema.json` para `widget/get` request/response, per `contracts/widget-protocol.md` e `data-model.md` §1.5
- [x] T011 [P] Criar `protocol/schema/v0.1/action.schema.json` para `action/invoke` request/response, per `contracts/action-protocol.md` e `data-model.md` §1.4
- [x] T012 [P] Criar `protocol/schema/v0.1/error.schema.json` para o objeto de erro JSON-RPC e a tabela de `code`/`data.reason`, per `contracts/error-model.md` e `data-model.md` §1.7

**Checkpoint**: `protocol/` completo e normativo (D1). **Nenhuma task de `farol-protocol` (Rust) ou
`git-local` (Python) pode começar antes deste checkpoint.** A partir daqui, core e plugin são
paralelizáveis entre si — linguagens e diretórios distintos, sem arquivo compartilhado.

### Phase 2b — Esqueletos de Core e Plugin (paralelo entre si, após 2a)

- [x] T013 [P] Implementar codec NDJSON em `crates/farol-protocol/src/framing.rs` (D2) — encode/decode de linha JSON compacta única, conforme `protocol/SPEC.md`
- [x] T014 [P] Implementar tipo `ProtocolVersion` e algoritmo de comparação em `crates/farol-protocol/src/version.rs` (D7) — igualdade exata quando `MAJOR == 0`, regra geral para `MAJOR >= 1`
- [x] T015 [P] Implementar tipos de mensagem (`HandshakeHello`/`HandshakeHelloResult`, `WidgetDeclaration`, `ActionDeclaration`, `ActionTarget`, `GitRepository`, `RemoteStatus`, objeto de erro) em `crates/farol-protocol/src/messages.rs`, a partir de `protocol/schema/v0.1/*.schema.json`
- [x] T016 [P] farol-core: esqueleto de app `iced` abrindo exatamente uma janela nativa ao iniciar (FR-001) em `crates/farol-core/src/main.rs`
- [x] T017 [P] farol-core: definir `PluginState`, `PluginConnection`, `UnavailableReason` em `crates/farol-core/src/model.rs`, per `data-model.md` §2.1 e §3 (transições de estado — ainda sem I/O real)
- [x] T018 [P] plugin git-local: loop de leitura/escrita NDJSON sobre stdin/stdout em `plugins/git-local/main.py` (`readline` + JSON compacto), conforme `protocol/SPEC.md` — sem lógica de negócio ainda
- [x] T019 [P] plugin git-local: leitor de configuração em `plugins/git-local/config.py` — lê `$XDG_CONFIG_HOME/farol/plugins/git-local/config.toml` (fallback `~/.config/...`), default `scan_root = ~/dev` quando ausente (FR-012, `contracts/git-local-plugin.md`)

**Checkpoint**: codec de protocolo, janela do core e loop stdio do plugin existem. A implementação
da User Story 1 pode começar.

---

## Phase 3: User Story 1 - Ver o estado dos repositórios git ao abrir o Farol (Priority: P1) 🎯 MVP

**Goal**: Core abre uma janela, sobe o plugin Git, os dois fazem handshake, o plugin declara
identidade/manifesto/widget, varre `scan_root` e devolve os dados; o core renderiza o widget e o
atualiza sozinho em ciclos periódicos.

**Independent Test**: Abrir o Farol com o plugin Git configurado apontando para um diretório de
teste e verificar que o status dos repositórios aparece corretamente na janela, sem qualquer ação
adicional do usuário.

### Implementação para User Story 1

- [x] T020 [US1] farol-core: implementar spawn do processo filho do plugin (`tokio::process::Command`) dentro de uma `iced::Subscription` worker em `crates/farol-core/src/plugin_worker.rs` (D4/D5) — transições `Starting` → `Handshaking`, ou `Unavailable{FailedToStart}` em falha de spawn
- [x] T021 [US1] farol-core: enviar `handshake/hello` pelo canal do worker e tratar a resposta em `crates/farol-core/src/plugin_worker.rs` — aplicar `RPC_TIMEOUT_CONTROL` (5s, D6) e a checagem de compatibilidade de versão (D7) via `farol_protocol::version` (depende de T013, T014, T020)
- [x] T022 [US1] farol-core: propagar o resultado do handshake para `crates/farol-core/src/update.rs` — `PluginState` transiciona para `Ready` (compatível) ou `Unavailable{VersionIncompatible}`/`Unavailable{Unresponsive}` (`data-model.md` §3) (depende de T017, T021)
- [x] T023 [P] [US1] plugin git-local: implementar handler de `handshake/hello` em `plugins/git-local/main.py` — responde `plugin_name: "git-local"`, `protocol_version: "0.1"`, `capabilities: ["exec"]`, `widgets: [repo-status/status-grid]`, `actions: []` (`contracts/handshake.md`, `contracts/git-local-plugin.md`) (depende de T018)
- [x] T024 [P] [US1] plugin git-local: implementar varredura em `plugins/git-local/scan.py` — subdiretórios diretos de `scan_root` contendo `.git`, `dirty` via `git status --porcelain`, `ahead`/`behind` via `git rev-list --left-right --count`, `remote_status: {"kind":"no_remote"}` quando sem remote configurado (FR-013/FR-014, `contracts/git-local-plugin.md`) (depende de T019)
- [x] T025 [US1] plugin git-local: implementar handler de `widget/get` em `plugins/git-local/main.py`, combinando `scan.py` com a `fetch_action` de cada repositório (`enabled: false` quando `no_remote`), conforme `contracts/widget-protocol.md` (depende de T023, T024)
- [x] T026 [US1] farol-core: implementar refresh periódico como `iced::Subscription` (`time::every`, default 30000ms ou `suggested_refresh_interval_ms` do handshake — FR-011) chamando `widget/get` pelo worker; timeout de `RPC_TIMEOUT_CONTROL` no ciclo contribui para `Unresponsive` (D6) em `crates/farol-core/src/plugin_worker.rs` (depende de T021, T025)
- [x] T027 [US1] farol-core: registrar e exibir o manifesto de capacidades declarado pelo plugin (incluindo `exec`), consultável pelo usuário na UI (FR-008) em `crates/farol-core/src/view.rs` (depende de T022)
- [x] T028 [US1] farol-core: renderizar o widget `status-grid` (repositórios com working tree suja/limpa e ahead/behind, ou "sem remoto" visualmente distinguível de "0 ahead / 0 behind") em `crates/farol-core/src/view.rs`, mapeando `GitRepository`/`RemoteStatus` recebidos (FR-009/FR-010/FR-013/FR-014) (depende de T015, T022, T026)
- [x] T029 [US1] farol-core: exibir mensagem legível de incompatibilidade de versão quando o handshake falha por versão (FR-005), sem renderizar nenhum widget desse plugin (depende de T022)

### Validação da User Story 1 (cenários de `quickstart.md`)

- [x] T030 [US1] Executar Cenário 1 de `quickstart.md` — abrir o Farol com diretório de teste populado; confirmar janela única, widget populado (working tree + ahead/behind, ou "sem remoto" distinguível), e atualização automática após ~30s sem reiniciar (FR-001, FR-011, SC-001, SC-002)
- [ ] T031 [US1] Executar Cenário 2 de `quickstart.md` — simular `protocol_version: "9.9"` na resposta de handshake do plugin; confirmar que nenhum widget é renderizado, mensagem legível de incompatibilidade aparece, e o core não trava nem cai (FR-005, SC-005) (pendente — não executado ainda)

**Checkpoint**: User Story 1 completa e testável de forma independente — MVP.

---

## Phase 4: User Story 2 - Disparar `git fetch` a partir da UI do Farol (Priority: P2)

**Goal**: Com o widget de US1 já visível, o usuário aciona a ação Fetch de um repositório pela UI;
o core invoca a ação no plugin, o plugin executa `git fetch` e devolve sucesso/erro, e o core
reflete o novo estado no mesmo widget.

**Independent Test**: Disparar a ação de fetch num repositório já exibido pelo widget e verificar
que o `git fetch` roda contra o repositório certo e que o ahead/behind exibido reflete o resultado.

**Depende de**: User Story 1 completa (não há onde disparar a ação sem o widget e o repositório
visíveis).

### Implementação para User Story 2

- [x] T032 [US2] farol-core: exibir a ação "Fetch" por repositório em `crates/farol-core/src/view.rs`, no estado (habilitado/desabilitado) exatamente como declarado pelo plugin (FR-015) — o core nunca decide isso por conta própria (depende de T028)
- [x] T033 [US2] farol-core: implementar envio de `action/invoke` pelo worker ao acionar Fetch, com `RPC_TIMEOUT_ACTION` (120s default, ou `timeout_hint_ms` da `ActionDeclaration` quando presente) em `crates/farol-core/src/plugin_worker.rs` (FR-016, D6) (depende de T020, T032)
- [x] T034 [P] [US2] plugin git-local: implementar `action/invoke` para `git.fetch` em `plugins/git-local/main.py`/`scan.py` — executa `git fetch` via subprocess no repositório-alvo, devolve `GitRepository` pós-fetch em sucesso, ou erro `-32001 fetch_failed` sem encerrar o processo do plugin (FR-017, `contracts/action-protocol.md`, `contracts/git-local-plugin.md`) (depende de T024)
- [x] T035 [US2] farol-core: fundir o resultado de `action/invoke` no `RepositoryViewModel` do repositório-alvo em `crates/farol-core/src/update.rs` — sucesso atualiza ahead/behind, erro popula `last_error` (FR-018, `data-model.md` §2.2) (depende de T033, T034)
- [x] T036 [US2] farol-core: refletir `fetch_in_flight`/erro por repositório em `crates/farol-core/src/view.rs` sem travar a janela (depende de T035)

### Validação da User Story 2 (cenário de `quickstart.md`)

- [ ] T037 [US2] Executar Cenário 3 de `quickstart.md` — disparar fetch num repositório com remoto (ahead/behind atualizado ao final), confirmar ação desabilitada, não omitida, para repositório sem remoto, e simular falha de rede confirmando erro estruturado exibido sem travar a janela nem derrubar o core (FR-015–FR-018, SC-003) (pendente — não executado ainda)

**Checkpoint**: User Stories 1 e 2 funcionam, cada uma de forma independente.

---

## Phase 5: User Story 3 - Farol continua funcionando quando o plugin trava ou morre (Priority: P3)

**Goal**: Se o processo do plugin travar ou morrer, o core detecta a condição, sinaliza o plugin
como indisponível na UI, e o restante da aplicação continua respondendo normalmente.

**Independent Test**: Matar o processo do plugin (ou simular trava) enquanto o Farol está aberto e
verificar que a janela permanece responsiva, o widget passa a indicar "indisponível", e nenhuma
outra funcionalidade do core é afetada.

**Depende de**: User Story 1 (precisa haver um plugin rodando para poder falhar).

### Implementação para User Story 3

- [x] T038 [US3] farol-core: observar `child.wait()` concorrentemente à leitura de stdout/recebimento de pedidos (via `tokio::select!`) no worker, emitindo `Unavailable{Crashed}` imediatamente quando o processo termina, sem depender de nenhuma requisição em voo (FR-019, D6) em `crates/farol-core/src/plugin_worker.rs` (depende de T020)
- [x] T039 [US3] farol-core: garantir que `Unavailable{Crashed}` e `Unavailable{Unresponsive}` (do timeout de refresh, T026) convergem para o mesmo estado visível de "indisponível" na UI, distinguível de "carregando"/"sem dados" (FR-020) em `crates/farol-core/src/view.rs` (depende de T026, T038)
- [x] T040 [US3] farol-core: garantir que o restante da janela (demais elementos e interação do usuário) continua respondendo quando `PluginState = Unavailable`, sem bloquear `update`/`view` (FR-021) (depende de T039)

### Validação da User Story 3 (cenários de `quickstart.md`)

- [x] T041 [US3] Executar Cenário 4 de `quickstart.md` — `kill -9` no processo do plugin; confirmar que a janela do Farol permanece aberta e responsiva, e o widget passa a exibir "indisponível" distinguível de carregando (FR-019–FR-021, SC-004)
- [x] T042 [US3] Executar Cenário 5 de `quickstart.md` — `kill -STOP` no processo do plugin (travamento); confirmar que o core não bloqueia indefinidamente e sinaliza `Unavailable{Unresponsive}` após `RPC_TIMEOUT_CONTROL` no próximo ciclo de refresh, sem impedir o resto da janela de responder; `kill -CONT` ao final do teste (FR-019, D6)

**Checkpoint**: as três user stories funcionam, cada uma de forma independente.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Edge cases do `spec.md` não amarrados a uma única user story, os 2 cenários
remanescentes de `quickstart.md`, e a suíte de testes automatizados descrita em `plan.md` §
Testing.

- [x] T043 [P] farol-core: sinalizar `Unavailable{FailedToStart}` quando o spawn do processo do plugin falha (binário ausente ou não executável), sem widget renderizado para esse plugin (Edge Case da spec) em `crates/farol-core/src/plugin_worker.rs`
- [ ] T044 Executar Cenário 6 de `quickstart.md` — apontar o core para um caminho de plugin inexistente; confirmar `Unavailable{FailedToStart}` desde o início, sem crash do core e sem widget renderizado (Edge Case da spec) (pendente — não executado ainda)
- [x] T045 [P] plugin git-local: reportar `-32003 exec_unavailable` quando o binário `git` está ausente do sistema (em `widget/get` ou `action/invoke`), sem encerrar o processo do plugin (Edge Case da spec, `contracts/error-model.md`) em `plugins/git-local/scan.py`
- [ ] T046 Executar Cenário 7 de `quickstart.md` — remover `git` do `PATH` visível ao plugin; confirmar erro `exec_unavailable` reportado nas operações afetadas, sem que o processo do plugin morra e sem que `PluginState` saia de `Ready` (pendente — não executado ainda)
- [ ] T047 [P] farol-protocol: testes de contrato (`cargo test`) — codec NDJSON, comparação de versão (D7), (de)serialização de cada forma de mensagem contra `protocol/schema/v0.1/*.schema.json`, em `tests/contract/` (pendente — não executado ainda)
- [x] T048 [P] farol-core: testes de unidade da máquina de estados `PluginState` (`data-model.md` §3) em `tests/unit/`
- [x] T049 [P] plugin git-local: testes pytest — varredura de `scan_root`, mapeamento de `no_remote`, execução de `git fetch` mockada, em `tests/unit/test_git_local_scan.py`

---

## Dependencies & Execution Order

### Dependências entre fases

- **Setup (Fase 1)**: sem dependências — pode começar imediatamente.
- **Foundational 2a — Protocolo (D1)**: depende da Fase 1. **Bloqueia** 2b, e transitivamente todo
  o resto — nenhuma linha de código de `farol-protocol`, `farol-core` ou `plugins/git-local` é
  escrita antes de `protocol/` existir.
- **Foundational 2b — Esqueletos**: depende de 2a. A partir daqui, **core (Rust) e plugin
  (Python) são paralelizáveis** — diretórios e linguagens distintas, nenhum arquivo compartilhado
  (T013–T015 em `crates/farol-protocol`, T016–T017 em `crates/farol-core`, T018–T019 em
  `plugins/git-local`).
- **User Story 1 (Fase 3)**: depende da Fase 2 completa. Nenhuma dependência de outra user story.
- **User Story 2 (Fase 4)**: depende da User Story 1 completa (a ação de fetch precisa do widget e
  do repositório já exibidos — não é uma dependência técnica de arquivo, é uma dependência de
  produto explícita da spec).
- **User Story 3 (Fase 5)**: depende da User Story 1 completa (precisa haver um plugin rodando
  para poder falhar); independente de US2.
- **Polish (Fase 6)**: depende de todas as user stories desejadas estarem completas.

### Dentro de cada user story

- US1: handshake (T020–T023) antes de varredura/widget (T024–T026); manifesto e renderização
  (T027–T029) dependem do handshake concluído; validação (T030–T031) depende de toda a
  implementação da story.
- US2: exposição da ação (T032) depende da renderização de US1 (T028); invocação (T033) depende do
  worker de US1 (T020) e da ação exposta (T032); handler do plugin (T034) depende da varredura de
  US1 (T024); fusão de resultado (T035) depende de T033+T034; validação (T037) depende de tudo
  acima.
- US3: observação de crash (T038) depende do worker de US1 (T020); convergência de estado (T039)
  depende do refresh de US1 (T026) e de T038; responsividade (T040) depende de T039; validação
  (T041–T042) depende de tudo acima.

### Oportunidades de paralelismo

- Todas as tasks `[P]` da Fase 1 podem rodar em paralelo entre si.
- T009–T012 (schemas JSON, Fase 2a) podem rodar em paralelo entre si, mas só depois de T008.
- **Depois que 2a termina**, T013–T015 (`farol-protocol`), T016–T017 (`farol-core`) e T018–T019
  (`plugins/git-local`) podem todos rodar em paralelo — três frentes sem arquivo compartilhado.
- Dentro de US1: T023–T024 (plugin) podem rodar em paralelo com T020–T022 (core), desde que T025
  (que depende de ambos os lados) só comece depois de T021/T023/T024 concluídos.
- Dentro de US2: T034 (plugin) pode rodar em paralelo com T032–T033 (core).
- Fase 6: T043/T045 (código) e T047/T048/T049 (testes) são paralelizáveis entre si; as execuções
  de cenário (T044, T046) são sequenciais aos seus pré-requisitos de código.

---

## Parallel Example: Foundational 2b (após protocolo pronto)

```bash
# Três frentes paralelas, sem arquivo compartilhado, todas dependendo só de T008-T012:
Task: "Implementar codec NDJSON em crates/farol-protocol/src/framing.rs (T013)"
Task: "Esqueleto de app iced abrindo uma janela em crates/farol-core/src/main.rs (T016)"
Task: "Loop de leitura/escrita NDJSON em plugins/git-local/main.py (T018)"
```

## Parallel Example: User Story 1

```bash
# Lado core e lado plugin do handshake, em paralelo:
Task: "Spawn do processo filho do plugin em crates/farol-core/src/plugin_worker.rs (T020)"
Task: "Handler de handshake/hello em plugins/git-local/main.py (T023)"
Task: "Varredura de scan_root em plugins/git-local/scan.py (T024)"
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Fase 1: Setup.
2. Completar Fase 2 (2a protocolo, depois 2b esqueletos) — bloqueante, sem atalho.
3. Completar Fase 3: User Story 1.
4. **PARAR e VALIDAR**: rodar Cenários 1 e 2 de `quickstart.md` (T030–T031) isoladamente.
5. Esse é o walking skeleton mínimo que prova handshake + widget declarativo + renderização
   (Princípios II, III e IV da constitution).

### Entrega Incremental

1. Setup + Foundational (2a → 2b) → protocolo e esqueletos prontos.
2. User Story 1 → validar com Cenários 1–2 → **MVP**.
3. User Story 2 → validar com Cenário 3 → round-trip de ação provado.
4. User Story 3 → validar com Cenários 4–5 → isolamento de falha provado.
5. Polish → Edge Cases restantes (Cenários 6–7) + suíte de testes automatizados.
6. Cada story soma valor sem quebrar a anterior — critério de "walking skeleton provado"
   (`quickstart.md` § final) é os 7 cenários passando.

### Estratégia de Equipe Paralela

Depois que a Fase 2 (protocolo + esqueletos) está pronta:

- Uma frente pode seguir em `crates/farol-core` (Rust) enquanto outra segue em
  `plugins/git-local` (Python) — sem colisão de arquivo, unidas apenas pelo contrato em
  `protocol/`.
- User Story 2 e User Story 3 dependem ambas de User Story 1 completa, mas não dependem uma da
  outra — podem ser trabalhadas em paralelo por pessoas diferentes assim que US1 fechar.

---

## Notes

- `[P]` = arquivos diferentes, sem dependência pendente.
- `[Story]` mapeia a task à user story correspondente para rastreabilidade.
- Nenhuma task desta lista cobre itens do `## Out of Scope` de `spec.md` (sandbox/enforcement,
  workspaces, paleta de comandos, registry, outros plugins, empacotamento, restart automático de
  plugin) — de propósito.
- `protocol/` (Fase 2a) é bloqueante para tudo o mais, por decisão D1 de `research.md`: nenhum
  binding Rust nem plugin Python nasce antes da especificação agnóstica de linguagem existir.
- Verificar que cada cenário de `quickstart.md` passa antes de considerar a story correspondente
  encerrada.
- Parar em qualquer checkpoint para validar a story isoladamente antes de seguir para a próxima.
