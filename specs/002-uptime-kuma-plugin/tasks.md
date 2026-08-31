---

description: "Task list template for feature implementation"
---

# Tasks: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Input**: Design documents from `/specs/002-uptime-kuma-plugin/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (5 documentos), quickstart.md

**Tests**: Sem TDD explicitamente requisitado pela spec. As tasks de teste geradas aqui vêm de
`plan.md` § Testing (`cargo test`/`pytest` já descritos como parte do Technical Context, não como
pedido de "tests first") e do `quickstart.md` (9 cenários de validação manual, mapeados como tasks
de verificação ao final da user story correspondente, ou da fase Foundational/Polish quando o
cenário não é específico de uma story).

**Organization**: Tasks agrupadas por user story priorizada (P1 → P2, `spec.md` — esta feature não
define P3). A evolução do protocolo (D1 de `research.md` — `CapabilityManifest` estruturado por
`kind`, bump de `protocol_version` para `"0.2"`) é pré-requisito bloqueante explícito, própria
subfase da Fase Foundational, análoga a como `protocol/` bloqueou tudo na feature 001. A migração
do plugin `git-local` (feature 001) para o novo formato de capacidades **NÃO** é tarefa deste
`tasks.md` — é débito técnico rastreado em issue própria no tracker do projeto (GitHub Issues),
separada desta feature (constitution v0.3.0, Governance, "Dívida técnica rastreável"; `plan.md` §
Constitution Check / § Complexity Tracking). Nenhuma task abaixo toca `plugins/git-local/`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2) — ausente em Setup, Foundational e
  Polish
- Caminho de arquivo exato em cada descrição

## Path Conventions

Estrutura definida em `plan.md` § Project Structure — mesma estrutura de workspace da feature 001
(`crates/farol-core`, `crates/farol-protocol`, `protocol/`), sem crate novo. `crates/farol-core`
já existe (feature 001) — esta feature não recria seu esqueleto, só estende `model.rs`/`update.rs`/
`view.rs`; `plugins/uptime-kuma/` é novo, sibling de `plugins/git-local/` (inalterado):

```text
protocol/SPEC.md                   # título/versão passam a descrever "0.2"
protocol/schema/v0.1/              # RETIDO como registro histórico — não editado nesta feature
protocol/schema/v0.2/              # NOVO — Capability estruturada, MonitorStatusItem, novos reasons
crates/farol-core/src/             # model.rs, update.rs, view.rs (estendidos); main.rs/plugin_worker.rs inalterados
crates/farol-protocol/src/         # messages.rs (Capability, MonitorStatusItem), version.rs (bump)
plugins/uptime-kuma/               # main.py, config.py, secrets.py, metrics_client.py, metrics_parser.py, poller.py
tests/{contract,integration,unit}/
```

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Pré-condição de ambiente e esqueleto de arquivos do novo plugin — sem lógica de
protocolo nem de negócio ainda.

- [ ] T001 Registrar/verificar pré-requisito de ambiente: CLI `op` (1Password CLI) **instalado e autenticado** (sessão ativa) no ambiente onde `farol-core` e o processo filho do plugin `uptime-kuma` rodam — validar com `op whoami` antes de iniciar qualquer task desta feature que dependa dele (T014 em diante); análogo a como a feature 001 assumiu `git` no `PATH` para `git-local`; o Farol/plugin **NÃO** instala nem gerencia o `op` (D8 de `research.md`, § Pré-requisitos de `quickstart.md`) — ausência bloqueia os Cenários 1, 3 e 7 de `quickstart.md`, mas não bloqueia o trabalho de protocolo (Fase 2a)
- [ ] T002 [P] Criar esqueleto de `plugins/uptime-kuma/` (`main.py`, `config.py`, `secrets.py`, `metrics_client.py`, `metrics_parser.py`, `poller.py` — stubs, apenas stdlib), per `plan.md` § Project Structure e D7 de `research.md`
- [ ] T003 [P] Configurar lint/format de `plugins/uptime-kuma` (Python, stdlib only — sem dependência de runtime externa), mesmo padrão já usado em `plugins/git-local`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: A evolução do `CapabilityManifest` (D1 de `research.md` — capacidades estruturadas por
`kind`, bump de `protocol_version` para `"0.2"`) é a decisão técnica central desta feature e
**pré-requisito bloqueante de tudo o mais**, no mesmo padrão de `protocol/` na feature 001. Dividida
em três subfases: 2a (especificação normativa do protocolo, bloqueia 2b e 2c), 2b (`farol-protocol`,
Rust) e 2c (esqueleto do plugin `uptime-kuma`, Python) — paralelizáveis entre si depois de 2a.

