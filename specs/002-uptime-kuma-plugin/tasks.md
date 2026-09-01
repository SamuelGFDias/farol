---

description: "Task list template for feature implementation"
---

# Tasks: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Input**: Design documents from `/specs/002-uptime-kuma-plugin/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/ (5 documentos), quickstart.md

**Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: esta versão de `tasks.md` renumera e
corrige a versão anterior, que continha lacunas CRITICAL/HIGH confirmadas contra o código real do
repositório (`crates/farol-core/src/plugin_worker.rs`, `main.rs`, `crates/farol-protocol/src/
messages.rs`) e incorpora uma segunda decisão de arquitetura tomada após o `/speckit-plan`: a
credencial e a URL base deste plugin deixam de ser resolvidas via CLI `op`/1Password pelo próprio
plugin e passam a ser geridas inteiramente pelo **core** — armazenamento em `config.toml`/
`secrets.toml`, injeção por variável de ambiente no spawn, e uma tela de setup dentro do próprio app
`iced` (`research.md` D8, revisado). Nenhuma task abaixo foi iniciada — renumerar é seguro. Zero linha
de implementação existe neste repositório para esta feature.

**Tests**: Sem TDD explicitamente requisitado pela spec. As tasks de teste geradas aqui vêm de
`plan.md` § Testing (`cargo test`/`pytest` já descritos como parte do Technical Context, não como
pedido de "tests first") e do `quickstart.md` (8 cenários de validação manual — revisado nesta sessão,
substituindo o mock de `op read` por mock de variável de ambiente e removendo o cenário de binário `op`
ausente, que não se aplica mais), mapeados como tasks de verificação ao final da user story
correspondente, ou da fase Foundational/Polish quando o cenário não é específico de uma story.

**Organization**: Tasks agrupadas por user story priorizada (P1 → P2, `spec.md` — esta feature não
define P3). Duas decisões técnicas são pré-requisito bloqueante explícito da Fase Foundational,
análogo a como `protocol/` bloqueou tudo na feature 001: (1) a evolução do `CapabilityManifest`
estruturado por `kind` + bump de `protocol_version` para `"0.2"` (D1 de `research.md`); (2) o novo
mecanismo de configuração/segredo gerido pelo core — campo de protocolo `required_config`,
armazenamento em `config.toml`/`secrets.toml`, injeção por variável de ambiente (D8 de `research.md`,
revisado nesta sessão). A migração do plugin `git-local` (feature 001) para o novo formato de
capacidades **NÃO** é tarefa de implementação desta feature — é débito técnico já rastreado como
**issue #4**, aberta no tracker do projeto, registrada como task própria em § Débito técnico ao final
deste documento. Nenhuma task das Fases 1–5 abaixo toca `plugins/git-local/`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência de task incompleta)
- **[Story]**: A qual user story esta task pertence (US1, US2) — ausente em Setup, Foundational,
  Polish e Débito técnico
- Caminho de arquivo exato em cada descrição; quando a task **corrige** um trecho já existente no
  repositório, a linha/intervalo atual é citada (`arquivo:linha`) para que quem implementar confirme
  que ainda corresponde ao estado do código antes de editar

## Path Conventions

Estrutura definida em `plan.md` § Project Structure — mesma estrutura de workspace da feature 001
(`crates/farol-core`, `crates/farol-protocol`, `protocol/`), sem crate novo. `crates/farol-core` já
existe (feature 001) — esta feature não recria seu esqueleto, mas **estende `main.rs` e
`plugin_worker.rs` de verdade**, não só `model.rs`/`update.rs`/`view.rs` (correção desta sessão —
versões anteriores deste documento e de `plan.md` marcavam os dois primeiros como "inalterados", o
que é falso: ver Fase 2b abaixo). `plugins/uptime-kuma/` é novo, sibling de `plugins/git-local/`
(inalterado):

```text
protocol/SPEC.md                   # título/versão passam a descrever "0.2"; + campo required_config
protocol/schema/v0.1/              # RETIDO como registro histórico — não editado nesta feature
protocol/schema/v0.2/              # NOVO — Capability estruturada (exec/network), RequiredConfigItem,
                                    # MonitorStatusItem, novos reasons
crates/farol-core/src/             # main.rs, plugin_worker.rs, model.rs, update.rs, view.rs — todos
                                    # estendidos (ver Fase 2b/3 para o que muda em cada um)
crates/farol-protocol/src/         # messages.rs (Capability, RequiredConfigItem, MonitorStatusItem,
                                    # WidgetGetResult.items como união discriminada — C3)
plugins/uptime-kuma/               # main.py, config.py, secrets.py, metrics_client.py,
                                    # metrics_parser.py, poller.py — config.py/secrets.py simplificados
                                    # a leitura de variável de ambiente (D8), sem parsing de TOML nem
                                    # CLI externo
tests/{contract,integration,unit}/
```

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Esqueleto de arquivos do novo plugin — sem lógica de protocolo nem de negócio ainda.
**Sem pré-requisito de ambiente externo nesta revisão** (correção desta sessão — versões anteriores
exigiam o CLI `op` do 1Password instalado/autenticado antes de começar; essa dependência foi removida
por D8 revisado).

- [X] T001 [P] Criar esqueleto de `plugins/uptime-kuma/` (`main.py`, `config.py`, `secrets.py`,
  `metrics_client.py`, `metrics_parser.py`, `poller.py` — stubs, apenas stdlib), per `plan.md` §
  Project Structure e D7 de `research.md`
- [X] T002 [P] Configurar lint/format de `plugins/uptime-kuma` (Python, stdlib only — sem dependência
  de runtime externa), mesmo padrão já usado em `plugins/git-local`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Duas decisões técnicas centrais são pré-requisito bloqueante de tudo o mais, no mesmo
padrão de `protocol/` na feature 001: (1) `CapabilityManifest` estruturado por `kind` + bump de
`protocol_version` (D1); (2) o mecanismo de configuração/segredo gerido pelo core — `required_config`,
`config.toml`/`secrets.toml`, injeção por variável de ambiente (D8, revisado). Dividida em quatro
subfases: 2a (especificação normativa do protocolo, bloqueia as demais), 2b (`farol-protocol` +
`farol-core`, Rust — inclui as correções CRITICAL/HIGH de uma auditoria técnica anterior, C1–C3/H1–H4,
e a infraestrutura de configuração/segredo de D8) e 2c (esqueleto do plugin `uptime-kuma`, Python),
paralelizáveis entre si depois de 2a.

**⚠️ CRITICAL**: Nenhuma task de `farol-protocol`, `farol-core` (extensões desta feature) ou do
plugin `uptime-kuma` pode começar antes de 2a estar completa.

### Phase 2a — Evolução do Protocolo para `"0.2"` (bloqueante de tudo o mais — D1, D8)

- [X] T003 Atualizar `protocol/SPEC.md` — título/versão corrente passam a descrever `"0.2"`; §6.3
  ganha a nova forma estruturada de `CapabilityManifest`/`Capability` (discriminada por `kind`:
  **`exec`/`network`** — sem `kind: "secret"`, removido nesta revisão, D1/D8 de `research.md`) e o
  campo novo `required_config: RequiredConfigItem[]` (irmão de `capabilities`/`widgets`/`actions` em
  `HandshakeHelloResult`, D8); §8.2/§10 ganham a tabela dos novos `reason`s (`-32005`..`-32007`, D9,
  com `not_configured` documentado como salvaguarda — o caminho primário é o core recusar `Ready` via
  `PluginState::Unavailable{NotConfigured}` antes de chamar `widget/get`); `protocol/schema/v0.1/`
  permanece **intocado**, retido como registro histórico do formato que `git-local` (inalterado)
  ainda fala, per `contracts/framing-and-versioning-delta.md`
- [X] T004 [P] Criar `protocol/schema/v0.2/handshake.schema.json` — `Capability` discriminada por
  `kind` (`exec` sem campos extras; `network` com `host` obrigatório + `port` opcional; **sem**
  `kind: "secret"`), `CapabilityManifest.capabilities: Capability[]` **sem** `minItems: 1` (MAY ser
  `[]`); + `RequiredConfigItem` (`name`, `secret`, `description`, todos obrigatórios) e o campo
  `required_config: RequiredConfigItem[]` em `HandshakeHelloResult`, per `research.md` D1/D8 (schema
  JSON ilustrativo `allOf`/`if`/`then` por `kind`) e `contracts/handshake-delta.md`. **Correção H4**:
  `$id` deste schema MUST apontar para `/v0.2/handshake.schema.json` (não copiar o `$id` absoluto de
  `v0.1/`), e qualquer `$ref` cross-arquivo (ex.: para `error.schema.json`) MUST apontar para o
  arquivo-irmão em `v0.2/`, nunca para `v0.1/` — evita colisão no registry de validação de
  `crates/farol-protocol/tests/contract_schema_validation.rs:20-26`; `v0.1/handshake.schema.json:3`
  e `action.schema.json:30,57` mostram o padrão de `$id`/`$ref` absoluto versionado que MUST ser
  seguido (apontando para `/v0.2/`, não para `/v0.1/`), não copiado
- [X] T005 [P] Criar `protocol/schema/v0.2/widget.schema.json` — novo item `MonitorStatusItem`
  (`name`, `status: "up"|"down"|"pending"|"maintenance"`, `response_time_ms: integer|null`) e novo
  valor de `kind` de widget `"monitor-status-grid"`; `WidgetItem`/`kind: "status-grid"` existentes
  permanecem inalterados no mesmo arquivo, per `research.md` D4 e `data-model.md` §1.3–§1.5.
  **Correção H4**: mesmo ajuste de `$id`/`$ref` de T004 (ex.: `v0.1/widget.schema.json:122` referencia
  `handshake.schema.json` de `v0.1` — o equivalente em `v0.2/` MUST referenciar `v0.2/handshake.schema.json`)
- [X] T006 [P] Criar `protocol/schema/v0.2/action.schema.json` — forma inalterada em relação a
  `v0.1/action.schema.json` (copiado/referenciado; ainda usado pelo `git-local` futuro migrado, que
  continua tendo ação de fetch), per `plan.md` § Project Structure. **Correção H4**: mesmo ajuste de
  `$id`/`$ref` de T004/T005
- [X] T007 [P] Criar `protocol/schema/v0.2/error.schema.json` — forma do `ErrorObject` inalterada;
  descrição/catálogo textual ganha os três novos `reason`s (`not_configured` `-32005`,
  `metrics_unreachable` `-32006`, `metrics_parse_error` `-32007`) e documenta que `exec_unavailable`
  (`-32003`) permanece no catálogo geral do protocolo mas **sem uso por `uptime-kuma`** (D8 revisado —
  não invoca mais nenhum binário externo), per `research.md` D9 e `contracts/error-model-delta.md`.
  **Correção H4**: mesmo ajuste de `$id`/`$ref` de T004–T006