**⚠️ CRITICAL**: Nenhuma task de `farol-protocol`, `farol-core` (extensões desta feature) ou do
plugin `uptime-kuma` pode começar antes de 2a estar completa.

### Phase 2a — Evolução do Protocolo para `"0.2"` (bloqueante de tudo o mais — D1)

- [ ] T004 Atualizar `protocol/SPEC.md` — título/versão corrente passam a descrever `"0.2"`; §6.3 ganha a nova forma estruturada de `CapabilityManifest`/`Capability` (discriminada por `kind`: `exec`/`network`/`secret`, per `research.md` D1); §8.2/§10 ganham a tabela dos novos `reason`s (`-32005`..`-32007`, D9); `protocol/schema/v0.1/` permanece **intocado**, retido como registro histórico do formato que `git-local` (inalterado) ainda fala, per `contracts/framing-and-versioning-delta.md`
- [ ] T005 [P] Criar `protocol/schema/v0.2/handshake.schema.json` — `Capability` discriminada por `kind` (`exec` sem campos extras; `network` com `host` obrigatório + `port` opcional; `secret` com `reference` obrigatório), `CapabilityManifest.capabilities: Capability[]`, per `research.md` D1 (schema JSON ilustrativo `allOf`/`if`/`then`) e `contracts/handshake-delta.md`
- [ ] T006 [P] Criar `protocol/schema/v0.2/widget.schema.json` — novo item `MonitorStatusItem` (`name`, `status: "up"|"down"|"pending"|"maintenance"`, `response_time_ms: integer|null`) e novo valor de `kind` de widget `"monitor-status-grid"`; `WidgetItem`/`kind: "status-grid"` existentes permanecem inalterados no mesmo arquivo, per `research.md` D4 e `data-model.md` §1.3–§1.5
- [ ] T007 [P] Criar `protocol/schema/v0.2/action.schema.json` — forma inalterada em relação a `v0.1/action.schema.json` (copiado/referenciado; ainda usado pelo `git-local` futuro migrado, que continua tendo ação de fetch), per `plan.md` § Project Structure
- [ ] T008 [P] Criar `protocol/schema/v0.2/error.schema.json` — forma do `ErrorObject` inalterada; descrição/catálogo textual ganha os três novos `reason`s (`not_configured` `-32005`, `metrics_unreachable` `-32006`, `metrics_parse_error` `-32007`) e reafirma o reuso de `exec_unavailable` (`-32003`)/`protocol_version_incompatible` (`-32000`), per `research.md` D9 e `contracts/error-model-delta.md`

> **Nota (débito técnico, não é task desta feature)**: um core em `"0.2"` recusa de forma limpa o
> plugin `git-local` (feature 001), inalterado, via `Unavailable{VersionIncompatible}` — consequência
> deliberada de D1. A migração de `plugins/git-local/` para `protocol_version = "0.2"` + o novo
> formato de `capabilities` é rastreada em issue própria no tracker do projeto (GitHub Issues),
> separada desta feature 002 — não criada nem numerada por este `tasks.md` (constitution v0.3.0,
> Governance, "Dívida técnica rastreável"; `plan.md` § Constitution Check / § Complexity Tracking).
> Antes de a feature 002 ser considerada encerrada, essa issue MUST existir.

**Checkpoint 2a**: `protocol/schema/v0.2/` completo e normativo. **Nenhuma task de `farol-protocol`
(Rust) ou `plugins/uptime-kuma` (Python) pode começar antes deste checkpoint.**

### Phase 2b — `farol-protocol` (Rust): bindings do novo schema

- [ ] T009 [P] Evoluir `CapabilityManifest` em `crates/farol-protocol/src/messages.rs` — enum `Capability` (`#[serde(tag = "kind", rename_all = "snake_case")]`: `Exec`, `Network { host, port: Option<u16> }`, `Secret { reference }`), `CapabilityManifest.capabilities: Vec<Capability>` (substitui `Vec<String>`), per `research.md` D1 (binding Rust ilustrativo) e `data-model.md` §1.1–§1.2; tratamento de `kind` desconhecido (forward-compat) é decisão de implementação a resolver aqui, per nota de `research.md` D1 (depende de T005)
- [ ] T010 Adicionar `MonitorStatusItem` e o novo valor de `kind` de widget `"monitor-status-grid"` ao vocabulário de `crates/farol-protocol/src/messages.rs`, per `research.md` D4 e `data-model.md` §1.3–§1.5 (mesmo arquivo de T009 — sequencial, depende de T006, T009)
- [ ] T011 [P] Atualizar `crates/farol-protocol/src/version.rs` — valor de `protocol_version` suportado passa a `"0.2"` (lógica de comparação por igualdade exata sob `MAJOR == 0` inalterada, reafirma D7 da feature 001), per `contracts/framing-and-versioning-delta.md` (depende de T004)

### Phase 2c — Esqueleto do plugin `uptime-kuma` (Python) — paralelo a 2b

- [ ] T012 [P] Loop de leitura/escrita NDJSON sobre stdin/stdout em `plugins/uptime-kuma/main.py` (mesmo padrão de `plugins/git-local/main.py`, D7 reafirma D3 da feature 001) — sem lógica de negócio ainda (depende de T002)
- [ ] T013 [P] Leitor de configuração TOML em `plugins/uptime-kuma/config.py` — lê `$XDG_CONFIG_HOME/farol/plugins/uptime-kuma/config.toml` (fallback `~/.config/...`), campo `base_url`, **sem** default seguro (diferente do `scan_root` de `git-local`) — ausência/vazio é estado a tratar, não a presumir (FR-007/FR-008, `contracts/uptime-kuma-plugin.md`) (depende de T002)
- [ ] T014 [P] Resolução de credencial via `op read` em `plugins/uptime-kuma/secrets.py` — `subprocess.run(["op", "read", "op://Dev/UptimeKuma/API Keys/farol"], ...)`, distinguindo binário `op` ausente do `PATH` (`FileNotFoundError` → sinal para `exec_unavailable`) de `op` presente mas retornando erro (código de saída não-zero → sinal para `not_configured`), per D8 de `research.md` e `contracts/uptime-kuma-plugin.md` § Credencial (depende de T001, T002)

**Checkpoint 2b/2c**: `farol-protocol` fala `"0.2"` com `Capability`/`MonitorStatusItem`; esqueleto
de `plugins/uptime-kuma` existe. A implementação da User Story 1 pode começar.

### Validação da Fase Foundational (cenário de `quickstart.md` — consequência do bump de versão)

- [ ] T015 Executar Cenário 9 de `quickstart.md` — rodar `farol-core` (recompilado com `farol-protocol` em `"0.2"`, T009–T011) com o plugin `git-local` da feature 001 **inalterado** configurado; confirmar `PluginState = Unavailable{VersionIncompatible}`, mensagem legível citando `"0.1"` (plugin) vs. `"0.2"` (core), **sem crash do core**, widget de repositórios git ausente — confirma a consequência deliberada de D1 antes de investir em US1/US2 (não depende de nenhuma task do plugin `uptime-kuma`, só de T004, T009–T011) (depende de T004, T009, T010, T011)

---

## Phase 3: User Story 1 - Ver o estado dos monitores Uptime Kuma ao abrir o Farol (Priority: P1) 🎯 MVP

**Goal**: Core inicia o plugin, os dois fazem handshake em `"0.2"`, o plugin declara identidade,
manifesto de capacidades (exec sempre; network/secret quando configurado) e o widget
`monitor-status-grid`; o plugin lê `/metrics` periodicamente via thread de polling em background,
mantém cache em memória, e responde a `widget/get` sempre a partir do cache (nunca I/O de rede
síncrono, FR-010); o core renderiza a lista de monitores e a mantém atualizada sozinha. Ausência de
`base_url` ou de credencial resolvível produz um estado explícito de "não configurado", nunca uma
lista vazia silenciosa.

**Independent Test**: Apontar o plugin, via seu arquivo de configuração, para uma instância Uptime
Kuma acessível com pelo menos um monitor cadastrado, e verificar que a lista de monitores aparece
corretamente na janela do Farol sem qualquer ação adicional do usuário.

### Implementação para User Story 1