> **Nota (débito técnico #4, já rastreado como issue aberta)**: um core em `"0.2"` recusa de forma
> limpa o plugin `git-local` (feature 001), inalterado, via `Unavailable{VersionIncompatible}` —
> consequência deliberada de D1. A migração de `plugins/git-local/` para `protocol_version = "0.2"` +
> o novo formato de `capabilities` está registrada como **issue #4** do tracker do projeto ("plugin
> git-local precisa migrar para protocol_version 0.2 (CapabilityManifest estruturado)") — já aberta,
> não criada por este documento. Task própria em § Débito técnico ao final deste arquivo (T050).

**Checkpoint 2a**: `protocol/schema/v0.2/` completo e normativo. **Nenhuma task de `farol-protocol`
(Rust) ou `plugins/uptime-kuma` (Python) pode começar antes deste checkpoint.**

### Phase 2b — `farol-protocol` + `farol-core` (Rust): bindings do novo schema, correções CRITICAL/HIGH e infraestrutura de configuração/segredo (D8)

- [X] T008 [P] Evoluir `CapabilityManifest` em `crates/farol-protocol/src/messages.rs:76-82` — enum
  `Capability` (`#[serde(tag = "kind", rename_all = "snake_case")]`: `Exec`, `Network { host,
  port: Option<u16> }` — **sem** variante `Secret`, removida nesta revisão), `CapabilityManifest.
  capabilities: Vec<Capability>` (substitui `Vec<String>`), per `research.md` D1/D8 (binding Rust
  ilustrativo) e `data-model.md` §1.1–§1.2; tratamento de `kind` desconhecido (forward-compat) é
  decisão de implementação a resolver aqui, per nota de `research.md` D1 (depende de T004)
- [X] T009 Adicionar `RequiredConfigItem { name: String, secret: bool, description: String }` e o
  campo `required_config: Vec<RequiredConfigItem>` em `HandshakeHelloResult`
  (`crates/farol-protocol/src/messages.rs:136-150`, logo após `capabilities`, antes de `widgets`),
  per `research.md` D8 e `data-model.md` §1.6/§1.6.1 (mesmo arquivo de T008 — sequencial, depende de
  T004, T008)
- [X] T010 Adicionar `MonitorStatusItem` e o novo valor de `kind` de widget `"monitor-status-grid"` ao
  vocabulário de `crates/farol-protocol/src/messages.rs`, per `research.md` D4 e `data-model.md`
  §1.3–§1.5. **Correção C3**: `WidgetGetResult.items` (hoje `Vec<WidgetItem>` fixo,
  `messages.rs:240-246`) MUST mudar para uma união discriminada que aceite `WidgetItem` (git) **ou**
  `MonitorStatusItem` (uptime-kuma) — não é uma reinterpretação de tipo em runtime sem mudança de
  código; ver `data-model.md` §1.4 para o desenho esperado (decisão exata de forma — enum genérico,
  campos `Option<Vec<T>>` mutuamente exclusivos, ou outra — é decisão de implementação desta task)
  (mesmo arquivo de T008/T009 — sequencial, depende de T005, T009)
- [X] T011 **[Correção C1]** Extrair/checar `protocol_version` via `serde_json::Value` (probe só do
  campo `protocol_version`) **antes** da desserialização tipada de `HandshakeHelloResponse` em
  `crates/farol-core/src/plugin_worker.rs` — hoje (`plugin_worker.rs:296-325`) a resposta inteira do
  handshake é desserializada como `HandshakeHelloResponse` tipado (`#[serde(untagged)]`,
  `messages.rs:154-167`) **antes** de checar `protocol_version`; com `Capability` tagueado por `kind`
  (T008), a resposta `v0.1` de um plugin desatualizado (`{"capabilities": ["exec"]}`, string) não
  desserializa em nenhuma variante → `Err` de decode → `plugin_worker.rs:306` cai em
  `HandshakeOutcome::Unresponsive`, **nunca chega em `is_compatible_with`** (só chamado dentro do
  ramo `Success`, linhas 312-314) — o esperado (`Unavailable{VersionIncompatible}`, Cenário 8 de
  `quickstart.md`) não acontece. MUST landar junto de ou antes de T008 entrar em uso real (mesma
  condição de corrida que motivou esta correção) (depende de T004; T023 abaixo — validação do
  Cenário 8 — depende desta task)
- [X] T012 **[Correção H1]** Bump da versão de protocolo suportada pelo core para `"0.2"` —
  **CORRIGIDO**: a constante que carrega essa versão é `CORE_PROTOCOL_VERSION` em
  `crates/farol-core/src/plugin_worker.rs:100` (`ProtocolVersion { major: 0, minor: 1 }` hoje), **não**
  em `crates/farol-protocol/src/version.rs` (versões anteriores deste documento e `plan.md:172`
  apontavam o arquivo errado — `version.rs` contém só a lógica de comparação por igualdade exata sob
  `MAJOR == 0`, que permanece inalterada); atualizar `plugin_worker.rs:100` para
  `ProtocolVersion { major: 0, minor: 2 }` (depende de T003)
- [X] T013 **[Correção H2]** Corrigir os 3 pontos que quebram de compilar com `Capability` estruturado
  (T008): `crates/farol-core/src/view.rs:60-63`
  (`identity.capabilities.capabilities.join(", ")`, só compila hoje com `Vec<String>`) e
  `crates/farol-core/src/update.rs:319-320,352-353,638-639`
  (`CapabilityManifest { capabilities: vec!["exec".to_string()] }`, literais de teste/helper) —
  atualizar para o novo `Vec<Capability>` (ex.: `vec![Capability::Exec]` e uma forma de exibição em
  `view.rs` que itere `Capability` estruturado em vez de `String`) (depende de T008; dependência de
  qualquer task que exija build/teste passando, incluindo T023)
- [X] T014 **[Correção C2, parte 1]** Tornar comando/args do processo do plugin configuráveis em
  `crates/farol-core/src/plugin_worker.rs` — hoje `PLUGIN_COMMAND`/`PLUGIN_ARGS`
  (`plugin_worker.rs:74,80`) são constantes hardcoded para `python3 plugins/git-local/main.py`; sem
  esta correção, T024 em diante (US1) é inexecutável (nenhum código spawna `uptime-kuma`)
- [X] T015 **[Correção C2, parte 2]** Suportar múltiplas `PluginConnection` simultâneas em
  `crates/farol-core/src/main.rs` — hoje (`main.rs:29-31`) `Farol.plugin` é uma única
  `model::PluginConnection`; vira uma coleção (uma conexão por plugin conhecido nesta feature —
  lista fixa `git-local` + `uptime-kuma`, hardcoded no core; um registry federado de plugins é
  `Out of Scope` do `spec.md`, não introduzido aqui), cada uma com seu próprio comando/args (T014) e
  sua própria subscription de worker (depende de T014)
- [X] T016 **[D8]** Leitura/escrita de `config.toml` (valores não-secretos de `required_config`) por
  plugin no core — `$XDG_CONFIG_HOME/farol/plugins/<nome>/config.toml`, generalizando o mecanismo já
  usado para `scan_root` de `git-local` (feature 001) para aceitar qualquer conjunto de chaves
  declarado por `required_config`, não só um campo fixo, per `research.md` D8
- [X] T017 **[D8]** Escrita de `$XDG_CONFIG_HOME/farol/secrets.toml` (valores secretos de
  `required_config`) exclusivamente pelo core, com permissão de arquivo forçada a `0600` (Unix) —
  seção por plugin (`[uptime-kuma]`), per `research.md` D8 (mesmo módulo de configuração de T016,
  arquivo separado por decisão de D8 — segredo nunca no mesmo arquivo que configuração não-secreta)
- [X] T018 **[D8]** Resolver cada item de `required_config` recebido no handshake contra
  `config.toml`/`secrets.toml` (T016/T017) e injetar como variável de ambiente do processo filho
  (`Command::env(env_var_name, value)`) no spawn, em `crates/farol-core/src/plugin_worker.rs` — nome
  da variável de ambiente = `FAROL_PLUGIN_<PLUGIN_NAME_MAIÚSCULO>_<NAME_MAIÚSCULO>` (convenção fixa de
  `research.md` D8, aplicada identicamente pelo lado Python em T021/T022) (depende de T009, T014,
  T016, T017)
- [X] T019 **[D8]** Comparar `required_config` recebido no `HandshakeHelloResult` contra o que foi
  efetivamente injetado (T018); se algum item obrigatório não tiver valor, transicionar
  `PluginState` para `Unavailable { reason: NotConfigured, .. }` **em vez de** `Ready` — novo membro
  `NotConfigured` em `UnavailableReason` (`crates/farol-core/src/model.rs`, hoje só `FailedToStart`/
  `VersionIncompatible`/`Crashed`/`Unresponsive`), **não terminal** (única exceção à regra "Unavailable
  é terminal" da feature 001 — precisa de caminho de volta, ver T032); o core não chama `widget/get`
  para uma conexão neste estado, per `research.md` D8/D9 e `data-model.md` §3.2 (depende de T009,
  T018)

### Phase 2c — Esqueleto do plugin `uptime-kuma` (Python) — paralelo a 2b

- [X] T020 [P] Loop de leitura/escrita NDJSON sobre stdin/stdout em `plugins/uptime-kuma/main.py`
  (mesmo padrão de `plugins/git-local/main.py`, D7 reafirma D3 da feature 001) — sem lógica de negócio
  ainda (depende de T001)
- [X] T021 [P] **[Correção 2.4]** Leitor de variável de ambiente em `plugins/uptime-kuma/config.py` —
  substitui o leitor de TOML de versões anteriores deste documento: `os.environ.get()` para
  `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL`, aplicando a mesma convenção de nome de T018, per `research.md`
  D8. **Sem** default seguro (diferente do `scan_root` de `git-local`) — ausência/vazio é estado a
  tratar (`not_configured`, FR-007/FR-008) (depende de T001)
- [X] T022 [P] **[Correção 2.4]** Leitor de variável de ambiente (secreta) em
  `plugins/uptime-kuma/secrets.py` — substitui a chamada a `op read` via `subprocess` de versões
  anteriores deste documento: `os.environ.get()` para `FAROL_PLUGIN_UPTIME_KUMA_API_KEY`, mesmo
  mecanismo de leitura de T021 (a única diferença entre os dois módulos é qual `name`/variável cada um
  lê — MAY colapsar num único módulo de configuração se preferir, critério de pronto inalterado: duas
  funções `Optional[str]`, uma por variável) (depende de T001)

**Checkpoint 2b/2c**: `farol-protocol` fala `"0.2"` com `Capability`/`RequiredConfigItem`/
`MonitorStatusItem`; `farol-core` spawna múltiplos plugins com comando configurável, resolve/injeta
`required_config` e reconhece `PluginState::Unavailable{NotConfigured}`; esqueleto de
`plugins/uptime-kuma` existe. A implementação da User Story 1 pode começar.

### Validação da Fase Foundational (cenário de `quickstart.md` — consequência do bump de versão)

- [X] T023 Executar Cenário 8 de `quickstart.md` (renumerado nesta sessão — era Cenário 9) — rodar
  `farol-core` (recompilado com `farol-protocol` em `"0.2"`, T008–T012) com o plugin `git-local` da
  feature 001 **inalterado** configurado (junto de `uptime-kuma`, graças a T014/T015); confirmar
  `PluginState = Unavailable{VersionIncompatible}` para `git-local`, mensagem legível citando `"0.1"`
  (plugin) vs. `"0.2"` (core), **sem crash do core**, widget de repositórios git ausente, enquanto
  `uptime-kuma` (se configurado) continua funcionando normalmente — confirma a consequência
  deliberada de D1 antes de investir em US1/US2, e valida a correção C1 (T011) especificamente: sem
  ela, o resultado observado seria `Unresponsive`, não `VersionIncompatible` (depende de T003, T008,
  T009, T010, T011, T012, T014, T015)

  **Execução real (`cargo run --bin farol` com os dois plugins spawnados, config/secrets já
  cadastrados em `~/.config/farol/`)**: revelou um bug de execução não coberto por nenhum teste
  unitário — `Farol::subscription` (`update.rs`) montava `plugin_worker::subscription(...).map(move
  |event| Message::Worker { plugin_name: worker_plugin_name.clone(), event })`, um closure
  **capturante**. `iced::Subscription::map` exige `size_of::<F>() == 0` (`debug_assert!` em
  `iced_futures::subscription::Subscription::map`) e entra em panic assim que a primeira
  `Subscription` é montada — o core morria no primeiro frame, antes de qualquer handshake, com todo
  plugin configurado (não só com `git-local`/versão incompatível). Nenhum teste unitário existente
  exercitava `Farol::subscription` (todos chamam `handle_worker_event`/`handle_handshake_outcome`
  diretamente), então isso só apareceu num `cargo run` real — exatamente o motivo desta task existir
  em vez de confiar só na suíte automatizada.

  **Correção aplicada**: `plugin_worker::subscription` passou a devolver
  `Subscription<(String, WorkerEvent)>` — o `plugin_name` é embutido no stream via
  `futures::StreamExt::map` (sem a restrição de closure não-capturante, por não passar pelo
  `Subscription::map` do `iced`) *antes* de virar `Subscription`, em vez de ser anexado depois por um
  closure capturante. `update.rs::subscription` agora usa `.map(|(plugin_name, event)| Message::Worker
  { plugin_name, event })` — não captura nada do ambiente, só usa o próprio parâmetro. Suíte completa
  (61 testes) e `cargo clippy --workspace --all-targets` permanecem limpos após a correção.

  **Resultado observado** (log de diagnóstico temporário em `handle_worker_event`, removido após a
  validação): `git-local` → `HandshakeCompleted(VersionIncompatible { plugin_version: 0.1,
  core_version: 0.2 })` — exatamente o resultado esperado, confirmando a correção C1 (T011).
  `uptime-kuma` → `HandshakeCompleted(Unresponsive)`, não `Ready`: esperado e fora do escopo desta
  task (T023 não depende de T020–T022/T024–T028) — `handle_handshake_hello` em
  `plugins/uptime-kuma/main.py` ainda é um placeholder que devolve erro JSON-RPC (`TODO US1`), então
  o probe de versão do core não encontra `result.protocol_version` e cai em `Unresponsive`; a
  claúsula "uptime-kuma continua funcionando normalmente" da descrição acima só se torna válida
  depois de T024. Em nenhum dos dois casos o core travou ou saiu do processo — critério central desta
  task confirmado.

---

## Phase 3: User Story 1 - Ver o estado dos monitores Uptime Kuma ao abrir o Farol (Priority: P1) 🎯 MVP

**Goal**: Core inicia o plugin, os dois fazem handshake em `"0.2"`, o plugin declara identidade,
manifesto de capacidades (`network` quando `base_url` resolvido) e `required_config` (sempre), e o
widget `monitor-status-grid`; o plugin lê `/metrics` periodicamente via thread de polling em
background, mantém cache em memória, e responde a `widget/get` sempre a partir do cache (nunca I/O de
rede síncrono, FR-010); o core renderiza a lista de monitores e a mantém atualizada sozinha. Ausência
de `base_url`/API Key produz uma **tela de setup** dentro do próprio Farol (D8, revisado nesta
sessão) — nunca uma lista vazia silenciosa, nunca exige editar arquivo ou instalar ferramenta externa.

**Independent Test**: Na primeira execução, preencher a tela de setup do plugin com a URL e a API Key
de uma instância Uptime Kuma acessível com pelo menos um monitor cadastrado, e verificar que a lista
de monitores aparece corretamente na janela do Farol logo em seguida, sem qualquer edição de arquivo.

### Implementação para User Story 1

- [ ] T024 [P] [US1] Handler de `handshake/hello` em `plugins/uptime-kuma/main.py` — responde
  `plugin_name: "uptime-kuma"`, `protocol_version: "0.2"`, `capabilities.capabilities`
  (`{"kind":"network","host":...,"port":...}` só quando `base_url` resolvido — **sem** `exec`, **sem**
  `secret`, D8 revisado), `required_config` (sempre os dois itens — `base_url` não-secreto, `api_key`
  secreto, D8), `widgets: [{"id":"uptime-kuma-monitors","kind":"monitor-status-grid","title":"Uptime
  Kuma","suggested_refresh_interval_ms":30000}]`, `actions: []` sempre (FR-002–FR-006,
  `contracts/handshake-delta.md`) (depende de T020, T021, T022, T008, T009)
- [ ] T025 [P] [US1] Cliente HTTP com Basic Auth em `plugins/uptime-kuma/metrics_client.py` —
  `urllib.request` contra `${base_url}/metrics`, header `Authorization: Basic base64(":"+api_key)`,
  timeout de 10s (D5/D7 de `research.md`, `contracts/uptime-kuma-plugin.md` § Autenticação HTTP)
  (depende de T021, T022)
- [ ] T026 [P] [US1] Parser Prometheus mínimo em `plugins/uptime-kuma/metrics_parser.py` — reconhece
  só `monitor_status{monitor_name="...",...}` e `monitor_response_time{monitor_name="...",...}`,
  mapeia `monitor_status` para `status` (`1→up`, `0→down`, `2→pending`, `3→maintenance`, FR-012),
  `metrics_parse_error` quando nenhuma linha `monitor_status{...}` é encontrada ou algum valor está
  fora de `{0,1,2,3}` (falha da resposta inteira daquela tentativa, não item a item), linhas
  malformadas isoladas são puladas (D7 de `research.md`, `contracts/uptime-kuma-plugin.md` § Parsing)
- [ ] T027 [US1] Thread de polling em background + cache (`last_success`/`last_error`,
  `threading.Lock`) em `plugins/uptime-kuma/poller.py` — laço estritamente sequencial a cada
  `suggested_refresh_interval_ms` (30000 default), chama `metrics_client`/`metrics_parser`, nunca duas
  chamadas HTTP concorrentes por construção, estado inicial pré-populado com `last_error = "aguardando
  primeira leitura"` (D6 de `research.md`, resolve FR-010, `data-model.md` §2.3–§2.4) (depende de
  T025, T026)
- [ ] T028 [US1] Handler de `widget/get` em `plugins/uptime-kuma/main.py` — lê exclusivamente o cache
  do poller sob o mesmo lock (nunca I/O de rede síncrono); lógica: `not_configured` (`-32005`,
  **salvaguarda** — D8/D9 revisados, o caminho primário é T019 no core) se
  `base_url`/`api_key` ausentes das variáveis de ambiente; senão `last_error` (`-32006`/`-32007`) se
  `last_success is None` ou `last_error.at >= last_success.at`; senão sucesso com `items =
  last_success.monitors` (D6/D9 de `research.md`, `data-model.md` §2.3, `contracts/widget-protocol-delta.md`)
  (mesmo arquivo de T024 — sequencial, depende de T024, T027)
- [ ] T029 [P] [US1] Adicionar `MonitorWidgetViewModel` (`monitors: MonitorStatusItem[]`,
  `last_error: Option<PluginError>`) em `crates/farol-core/src/model.rs`, per `data-model.md` §3.1
  (depende de T010)
- [ ] T030 [US1] **[D8]** Adicionar estado de UI do formulário de setup em
  `crates/farol-core/src/model.rs` — `UnavailableReason::NotConfigured` (já introduzido pela mudança
  de tipo em T019, aqui consumido pela UI) e `SetupForm { plugin_name: String, fields:
  Vec<(RequiredConfigItem, String)> }`, per `data-model.md` §3.2 (depende de T019, T009)
- [ ] T031 [US1] Popular `MonitorWidgetViewModel` a partir do resultado de `widget/get` do widget
  `uptime-kuma-monitors` em `crates/farol-core/src/update.rs` — sucesso atualiza `monitors`; qualquer
  erro pontual (`not_configured`/`metrics_unreachable`/`metrics_parse_error`) atualiza só
  `last_error`, preservando `monitors` anterior, sem alterar `PluginState` (FR-017, mesmo mecanismo
  genérico `protocol/SPEC.md` §5.2 já usado para `git-local`); usa a união discriminada de `items`
  introduzida por T010 (correção C3) — generaliza `merge_widget_items` (`update.rs:291-306`, hoje
  tipado só para `Vec<farol_protocol::WidgetItem>`) para também aceitar `MonitorStatusItem` (depende
  de T029, T010)
- [ ] T032 [US1] **[D8]** Mensagens novas em `crates/farol-core/src/update.rs` —
  `Message::SetupFieldChanged { plugin_name, field_name, value }` (atualiza `SetupForm.fields`) e
  `Message::SetupSubmitted { plugin_name }` (persiste cada valor em `config.toml`/`secrets.toml`
  conforme `secret` do item, T016/T017, e dispara a reconexão do worker daquele plugin — mecanismo de
  restart da `Subscription`, `research.md` D8, "Decisão — tela de setup") (depende de T030, T016,
  T017, T018)
- [ ] T033 [US1] Renderizar o `kind: "monitor-status-grid"` em `crates/farol-core/src/view.rs` — lista
  de monitores (nome, status `up`/`down`/`pending`/`maintenance`, tempo de resposta quando aplicável)
  mapeando `MonitorStatusItem`; qualquer `last_error` presente (incluindo `not_configured`) MUST
  renderizar um estado explícito, visivelmente distinto de "0 monitores" (FR-008, FR-013, FR-014,
  SC-001, SC-005) (depende de T031, T010)
- [ ] T034 [US1] Exibir a capacidade `network` (host/port) do manifesto deste plugin na UI, mesmo
  padrão apenas declarativo já usado para `exec` na feature 001 (FR-005/FR-006) em
  `crates/farol-core/src/view.rs` — **correção desta sessão**: nenhuma capacidade `secret` a exibir
  mais (removida, D1/D8 revisados); usa a correção H2 (T013) para iterar `Vec<Capability>` estruturado
  (mesmo arquivo de T033 — sequencial, depende de T008, T013, T033)
- [ ] T035 [US1] **[D8]** View novo em `crates/farol-core/src/view.rs` para renderizar o formulário de
  setup (`SetupForm`, T030) quando `PluginState = Unavailable{NotConfigured}` — um campo de texto por
  item de `required_config` (mascarado quando `secret: true`), rótulo = `description`, botão de
  confirmar (dispara `Message::SetupSubmitted`, T032); renderizado **em vez do** widget normal daquele
  plugin (`data-model.md` §3.2) (mesmo arquivo de T033/T034 — sequencial, depende de T030, T032, T034)

### Validação da User Story 1 (cenários de `quickstart.md`, renumerados nesta sessão)

- [ ] T036 [US1] Executar Cenário 1 de `quickstart.md` — primeira execução sem `config.toml`/
  `secrets.toml`: confirmar que a tela de setup aparece (`Unavailable{NotConfigured}`); preencher e
  confirmar; confirmar que o widget é populado (nome/status/tempo de resposta) logo em seguida, sem
  editar nenhum arquivo manualmente; confirmar atualização automática após ~30s sem reiniciar o
  Farol; confirmar que o manifesto de capacidades exibe `network` (**não** `secret`, removida nesta
  revisão); reiniciar o Farol e confirmar que a configuração persiste (widget populado direto, sem
  tela de setup de novo) (FR-003–FR-009, SC-001, SC-002, SC-005)
- [ ] T037 [US1] Executar Cenário 2 de `quickstart.md` — primeira execução, tela de setup aparece mas
  **não** é preenchida; confirmar `PluginState = Unavailable{NotConfigured}` como caminho primário
  (T019), e — via chamada direta de `widget/get`, se exercitável no diagnóstico — `error(-32005,
  not_configured)` como salvaguarda (T028), distinguível de "0 monitores", sem crash do plugin
  (FR-008, SC-005)
- [ ] T038 [US1] Executar Cenário 3 de `quickstart.md` — com o Cenário 1 já concluído, editar
  `config.toml` para um `base_url` inválido/inacessível e reiniciar; confirmar `metrics_unreachable`
  (`-32006`), **não** `not_configured` — distingue "configurado com valor que não funciona" de "sem
  valor" (FR-019)
- [ ] T039 [US1] Executar Cenário 6 de `quickstart.md` (renumerado — era Cenário 7) — apontar
  `base_url` para uma instância Uptime Kuma real, acessível, sem nenhum monitor cadastrado; confirmar
  `widget/get` com sucesso e `items: []` como estado válido, distinto de qualquer um dos erros acima
  (Edge Case do spec, análogo a diretório sem repositórios git da feature 001)

**Checkpoint**: User Story 1 completa e testável de forma independente — MVP.

---

## Phase 4: User Story 2 - Farol permanece utilizável quando o Uptime Kuma está inacessível ou responde de forma inválida (Priority: P2)

**Goal**: Provar, sob condição adversa, o mesmo mecanismo já construído em US1/Foundational: falha
de rede ou resposta não parseável ao consultar `/metrics` é erro pontual daquela leitura — a thread
de polling (T027) já grava isso em `last_error` sem apagar `last_success`, e o core (T031, mecanismo
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

### Validação da User Story 2 (cenários de `quickstart.md`, renumerados nesta sessão)

- [ ] T040 [US2] Executar Cenário 4 de `quickstart.md` (renumerado — era Cenário 5) — com o Farol já
  rodando e o widget populado (Cenário 1/T036), tornar a instância Uptime Kuma inacessível
  (desconectar rede, apontar para host/porta fechada, ou parar o serviço) sem reiniciar o Farol;
  confirmar que (a) a janela permanece aberta e responsiva, (b) no próximo ciclo de refresh o widget
  sinaliza `metrics_unreachable` (`-32006`) mantendo os últimos monitores conhecidos, (c) restaurar o
  acesso faz os dados reais voltarem no próximo ciclo, sem intervenção manual (FR-015, FR-017,
  SC-003, SC-004)
- [ ] T041 [US2] Executar Cenário 5 de `quickstart.md` (renumerado — era Cenário 6) — apontar
  temporariamente `base_url` para um servidor HTTP que não seja Uptime Kuma (resposta não
  reconhecível como `/metrics` Prometheus válido); confirmar `error(-32007, metrics_parse_error)` no
  próximo ciclo de refresh, mesmo tratamento de erro pontual do Cenário 4, sem crash do plugin
  (FR-016)

**Checkpoint**: as duas user stories funcionam, cada uma de forma independente.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Comportamentos genéricos herdados sem reespecificação (isolamento de falha de
processo, FR-018), correção de teste de contrato quebrado por T008–T010 (H3), e a suíte de testes
automatizados descrita em `plan.md` § Testing. Nenhuma task desta fase é específica de uma user
story.

- [ ] T042 [P] Executar Cenário 7 de `quickstart.md` (renumerado — era Cenário 8) — `kill -9`/
  `kill -STOP` no processo `plugins/uptime-kuma`; confirmar comportamento idêntico ao já provado pela
  feature 001 (`Unavailable{Crashed}`/`Unavailable{Unresponsive}`), janela do Farol permanece aberta e
  responsiva — **nenhuma implementação nova**, só confirmação de que o mecanismo genérico do core (D6
  da feature 001, inalterado) se aplica também a este plugin (FR-018)
- [X] T043 **[Correção H3]** Adaptar ou aposentar
  `crates/farol-protocol/tests/contract_schema_validation.rs:41-44,155-156` — hoje valida os tipos
  Rust contra os 4 schemas `v0.1` usando `capabilities: vec!["exec".to_string()]`, que para de
  compilar/fazer sentido depois de T008–T010 (`Capability` deixa de ser `Vec<String>`). Como
  `git-local` também migra para `v0.2` via débito técnico (issue #4, T050), o teste `v0.1` passa a
  validar só registro histórico do schema, sem binding Rust ativo — decisão de implementação desta
  task: adaptar para usar o novo tipo estruturado (perdendo a cobertura do formato `v0.1` real) ou
  aposentar o teste com uma nota explicando por quê (depende de T008, T009, T010; antes de T045)
- [ ] T044 [P] `farol-protocol`: testes de contrato (`cargo test`) para `Capability`
  (serialização/deserialização de `exec`/`network` — **sem** `secret`, removida nesta revisão — contra
  `protocol/schema/v0.2/handshake.schema.json`), `RequiredConfigItem`/`required_config`, e para
  `MonitorStatusItem`/`kind: "monitor-status-grid"` contra `protocol/schema/v0.2/widget.schema.json`,
  em `tests/contract/` (depende de T008, T009, T010)
- [ ] T045 [P] `farol-core`: teste de unidade da renderização do novo `kind` de widget
  (`monitor-status-grid`, incluindo o estado explícito de erro/não-configurado distinto de lista
  vazia) e do formulário de setup (`SetupForm`, T035), em `tests/unit/` (depende de T033, T035)
- [ ] T046 [P] plugin `uptime-kuma`: testes `pytest` — parsing Prometheus com corpos sintéticos
  válidos e inválidos (`metrics_parser.py`), mapeamento de status FR-012, lógica de cache/erro do
  poller mockando a chamada HTTP e a **leitura de variável de ambiente** (`config.py`/`secrets.py`,
  T021/T022 — substitui o mock de `op read` de versões anteriores deste documento), em
  `tests/unit/test_uptime_kuma_*.py`

---

## Débito técnico (issues abertas)

Cada task abaixo referencia uma issue já existente no tracker do projeto (GitHub) — nenhuma issue
nova é criada por este documento. Critério de pronto de cada task inclui fechar a issue
correspondente (`gh issue close <N> --comment "..."`) — instrução registrada aqui, **não executada**
nesta sessão de planejamento. T047–T049 são independentes entre si e da feature 002 (podem rodar a
qualquer momento, inclusive antes da Fase 1); T050 depende do protocolo `"0.2"` desta feature.

- [ ] T047 **[Débito #1]** `rustfmt.toml` usa opções exclusivas de `nightly`
  (`format_code_in_doc_comments`, `wrap_comments`, `format_strings`, `format_macro_matchers`,
  `match_block_trailing_comma`) num toolchain `stable`. Correção: remover essas cinco opções de
  `rustfmt.toml`. Critério de pronto: `cargo fmt --check` roda sem erro de "unknown config option" em
  toolchain `stable`; fechar a issue #1 com `gh issue close 1 --comment "rustfmt.toml corrigido —
  opções exclusivas de nightly removidas"`.
- [ ] T048 **[Débito #2]** `Cargo.toml` do workspace não declara `resolver`, então `cargo` assume o
  resolver v1 apesar do `edition = "2021"` de cada crate membro. Correção: adicionar
  `resolver = "2"` em `[workspace]` no `Cargo.toml` raiz. Critério de pronto: `cargo metadata --format-version=1`
  confirma `resolver: "2"`; fechar a issue #2 com `gh issue close 2 --comment "Cargo.toml corrigido —
  resolver = \"2\" declarado em [workspace]"`.
- [ ] T049 **[Débito #3]** `plugins/git-local/scan.py:_ahead_behind` (linhas 74-99) cai em fallback
  silencioso `(0, 0)` quando o remote existe (`git remote` não-vazio) mas o branch atual não tem
  upstream de tracking configurado (`@{u}` falha) — esse caso não é o estado `no_remote` que o
  contrato já define, e o fallback mascara a diferença entre "0 ahead/0 behind de verdade" e "não dá
  pra saber". Correção: novo estado em `RemoteStatus` (ex. `{"kind": "no_upstream_tracking"}`),
  refletido em `protocol/schema/v0.1/widget.schema.json` (`RemoteStatus`, ainda em uso por `git-local`
  até a migração #4), no binding Rust (`crates/farol-protocol/src/messages.rs`, enum `RemoteStatus`) e
  na UI (`crates/farol-core/src/view.rs`, distinção visual do `no_remote` já renderizado). Critério de
  pronto: `_ahead_behind` retorna o novo estado em vez de `(0, 0)` quando `@{u}` falha com remote
  presente, testado com um repositório de fixture nessa condição; fechar a issue #3 com
  `gh issue close 3 --comment "RemoteStatus ganhou no_upstream_tracking — ahead/behind não confunde
  mais 'sem tracking' com '0 commits de diferença'"`.
- [ ] T050 **[Débito #4]** `plugins/git-local/main.py` declara `protocol_version = "0.1"` e
  `capabilities: {"capabilities": ["exec"]}` (formato antigo, `string[]`) — incompatível com um core
  em `"0.2"` (esta feature, T003–T019). Correção: atualizar `plugins/git-local/main.py` para declarar
  `protocol_version = "0.2"` e `capabilities.capabilities: [{"kind": "exec"}]` (novo formato
  estruturado, `research.md` D1 desta feature como especificação normativa); confirmar que nenhum
  outro campo do handshake de `git-local` precisa mudar (não deveria — D1 só afeta
  `CapabilityManifest`; `git-local` não tem segredo, então não precisa declarar `required_config`,
  D8). Critério de pronto: Cenário 8 de `quickstart.md` (T023), repetido após esta correção, mostra
  `git-local` em `PluginState::Ready` (não mais `Unavailable{VersionIncompatible}`) contra um core em
  `"0.2"`; fechar a issue #4 com `gh issue close 4 --comment "git-local migrado para protocol_version
  0.2 e Capability estruturada — Ready contra o core desta feature"`. Depende de T003–T013 (protocolo
  `"0.2"` completo + correções C1/H1/H2/H4 do lado do core).

---

## Dependencies & Execution Order

### Dependências entre fases

- **Setup (Fase 1)**: sem dependências — pode começar imediatamente.
- **Foundational 2a — Protocolo (D1, D8)**: depende da Fase 1. **Bloqueia** 2b e 2c, e
  transitivamente todo o resto — nenhuma linha de código de `farol-protocol`, extensão de
  `farol-core` ou do plugin `uptime-kuma` é escrita antes de `protocol/schema/v0.2/` existir.
- **Foundational 2b — Bindings Rust, correções C1–C3/H1–H4, infraestrutura D8**: depende de 2a.
  Internamente: T008→T009→T010 (mesmo arquivo `messages.rs`, sequencial); T011 depende de T004 e MUST
  landar junto de/antes de T008 entrar em uso; T012 depende de T003; T013 depende de T008; T014→T015
  (mesmo arquivo `main.rs`/`plugin_worker.rs`, sequencial); T016→T017 (módulo de configuração);
  T018 depende de T009, T014, T016, T017; T019 depende de T009, T018.
- **Foundational 2c — Esqueleto Python**: depende de 2a (T001). Paralelo a 2b — diretórios e
  linguagens distintas.
- **Validação Foundational (T023, Cenário 8)**: depende de T003, T008–T012, T014, T015 — não depende
  de 2c nem de nenhuma task do plugin `uptime-kuma`.
- **User Story 1 (Fase 3)**: depende da Fase 2 completa (incluindo T023). Nenhuma dependência de
  outra user story.
- **User Story 2 (Fase 4)**: depende da User Story 1 completa — dependência de produto explícita da
  spec (US2 "Depende da User Story 1 já estar funcionando"), não uma dependência técnica nova de
  arquivo: as tasks de US2 são só validação do que US1/Foundational já constroem.
- **Polish (Fase 5)**: depende de todas as user stories desejadas estarem completas; T043 (H3) depende
  especificamente de T008–T010.
- **Débito técnico**: T047–T049 são independentes entre si e de toda a feature 002 — podem rodar a
  qualquer momento. T050 depende de T003–T013.

### Dentro de cada user story

- US1: handshake (T024) e infraestrutura de leitura (T025–T027) antes do handler de `widget/get`
  (T028); lado core (T029–T035) depende de Foundational (T009/T010/T019) e forma uma cadeia
  parcialmente sequencial: T029→T031→T033→T034→T035 (arquivos `model.rs`→`update.rs`→`view.rs`,
  algumas sequenciais no mesmo arquivo), com T030→T032 (setup) intercalado antes de T035; validação
  (T036–T039) depende de toda a implementação da story.
- US2: nenhuma implementação nova — validação (T040–T041) depende de US1 completa.

### Oportunidades de paralelismo

- T001/T002 (Fase 1) podem rodar em paralelo entre si.
- T004–T007 (schemas JSON, Fase 2a) podem rodar em paralelo entre si, mas só depois de T003.
- **Depois que 2a termina**: T008 e T020–T022 (Python) podem rodar em paralelo — duas frentes sem
  arquivo compartilhado. T011–T019 (Rust, `farol-core`) dependem de T008/T009 antes de poder validar
  contra o novo `Capability`, mas T012 (H1) e T014 (C2) só dependem de T003 e podem começar assim que
  2a termina.
- Dentro de US1: T024–T026 (plugin) e T029 (core `model.rs`) podem todos rodar em paralelo entre si —
  dependem só de Foundational, arquivos e linguagens distintas. T027/T028 (plugin) são sequenciais
  entre si e a T025/T026/T024 respectivamente. T031–T035 (core) têm dependências sequenciais internas
  (ver acima), mas todo o bloco pode avançar em paralelo ao bloco T024–T028 do plugin.
- Fase 5: T042/T044/T045/T046 são paralelizáveis entre si; T043 precisa completar (ou pelo menos
  começar) antes de T044 tocar o mesmo crate `farol-protocol`.
- Débito técnico: T047/T048/T049 são paralelizáveis entre si e com qualquer outra fase.

---

## Parallel Example: Foundational 2b/2c (após protocolo 2a pronto)

```bash
# Duas frentes paralelas, sem arquivo compartilhado, ambas dependendo só de T003-T007:
Task: "Evoluir CapabilityManifest em crates/farol-protocol/src/messages.rs (T008)"
Task: "Loop de leitura/escrita NDJSON em plugins/uptime-kuma/main.py (T020)"
```

## Parallel Example: User Story 1

```bash
# Lado plugin e lado core, em paralelo:
Task: "Handler de handshake/hello em plugins/uptime-kuma/main.py (T024)"
Task: "Cliente HTTP com Basic Auth em plugins/uptime-kuma/metrics_client.py (T025)"
Task: "Adicionar MonitorWidgetViewModel em crates/farol-core/src/model.rs (T029)"
```

---

## Implementation Strategy

### MVP First (User Story 1 apenas)

1. Completar Fase 1: Setup (sem pré-requisito de ambiente externo nesta revisão).
2. Completar Fase 2 (2a protocolo → 2b/2c em paralelo → T023) — bloqueante, sem atalho; 2b inclui as
   correções CRITICAL/HIGH (C1–C3/H1–H4) e a infraestrutura de configuração/segredo (D8).
3. Completar Fase 3: User Story 1.
4. **PARAR e VALIDAR**: rodar Cenários 1, 2, 3 e 6 de `quickstart.md` (T036–T039) isoladamente.
5. Esse é o MVP que prova o segundo perfil de capacidade (rede) e o novo `kind` de widget declarativo
   com um consumidor real (Princípios III e IV da constitution), além da tela de setup como novo
   mecanismo de provisionamento de configuração/segredo compartilhado por design entre plugins
   futuros.

### Entrega Incremental

1. Setup + Foundational (2a → 2b/2c → T023) → protocolo `"0.2"` pronto, correções CRITICAL/HIGH
   aplicadas, infraestrutura de configuração/segredo pronta, quebra de `git-local` confirmada
   (débito técnico já rastreado como issue #4, T050).
2. User Story 1 → validar com Cenários 1–3, 6 → **MVP**.
3. User Story 2 → validar com Cenários 4–5 → resiliência sob rede degradada provada, sem
   implementação nova além do que US1/Foundational já construíram.
4. Polish → Cenário 7 (isolamento de falha herdado) + suíte de testes automatizados + correção H3.
5. Débito técnico (T047–T050) → pode ser conduzido em paralelo a qualquer momento, exceto T050
   (depende do protocolo `"0.2"` desta feature).
6. Cada story soma valor sem quebrar a anterior — critério de "feature 002 provada"
   (`quickstart.md` § final) é os 8 cenários passando.

### Estratégia de Equipe Paralela

Depois que a Fase 2 (protocolo + bindings + esqueletos + infraestrutura de configuração/segredo) está
pronta:

- Uma frente pode seguir em `crates/farol-core`/`crates/farol-protocol` (Rust) enquanto outra segue
  em `plugins/uptime-kuma` (Python) — sem colisão de arquivo, unidas apenas pelo contrato em
  `protocol/schema/v0.2/`.
- User Story 2 não tem frente própria de implementação (só validação) — pode ser conduzida por
  quem validou User Story 1, assim que ela fechar.
- Débito técnico #1/#2/#3 (T047–T049) pode ser conduzido por qualquer pessoa, a qualquer momento,
  inclusive antes desta feature começar — são independentes.

---

## Notes

- `[P]` = arquivos diferentes, sem dependência pendente.
- `[Story]` mapeia a task à user story correspondente para rastreabilidade.
- Nenhuma task desta lista cobre itens do `## Out of Scope` de `spec.md` (enforcement de allowlist
  de rede, qualquer ação de escrita/gerenciamento de monitores, outro plugin, migração de
  `git-local` — rastreada como débito técnico #4 acima, não como task de implementação desta
  feature —, workspaces, paleta de comandos, registry federado de plugins, empacotamento) — de
  propósito.
- `protocol/schema/v0.2/` (Fase 2a) é bloqueante para tudo o mais, por decisão D1 de `research.md`:
  nenhum binding Rust nem trecho do plugin Python nasce antes da especificação normativa existir.
- A migração de `plugins/git-local/` para `protocol_version = "0.2"` é rastreada como **issue #4**,
  já aberta no tracker do projeto, e registrada como task própria (T050) em § Débito técnico —
  **MUST** ser fechada antes de esta feature ser considerada encerrada (constitution v0.3.0,
  Governance).
- Verificar que cada cenário de `quickstart.md` (8 no total, renumerados nesta sessão) passa antes de
  considerar a story correspondente, ou a feature como um todo, encerrada.
- Parar em qualquer checkpoint para validar a story isoladamente antes de seguir para a próxima.