- [ ] T016 [P] [US1] Handler de `handshake/hello` em `plugins/uptime-kuma/main.py` — responde `plugin_name: "uptime-kuma"`, `protocol_version: "0.2"`, `capabilities.capabilities` (`{"kind":"exec"}` sempre; `+{"kind":"network","host":...,"port":...}` e `+{"kind":"secret","reference":"op://Dev/UptimeKuma/API Keys/farol"}` só quando `base_url`/credencial resolvidos — nunca placeholder vazio), `widgets: [{"id":"uptime-kuma-monitors","kind":"monitor-status-grid","title":"Uptime Kuma","suggested_refresh_interval_ms":30000}]`, `actions: []` sempre (FR-002–FR-006, `contracts/handshake-delta.md`) (depende de T012, T013, T014)
- [ ] T017 [P] [US1] Cliente HTTP com Basic Auth em `plugins/uptime-kuma/metrics_client.py` — `urllib.request` contra `${base_url}/metrics`, header `Authorization: Basic base64(":"+api_key)`, timeout de 10s (D5/D7 de `research.md`, `contracts/uptime-kuma-plugin.md` § Autenticação HTTP) (depende de T013, T014)
- [ ] T018 [P] [US1] Parser Prometheus mínimo em `plugins/uptime-kuma/metrics_parser.py` — reconhece só `monitor_status{monitor_name="...",...}` e `monitor_response_time{monitor_name="...",...}`, mapeia `monitor_status` para `status` (`1→up`, `0→down`, `2→pending`, `3→maintenance`, FR-012), `metrics_parse_error` quando nenhuma linha `monitor_status{...}` é encontrada ou algum valor está fora de `{0,1,2,3}` (falha da resposta inteira daquela tentativa, não item a item), linhas malformadas isoladas são puladas (D7 de `research.md`, `contracts/uptime-kuma-plugin.md` § Parsing)
- [ ] T019 [US1] Thread de polling em background + cache (`last_success`/`last_error`, `threading.Lock`) em `plugins/uptime-kuma/poller.py` — laço estritamente sequencial a cada `suggested_refresh_interval_ms` (30000 default), chama `metrics_client`/`metrics_parser`, nunca duas chamadas HTTP concorrentes por construção, estado inicial pré-populado com `last_error = "aguardando primeira leitura"` (D6 de `research.md`, resolve FR-010, `data-model.md` §2.3–§2.4) (depende de T017, T018)
- [ ] T020 [US1] Handler de `widget/get` em `plugins/uptime-kuma/main.py` — lê exclusivamente o cache do poller sob o mesmo lock (nunca I/O de rede síncrono); lógica: `not_configured` (`-32005`) se config/credencial ausente; senão `last_error` (`-32006`/`-32007`) se `last_success is None` ou `last_error.at >= last_success.at`; senão sucesso com `items = last_success.monitors` (D6/D9 de `research.md`, `data-model.md` §2.3, `contracts/widget-protocol-delta.md`) (mesmo arquivo de T016 — sequencial, depende de T016, T019)
- [ ] T021 [P] [US1] Adicionar `MonitorWidgetViewModel` (`monitors: MonitorStatusItem[]`, `last_error: Option<PluginError>`) em `crates/farol-core/src/model.rs`, per `data-model.md` §3.1 (depende de T010)
- [ ] T022 [US1] Popular `MonitorWidgetViewModel` a partir do resultado de `widget/get` do widget `uptime-kuma-monitors` em `crates/farol-core/src/update.rs` — sucesso atualiza `monitors`; qualquer erro pontual (`not_configured`/`metrics_unreachable`/`metrics_parse_error`) atualiza só `last_error`, preservando `monitors` anterior, sem alterar `PluginState` (FR-017, mesmo mecanismo genérico `protocol/SPEC.md` §5.2 já usado para `git-local`) (depende de T021, T010)
- [ ] T023 [US1] Renderizar o `kind: "monitor-status-grid"` em `crates/farol-core/src/view.rs` — lista de monitores (nome, status `up`/`down`/`pending`/`maintenance`, tempo de resposta quando aplicável) mapeando `MonitorStatusItem`; qualquer `last_error` presente (incluindo `not_configured`) MUST renderizar um estado explícito, visivelmente distinto de "0 monitores" (FR-008, FR-013, FR-014, SC-001, SC-005) (depende de T022, T010)
- [ ] T024 [US1] Exibir as capacidades `network` (host/port) e `secret` (reference) do manifesto deste plugin na UI, mesmo padrão apenas declarativo já usado para `exec` (FR-005/FR-006) em `crates/farol-core/src/view.rs` (mesmo arquivo de T023 — sequencial, depende de T009, T023)

### Validação da User Story 1 (cenários de `quickstart.md`)

- [ ] T025 [US1] Executar Cenário 1 de `quickstart.md` — instância Uptime Kuma configurada e acessível com ao menos um monitor; confirmar widget populado (nome/status/tempo de resposta), atualização automática após ~30s sem reiniciar o Farol, e manifesto de capacidades exibindo `network`/`secret` (FR-003–FR-006, SC-001, SC-002)
- [ ] T026 [US1] Executar Cenário 2 de `quickstart.md` — remover `~/.config/farol/plugins/uptime-kuma/config.toml`; confirmar estado explícito "não configurado" (`error(-32005, not_configured)`) distinguível de "0 monitores", sem crash do plugin (FR-008, SC-005)
- [ ] T027 [US1] Executar Cenário 3 de `quickstart.md` — invalidar a resolução da credencial (renomear/mover o item no cofre 1Password, ou `op signout`); confirmar o mesmo estado `not_configured` do Cenário 2 (FR-019 — mesmo tratamento explícito exigido de FR-008)
- [ ] T028 [US1] Executar Cenário 4 de `quickstart.md` — rodar `farol-core` com um `PATH` que não inclua `op`; confirmar `error(-32003, exec_unavailable)`, **distinto** de `not_configured`, sinalizando ambiente quebrado (ferramenta ausente) em vez de credencial não provisionada, sem crash do plugin (D8/D9 de `research.md`)
- [ ] T029 [US1] Executar Cenário 7 de `quickstart.md` — apontar `base_url` para uma instância Uptime Kuma real, acessível, sem nenhum monitor cadastrado; confirmar `widget/get` com sucesso e `items: []` como estado válido, distinto de qualquer um dos erros acima (Edge Case do spec, análogo a diretório sem repositórios git da feature 001)

**Checkpoint**: User Story 1 completa e testável de forma independente — MVP.

---

## Phase 4: User Story 2 - Farol permanece utilizável quando o Uptime Kuma está inacessível ou responde de forma inválida (Priority: P2)

**Goal**: Provar, sob condição adversa, o mesmo mecanismo já construído em US1/Foundational: falha
de rede ou resposta não parseável ao consultar `/metrics` é erro pontual daquela leitura — a thread
de polling (T019) já grava isso em `last_error` sem apagar `last_success`, e o core (T022, mecanismo
genérico `protocol/SPEC.md` §5.2, inalterado desde a feature 001) já preserva os últimos dados
conhecidos e sinaliza o erro sem mudar `PluginState`. **Esta story não introduz nenhuma
implementação nova** — FR-015/FR-016/FR-017 descrevem explicitamente o reuso do mecanismo de erro
pontual já existente, e FR-018 (isolamento de crash/trava do processo) é herdado sem reespecificação
da feature 001. As tasks abaixo são exclusivamente de **validação** do que US1/Foundational já
constroem, sob um ambiente de rede degradado.

**Independent Test**: Apontar o plugin para uma URL inacessível (host errado ou porta fechada) e
verificar que (a) o Farol continua respondendo normalmente, (b) o widget sinaliza o erro de leitura
de forma distinguível de "0 monitores" e de um estado de "plugin indisponível", e (c) corrigir a
URL/tornar o host acessível novamente faz o widget voltar a exibir os monitores reais no ciclo de
refresh seguinte, sem reiniciar o Farol.

**Depende de**: User Story 1 completa (não há o que exibir sem um widget e uma leitura periódica
funcionando no caminho feliz).

### Validação da User Story 2 (cenários de `quickstart.md`)

- [ ] T030 [US2] Executar Cenário 5 de `quickstart.md` — com o Farol já rodando e o widget populado (Cenário 1/T025), tornar a instância Uptime Kuma inacessível (desconectar rede, apontar para host/porta fechada, ou parar o serviço) sem reiniciar o Farol; confirmar que (a) a janela permanece aberta e responsiva, (b) no próximo ciclo de refresh o widget sinaliza `metrics_unreachable` (`-32006`) mantendo os últimos monitores conhecidos, (c) restaurar o acesso faz os dados reais voltarem no próximo ciclo, sem intervenção manual (FR-015, FR-017, SC-003, SC-004)
- [ ] T031 [US2] Executar Cenário 6 de `quickstart.md` — apontar temporariamente `base_url` para um servidor HTTP que não seja Uptime Kuma (resposta não reconhecível como `/metrics` Prometheus válido); confirmar `error(-32007, metrics_parse_error)` no próximo ciclo de refresh, mesmo tratamento de erro pontual do Cenário 5, sem crash do plugin (FR-016)

**Checkpoint**: as duas user stories funcionam, cada uma de forma independente.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Comportamentos genéricos herdados sem reespecificação (isolamento de falha de
processo, FR-018), e a suíte de testes automatizados descrita em `plan.md` § Testing. Nenhuma task
desta fase é específica de uma user story.

- [ ] T032 [P] Executar Cenário 8 de `quickstart.md` — `kill -9`/`kill -STOP` no processo `plugins/uptime-kuma`; confirmar comportamento idêntico ao já provado pela feature 001 (`Unavailable{Crashed}`/`Unavailable{Unresponsive}`), janela do Farol permanece aberta e responsiva — **nenhuma implementação nova**, só confirmação de que o mecanismo genérico do core (D6 da feature 001, inalterado) se aplica também a este plugin (FR-018)
- [ ] T033 [P] `farol-protocol`: testes de contrato (`cargo test`) para `Capability` (serialização/deserialização de `exec`/`network`/`secret` contra `protocol/schema/v0.2/handshake.schema.json`) e para `MonitorStatusItem`/`kind: "monitor-status-grid"` contra `protocol/schema/v0.2/widget.schema.json`, em `tests/contract/`
- [ ] T034 [P] `farol-core`: teste de unidade da renderização do novo `kind` de widget (`monitor-status-grid`, incluindo o estado explícito de erro/não-configurado distinto de lista vazia) em `tests/unit/`
- [ ] T035 [P] plugin `uptime-kuma`: testes `pytest` — parsing Prometheus com corpos sintéticos válidos e inválidos (`metrics_parser.py`), mapeamento de status FR-012, lógica de cache/erro do poller mockando a chamada HTTP e o `op read` (`poller.py`/`secrets.py`), em `tests/unit/test_uptime_kuma_*.py`

---

## Dependencies & Execution Order

### Dependências entre fases

- **Setup (Fase 1)**: sem dependências — pode começar imediatamente.
- **Foundational 2a — Protocolo (D1)**: depende da Fase 1. **Bloqueia** 2b e 2c, e transitivamente
  todo o resto — nenhuma linha de código de `farol-protocol`, extensão de `farol-core` ou do plugin
  `uptime-kuma` é escrita antes de `protocol/schema/v0.2/` existir.
- **Foundational 2b/2c — Bindings Rust e esqueleto Python**: dependem de 2a. A partir daqui, **core
  (Rust) e plugin (Python) são paralelizáveis** entre si — diretórios e linguagens distintas.
- **Validação Foundational (T015, Cenário 9)**: depende de 2a + 2b (T004, T009–T011) — não depende
  de 2c nem de nenhuma task do plugin `uptime-kuma`.
- **User Story 1 (Fase 3)**: depende da Fase 2 completa (incluindo T015). Nenhuma dependência de
  outra user story.
- **User Story 2 (Fase 4)**: depende da User Story 1 completa — dependência de produto explícita da
  spec (US2 "Depende da User Story 1 já estar funcionando"), não uma dependência técnica nova de
  arquivo: as tasks de US2 são só validação do que US1/Foundational já constroem.
- **Polish (Fase 5)**: depende de todas as user stories desejadas estarem completas.

### Dentro de cada user story

- US1: handshake (T016) e infraestrutura de leitura (T017–T019) antes do handler de `widget/get`
  (T020); lado core (T021–T024) depende só de Foundational (T009/T010), independente do lado
  plugin; validação (T025–T029) depende de toda a implementação da story.
- US2: nenhuma implementação nova — validação (T030–T031) depende de US1 completa.

### Oportunidades de paralelismo

- Todas as tasks `[P]` da Fase 1 podem rodar em paralelo entre si.
- T005–T008 (schemas JSON, Fase 2a) podem rodar em paralelo entre si, mas só depois de T004.
- **Depois que 2a termina**, T009/T011 (`farol-protocol`) e T012–T014 (`plugins/uptime-kuma`) podem
  rodar em paralelo — duas frentes sem arquivo compartilhado (T010 depende de T009, sequencial no
  mesmo arquivo `messages.rs`).
- Dentro de US1: T016–T018 (plugin) e T021 (core `model.rs`) podem todos rodar em paralelo entre
  si — dependem só de Foundational, arquivos e linguagens distintas. T019/T020 (plugin) são
  sequenciais entre si e a T017/T018/T016 respectivamente. T022–T024 (core) são sequenciais entre
  si (mesmo arquivo `update.rs`→`view.rs`→`view.rs`), mas todo o bloco T021–T024 pode avançar em
  paralelo ao bloco T016–T020 do plugin.
- Fase 5: T032/T033/T034/T035 são todas paralelizáveis entre si (arquivos e propósitos distintos).

---

## Parallel Example: Foundational 2b/2c (após protocolo 2a pronto)

```bash
# Duas frentes paralelas, sem arquivo compartilhado, ambas dependendo só de T004-T008:
Task: "Evoluir CapabilityManifest em crates/farol-protocol/src/messages.rs (T009)"
Task: "Loop de leitura/escrita NDJSON em plugins/uptime-kuma/main.py (T012)"
```

## Parallel Example: User Story 1

```bash
# Lado plugin e lado core, em paralelo:
Task: "Handler de handshake/hello em plugins/uptime-kuma/main.py (T016)"
Task: "Cliente HTTP com Basic Auth em plugins/uptime-kuma/metrics_client.py (T017)"
Task: "Adicionar MonitorWidgetViewModel em crates/farol-core/src/model.rs (T021)"
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Fase 1: Setup (incluindo confirmar `op whoami`, T001).
2. Completar Fase 2 (2a protocolo → 2b/2c em paralelo → T015) — bloqueante, sem atalho.
3. Completar Fase 3: User Story 1.
4. **PARAR e VALIDAR**: rodar Cenários 1, 2, 3, 4 e 7 de `quickstart.md` (T025–T029)
   isoladamente.
5. Esse é o MVP que prova o segundo perfil de capacidade (rede + segredo) e o novo `kind` de
   widget declarativo com um consumidor real (Princípios III e IV da constitution).

### Entrega Incremental

1. Setup + Foundational (2a → 2b/2c → T015) → protocolo `"0.2"` pronto, quebra de `git-local`
   confirmada e documentada (débito técnico rastreado em issue separada).
2. User Story 1 → validar com Cenários 1–4, 7 → **MVP**.
3. User Story 2 → validar com Cenários 5–6 → resiliência sob rede degradada provada, sem
   implementação nova além do que US1/Foundational já construíram.
4. Polish → Cenário 8 (isolamento de falha herdado) + suíte de testes automatizados.
5. Cada story soma valor sem quebrar a anterior — critério de "feature 002 provada"
   (`quickstart.md` § final) é os 9 cenários passando.

### Estratégia de Equipe Paralela

Depois que a Fase 2 (protocolo + bindings + esqueletos) está pronta:

- Uma frente pode seguir em `crates/farol-core`/`crates/farol-protocol` (Rust) enquanto outra segue
  em `plugins/uptime-kuma` (Python) — sem colisão de arquivo, unidas apenas pelo contrato em
  `protocol/schema/v0.2/`.
- User Story 2 não tem frente própria de implementação (só validação) — pode ser conduzida por
  quem validou User Story 1, assim que ela fechar.

---

## Notes

- `[P]` = arquivos diferentes, sem dependência pendente.
- `[Story]` mapeia a task à user story correspondente para rastreabilidade.
- Nenhuma task desta lista cobre itens do `## Out of Scope` de `spec.md` (enforcement de allowlist
  de rede, qualquer ação de escrita/gerenciamento de monitores, outro plugin, migração de
  `git-local`, workspaces, paleta de comandos, registry, empacotamento) — de propósito.
- `protocol/schema/v0.2/` (Fase 2a) é bloqueante para tudo o mais, por decisão D1 de `research.md`:
  nenhum binding Rust nem trecho do plugin Python nasce antes da especificação normativa existir.
- A migração de `plugins/git-local/` para `protocol_version = "0.2"` **não é tarefa deste
  `tasks.md`** — débito técnico rastreado em issue própria do tracker do projeto, MUST existir
  antes de esta feature ser considerada encerrada (constitution v0.3.0, Governance).
- Verificar que cada cenário de `quickstart.md` (9 no total) passa antes de considerar a story
  correspondente, ou a feature como um todo, encerrada.
- Parar em qualquer checkpoint para validar a story isoladamente antes de seguir para a próxima.
