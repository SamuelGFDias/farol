# Implementation Plan: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Branch**: `002-uptime-kuma-plugin` | **Date**: 2026-08-31 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-uptime-kuma-plugin/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Segunda fatia vertical do Farol depois do walking skeleton (feature 001): um segundo plugin de
referência, `uptime-kuma` (Python, somente leitura), que fala o mesmo protocolo JSON-RPC/NDJSON já
provado, mas exercita pela primeira vez um perfil de capacidade diferente — rede (allowlist de host)
e configuração/segredo de usuário (URL base e credencial de API), nenhum dos dois exercitado pelo
`git-local`. O plugin consulta periodicamente o endpoint `/metrics` (formato Prometheus) de uma
instância Uptime Kuma configurada pelo usuário, autenticando via HTTP Basic com uma API Key, e
devolve, como widget declarativo, a lista de monitores (nome, status, tempo de resposta). Sem nenhuma
ação (`action/invoke`): é leitura pura.

**Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: a versão original deste plano descrevia a
credencial como resolvida pelo *plugin*, via CLI `op` do 1Password. Essa decisão foi substituída — o
**core**, não o plugin, gerencia armazenamento seguro de configuração (secreta ou não) e o
provisionamento acontece por uma tela dentro do próprio Farol (`iced`), não por CLI externo. O plugin
declara, no handshake, quais variáveis precisa (`required_config`, novo campo de protocolo) e as lê
como variável de ambiente injetada pelo core no spawn do processo — nunca de arquivo, nunca via
`subprocess`. Ver `research.md` D8 (revisado) para o desenho completo; este documento foi corrigido
em todos os pontos que descreviam o mecanismo anterior (§ Technical Context, § Constitution Check,
§ Project Structure).

A decisão técnica central desta feature (`research.md` D1) é a evolução do `CapabilityManifest` de
`string[]` para uma lista de capacidades estruturadas por `kind` (`exec`/`network` — **revisão desta
sessão**: `secret` não faz mais parte deste vocabulário, ver D8/D1 revisados acima; FR-005 previa
`secret` como exemplo ilustrativo do rumo, não como schema fechado — decisão do usuário nesta sessão
substituiu esse mecanismo por `required_config`, ver Riscos/pendências no retorno desta sessão de
auditoria) — FR-005 já fixava o *rumo* de capacidades estruturadas; este plano fixa o *schema exato*,
o bump de `protocol_version` para `"0.2"`
(MINOR, sob o regime `MAJOR == 0` de igualdade exata já estabelecido pela feature 001), e registra
explicitamente a consequência: **o plugin `git-local` (feature 001), inalterado, deixa de funcionar**
sob um core em `"0.2"` — quebra limpa via o mecanismo `Unavailable{VersionIncompatible}` já
construído (não um crash), mas uma perda de funcionalidade real e imediata. A migração de
`git-local` para o novo schema é débito técnico deliberadamente adiado (fora do escopo desta
feature), e — pela regra "Dívida técnica rastreável" da constitution v0.3.0 (Governance) — **MUST
virar uma issue no GitHub antes desta feature ser considerada encerrada**; essa issue não é criada
por este plano (fora do escopo desta sessão de planejamento).

Demais decisões técnicas (detalhadas em `research.md`): framing NDJSON e modelo de concorrência do
core reafirmados sem mudança (D2/D3, herdados de D2/D4/D5 da feature 001); um novo `kind` de widget
`monitor-status-grid` em vez de reaproveitar `status-grid` como hoje schemado, por incompatibilidade
estrutural de item (D4); os orçamentos `RPC_TIMEOUT_CONTROL`/`RPC_TIMEOUT_ACTION` inalterados, com um
terceiro orçamento — o timeout HTTP interno do plugin (10s) — explicitamente desacoplado e não
normativo do protocolo (D5); uma thread de polling em background dentro do plugin, servindo
`widget/get` a partir de um cache em memória, para nunca bloquear a resposta de controle na latência
de rede (D6, resolve FR-010); Python 3 stdlib mantido como linguagem do plugin de referência, com
`urllib.request` para HTTP+Basic Auth e um parser Prometheus mínimo próprio (D7); configuração e
segredo de usuário (`base_url`/API Key) declarados pelo plugin via um novo campo de protocolo,
`required_config`, e geridos inteiramente pelo **core** — armazenamento em `config.toml`/`secrets.toml`
(`0600`), injeção como variável de ambiente no spawn, provisionamento por uma tela dentro do próprio
Farol (`iced`) — revisão desta sessão que substitui a decisão original de CLI `op`/1Password (D8);
e três novos `reason`s de erro de domínio Farol, reaproveitando a máquina de erro pontual de
`widget/get` já existente sem mudança de mecanismo (D9, também revisado — `not_configured` passa a
ser salvaguarda, não caminho primário).

## Technical Context

**Language/Version**: Rust (edition 2021, MSRV ≥ 1.75) para `farol-core`/`farol-protocol`
(inalterado da feature 001 — nenhuma mudança de linguagem do core por causa desta feature); Python
3.11+ (somente biblioteca padrão) para o novo plugin de referência `plugins/uptime-kuma` (D7 —
reafirma D3 da feature 001).

**Primary Dependencies**: do lado do core (`iced` ~0.13 feature `tokio`, `tokio`,
`serde`/`serde_json`, `thiserror` — D3/D4/D5 de `research.md` desta feature reafirmam D4/D5 da
feature 001), **estendido** por esta revisão: o core (Rust) passa a fazer parsing/escrita de TOML
(`config.toml`) e escrita de um novo arquivo `secrets.toml` com permissão `0600` (D8 revisado) —
mesma dependência de crate já implícita em qualquer leitor de TOML do lado Rust, sem crate novo além
do necessário para isso (ex.: `toml`, já um candidato natural, decisão de implementação). Plugin
`uptime-kuma`: nenhuma dependência externa via `pip` — `json`, `urllib.request`, `threading`,
`pathlib`, `re`, `base64` da stdlib (D7); **revisão desta sessão**: `subprocess`/`tomllib` **não são
mais necessários** do lado do plugin (D8 revisado — sem CLI `op`, sem parsing de TOML pelo plugin, só
`os.environ`). **Nenhuma dependência de sistema externa nova** (revisão desta sessão — a versão
anterior deste plano exigia o CLI `op` do 1Password instalado/autenticado; essa dependência foi
removida).

**Storage**: **revisado nesta sessão** — deixa de ser "N/A do lado do core". O core (Rust) passa a
gerenciar dois arquivos: `$XDG_CONFIG_HOME/farol/plugins/<nome>/config.toml` (configuração não-secreta
por plugin, mesmo padrão já usado pelo `scan_root` de `git-local`, agora também escrito pela tela de
setup) e `$XDG_CONFIG_HOME/farol/secrets.toml` (configuração secreta de todos os plugins, permissão
`0600`, escrito exclusivamente pelo core) — ambos escritos na submissão do formulário de setup (D8 de
`research.md`) e lidos no spawn de cada processo filho para injeção como variável de ambiente
(`Command::env`). **O plugin não lê nenhum dos dois arquivos** — só variáveis de ambiente já
resolvidas pelo core (`contracts/uptime-kuma-plugin.md`).

**Testing**: `cargo test` para `farol-protocol` (testes de contrato adicionais para o novo
`Capability` estruturado — serialização de `exec`/`network` — e para o novo
`MonitorStatusItem`/`kind: "monitor-status-grid"`, e para `RequiredConfigItem`/`required_config`) e
para `farol-core` (renderização do novo kind de widget, e — revisão desta sessão — o novo formulário
de setup construído a partir de `required_config`, e a resolução/injeção de `required_config` contra
`config.toml`/`secrets.toml` em `plugin_worker.rs`); `pytest` para o plugin `uptime-kuma` (parsing
Prometheus com corpos sintéticos válidos e inválidos, mapeamento de status FR-012, lógica de
cache/erro do poller mockando a chamada HTTP e a leitura de variável de ambiente — revisão desta
sessão, substitui o mock de `op read` de versões anteriores); harness de integração (script, não
implementado nesta fase de planejamento) cobrindo os cenários de `quickstart.md`.

**Target Platform**: Linux desktop nativo (Princípio I), mesma máquina para core e plugin
(Assumptions do spec — sem execução remota). A instância Uptime Kuma consultada roda em outra
máquina/rede — é o próprio alvo da nova capacidade de rede, não uma mudança de plataforma do Farol.

**Project Type**: Aplicação desktop nativa (GUI) com processo filho de plugin — mesmo tipo de projeto
da feature 001; nenhuma estrutura nova de projeto é introduzida, só um novo diretório de plugin
sibling a `plugins/git-local/` (ver § Project Structure).

**Performance Goals**: Mesma meta qualitativa da feature 001 (`update`/`view` do iced nunca bloqueiam
esperando I/O de plugin) — **estendida** por esta feature ao caso novo em que a fonte de dados do
plugin é, ela mesma, uma chamada de rede potencialmente lenta: o desenho do plugin (D6) garante que
essa latência nunca vaza para o orçamento `RPC_TIMEOUT_CONTROL` que o core já aplica a `widget/get`.

**Constraints**: Ciclo de refresh do widget = `suggested_refresh_interval_ms` declarado pelo plugin
(default 30000ms, FR-009 — mesmo mecanismo da feature 001, reafirmado). `RPC_TIMEOUT_CONTROL` (5s
default) aplicado a `handshake/hello` e `widget/get` deste plugin, sem exceção — é a restrição que
FR-010 exige respeitar apesar da fonte de dados ser rede (D5/D6). `RPC_TIMEOUT_ACTION` não se aplica
(FR-004, sem ações). **Novo, não normativo do protocolo**: timeout HTTP interno do plugin = 10s
(D5), deliberadamente menor que o intervalo de refresh, para nunca sobrepor duas tentativas de
leitura de rede (garantia estrutural de laço sequencial único, D6).

**Scale/Scope**: Uma única instância Uptime Kuma configurada e consultada por vez (Assumptions do
spec) — mesmo padrão "um alvo por vez" já usado por `scan_root` do `git-local`. Número de monitores
reportados pelo `/metrics` limitado pelo que a instância Uptime Kuma tem cadastrado — sem meta
numérica.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Avaliação contra os 7 Core Principles da constitution v0.3.0. Nenhum item mudou entre a checagem
pré-Fase 0 e a pós-Fase 1 (o design de Fase 1 não introduziu violação nova além do que já estava
identificado em Fase 0) — reportado uma única vez.

| # | Princípio | Status | Como esta feature cumpre |
|---|---|---|---|
| I | Nativo e Sem Navegador | **PASS** | Nenhuma mudança — core continua `iced` nativo; o plugin `uptime-kuma` roda como processo filho local, a instância Uptime Kuma consultada é um serviço de rede externo consumido via HTTP, não um motor de navegador embutido no Farol. |
| II | Plugins como Processos Isolados via JSON-RPC | **PASS** | FR-001/FR-002 + D2/D3 de `research.md`: mesmo modelo de processo filho + JSON-RPC/NDJSON já provado; o isolamento de crash/trava do plugin (D6 da feature 001) se aplica sem modificação a este plugin também — nenhuma reespecificação necessária (FR-018 do spec). |
| III | Widgets Declarativos, Core Renderiza | **PASS** | FR-013/FR-014; `data-model.md` § 1 define `MonitorStatusItem` como dado puro (nome/status/tempo de resposta), sem nenhum campo de markup/desenho — mesma disciplina do `GitRepository` da feature 001, aplicada a um novo `kind` (D4). |
| IV | Permissões Explícitas por Manifesto | **PASS, com cumprimento parcial já antecipado pelo spec — mesmo padrão da feature 001, agora estendido a `network`, e uma revisão de mecanismo para o segredo** | Esta é a feature que primeiro exercita a capacidade `network` do manifesto que a constitution já previa desde a v0.1 (allowlist de host — FR-005, D1: capacidade estruturada com `host`/`port`, nunca antes declarada por nenhum plugin). Como já era o caso para `exec` na feature 001, **a metade "declarar" é cumprida, sem enforcement** — nenhuma restrição de rede de fato é aplicada pelo core, `Out of Scope` explícito do `spec.md`, decisão já tomada na especificação. **Revisão desta sessão sobre o mecanismo de segredo**: a credencial de autenticação contra `/metrics` deixou de ser declarada como uma terceira capacidade (`kind: "secret"`) e resolvida pelo *plugin* via CLI `op`/1Password — agora é declarada via um novo campo de protocolo, `required_config` (irmão de `capabilities`), e o **core** passa a armazenar (`secrets.toml`, `0600`) e injetar a credencial como variável de ambiente do processo filho, provisionada por uma tela dentro do próprio Farol. Isto é uma mudança real de responsabilidade em relação ao desenho original: o core agora **lê e escreve** o segredo (nunca o expõe de volta ao plugin por outro canal que não a variável de ambiente daquele processo, e nunca o expõe na UI em texto plano) — distinto do padrão "puramente declarativo, sem enforcement" que `exec`/`network` mantêm. **Risco a registrar**: FR-019/`spec.md` fala em credencial vinda "do keyring do sistema" — o mecanismo desta revisão (arquivo `0600` gerido pelo core) não é um keyring do sistema operacional; ver `spec.md` § para decisão do usuário sobre se isso é uma reformulação aceitável de FR-019 ou uma contradição a resolver (não editado por este plano — fora do escopo desta sessão de correção de tasks/planning). |
| V | Espaços (Workspaces) por Contexto | **N/A nesta feature** | `Out of Scope` explícito do `spec.md` — nenhuma decisão deste plano assume workspace único como regra permanente. |
| VI | Paleta de Comandos Universal | **N/A nesta feature** | `Out of Scope` explícito — este plugin não declara nenhuma ação (FR-004), então não há nada de novo para uma paleta de comandos agregar; a compatibilidade estrutural futura já estabelecida pela feature 001 (ações declaradas simetricamente a widgets) permanece intacta, só não é exercitada aqui. |
| VII | Registry Federado sem Infra Própria | **N/A nesta feature** | `Out of Scope` explícito; nenhuma decisão deste plano assume instalação manual de plugin como forma permanente de distribuição. |

**Gate**: PASS, com uma obrigação de Governance explícita e não-opcional a satisfazer **antes de a
feature ser considerada encerrada** (não antes da implementação começar): a regra "Dívida técnica
rastreável" da constitution v0.3.0 exige que a quebra deliberada de `git-local` (consequência direta
de D1 — bump de `protocol_version` para `"0.2"`) vire uma issue no tracker do projeto. Este plano
registra a obrigação; **a criação da issue em si está fora do escopo desta sessão de planejamento**
(instrução explícita da task que gerou este plano) — quem executar a fase de implementação
(`/speckit-tasks` + `/speckit-implement`) deve garantir que essa issue exista, ou criá-la como parte
do trabalho de fechamento da feature, antes de declará-la concluída. Ver também § Complexity
Tracking abaixo, onde este item é registrado formalmente para não se perder.

## Project Structure

### Documentation (this feature)

```text
specs/002-uptime-kuma-plugin/
├── plan.md                          # This file (/speckit-plan command output)
├── research.md                      # Phase 0 output — decisões D1–D9
├── data-model.md                    # Phase 1 output — entidades de protocolo novas/estendidas
├── quickstart.md                    # Phase 1 output — cenários de validação manual
├── contracts/                        # Phase 1 output
│   ├── framing-and-versioning-delta.md
│   ├── handshake-delta.md
│   ├── widget-protocol-delta.md
│   ├── error-model-delta.md
│   └── uptime-kuma-plugin.md
└── tasks.md                          # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

Os arquivos `contracts/*-delta.md` documentam **apenas o que muda ou é adicionado** em relação aos
contratos já normativos da feature 001 (`specs/001-walking-skeleton-git-plugin/contracts/*.md`, que
por sua vez já viraram o `protocol/SPEC.md`/`protocol/schema/v0.1/` reais do repositório) — não
duplicam o que continua válido sem mudança (framing, sequenciamento de handshake, orçamentos de
timeout de controle). `uptime-kuma-plugin.md` é o análogo, específico deste plugin, de
`git-local-plugin.md` da feature 001.

### Source Code (repository root)

Estrutura alvo para as fases de implementação (`/speckit-tasks` + `/speckit-implement` — **nenhum
arquivo abaixo é criado por este plano**; documentado aqui só para orientar as tasks futuras,
conforme decisões D1–D9 de `research.md`):

**Correção desta sessão (auditoria pós-plan)**: a árvore abaixo, em versões anteriores deste plano,
marcava `main.rs` e `plugin_worker.rs` como "inalterados" e `update.rs` como "inalterado em
arquitetura". Isso é **falso** e foi corrigido — ver anotações `[CORRIGIDO]` abaixo. Nenhum destes
três arquivos sai desta feature do jeito que estava na feature 001; a causa raiz é a mesma nos três
casos: o design original assumia uma única `PluginConnection`/comando de plugin hardcoded (herdado
do walking skeleton, nunca generalizado) e checagem de versão só depois de desserializar `Capability`
tipado — nenhum dos dois pressupostos sobrevive a esta feature.

```text
Cargo.toml                          # workspace root (inalterado — nenhum crate novo)
crates/
├── farol-core/
│   └── src/
│       ├── main.rs                  # [CORRIGIDO] MUDA — Farol.plugin (uma única PluginConnection,
│       │                            # main.rs:29-31 hoje) vira uma coleção (uma conexão por plugin
│       │                            # configurado: git-local + uptime-kuma), C2 do checklist de
│       │                            # auditoria; nenhuma task futura de US1/US2 é executável sem isso
│       ├── model.rs                 # + MonitorStatusItem/MonitorWidgetViewModel (novo, análogo a
│       │                            # RepositoryViewModel, data-model.md §3.1); + UnavailableReason::
│       │                            # NotConfigured e SetupForm (data-model.md §3.2, D8) — a única
│       │                            # variante de Unavailable que não é terminal nesta feature
│       ├── update.rs                # MUDA, não só "reage a mais um Message" — merge_widget_items
│       │                            # (update.rs:291-306) hoje tipado para Vec<farol_protocol::
│       │                            # WidgetItem> precisa aceitar também MonitorStatusItem (C3);
│       │                            # + mensagens/lógica do formulário de setup (SetupFieldChanged/
│       │                            # SetupSubmitted, D8); os 3 literais `CapabilityManifest {
│       │                            # capabilities: vec!["exec".to_string()] }` (update.rs:319-320,
│       │                            # 352-353, 638-639) quebram de compilar com Capability
│       │                            # estruturado (T009) e precisam de correção própria (H2)
│       ├── view.rs                  # + renderização do kind "monitor-status-grid" (data-model.md § 3);
│       │                            # + renderização do formulário de setup quando PluginState =
│       │                            # Unavailable{NotConfigured} (D8); `identity.capabilities.
│       │                            # capabilities.join(", ")` (view.rs:60-63) só compila hoje porque
│       │                            # capabilities é Vec<String> — quebra com Capability estruturado
│       │                            # (H2)
│       └── plugin_worker.rs         # [CORRIGIDO] MUDA — hoje PLUGIN_COMMAND/PLUGIN_ARGS
│                                     # (plugin_worker.rs:74,80) são constantes hardcoded para
│                                     # `python3 plugins/git-local/main.py`; precisam virar parâmetro
│                                     # por conexão (C2); a checagem de protocol_version acontece hoje
│                                     # (plugin_worker.rs:296-325) só DEPOIS de desserializar a
│                                     # resposta inteira como HandshakeHelloResponse tipado — com
│                                     # Capability tagueado por kind (T009), a resposta v0.1 do
│                                     # git-local (`capabilities: ["exec"]`, string) falha a
│                                     # desserialização antes da checagem de versão rodar, produzindo
│                                     # Unavailable{Unresponsive} em vez de Unavailable
│                                     # {VersionIncompatible} (C1) — precisa de uma checagem prévia
│                                     # (probe via serde_json::Value só do campo protocol_version)
│                                     # antes da desserialização tipada; + resolução/injeção de
│                                     # required_config como variável de ambiente no spawn (D8)
└── farol-protocol/
    └── src/
        ├── framing.rs               # inalterado (D2)
        ├── version.rs               # inalterado em lógica — só o valor "0.2" passa a ser o suportado;
        │                            # [CORRIGIDO] a constante que hoje carrega a versão suportada pelo
        │                            # core (`CORE_PROTOCOL_VERSION`) vive em
        │                            # `farol-core/src/plugin_worker.rs:100`, não em `version.rs`
        │                            # (H1 — versões anteriores deste plano/tasks.md apontavam errado)
        └── messages.rs              # CapabilityManifest: Vec<String> -> Vec<Capability> (D1, sem
                                      # kind "secret" — D8 revisado); + RequiredConfigItem, + campo
                                      # required_config em HandshakeHelloResult (D8); + MonitorStatusItem,
                                      # + kind "monitor-status-grid"; WidgetGetResult.items MUDA de
                                      # Vec<WidgetItem> fixo para união discriminada (C3 — correção de
                                      # versões anteriores deste plano/data-model.md, que descreviam
                                      # isso como "não muda de forma")

protocol/                            # FONTE DA VERDADE do protocolo (D1 da feature 001 — inalterado)
├── SPEC.md                          # título/versão passam a descrever "0.2"; §6.3/§10 ganham a nova
│                                     # forma de CapabilityManifest, o campo required_config e os
│                                     # novos error reasons
└── schema/
    ├── v0.1/                        # RETIDO como registro histórico — não é mais a versão corrente
    │   └── *.schema.json            # (inalterado nesta feature — não editado)
    └── v0.2/                        # NOVO diretório — sibling de v0.1/, mesma convenção de nomeação;
                                      # [CORRIGIDO] $id de cada schema aponta para /v0.2/ (não copiar o
                                      # $id absoluto de v0.1/), e todo $ref cross-arquivo aponta para um
                                      # arquivo-irmão em v0.2/ (nunca para v0.1/) — v0.1/ usa $id/$ref
                                      # absolutos versionados (ex.: widget.schema.json:122 referencia
                                      # handshake.schema.json v0.1) que colidiriam no registry de
                                      # validação de contract_schema_validation.rs se copiados sem
                                      # ajuste (H4)
        ├── handshake.schema.json    # CapabilityManifest com Capability[] estruturada, sem kind
        │                            # "secret" (D1/D8); + required_config: RequiredConfigItem[]
        │                            # (irmão de capabilities/widgets/actions, D8); WidgetDeclaration/
        │                            # ActionDeclaration inalterados em forma
        ├── widget.schema.json       # + MonitorStatusItem, WidgetItem existente inalterado (D4)
        ├── action.schema.json       # inalterado em forma (copiado/referenciado — ainda usado pelo
        │                            # git-local futuro migrado, que continua tendo ação de fetch)
        └── error.schema.json        # descrição/catálogo textual + novos reasons -32005..-32007 (D9,
                                      # not_configured agora descrito como salvaguarda, ver D8/D9
                                      # revisados); forma do ErrorObject em si inalterada

plugins/
├── git-local/                       # INALTERADO por esta feature — mas incompatível com um core em
│                                     # "0.2" até ser migrado (débito técnico #4 no tracker do projeto,
│                                     # já aberto — ver "Débito técnico" em tasks.md) — migração é
│                                     # tarefa própria dentro do débito técnico, não desta feature.
└── uptime-kuma/                     # NOVO plugin de referência — Python stdlib
    ├── main.py                      # farol-protocol (mesmo princípio de D3 da feature 001); handler
    │                                # de handshake declara required_config sempre, capabilities só
    │                                # com network (sem exec — D8 revisado, sem chamada a binário
    │                                # externo)
    ├── config.py                    # [CORRIGIDO] deixa de fazer parsing de TOML — lê
    │                                # os.environ.get() para base_url/api_key, convenção
    │                                # FAROL_PLUGIN_UPTIME_KUMA_<NAME> (D8 revisado)
    ├── secrets.py                   # [CORRIGIDO] deixa de invocar `op` via subprocess — mesmo
    │                                # mecanismo de leitura de ambiente de config.py; T013/T014 podem
    │                                # colapsar numa única task (ver tasks.md)
    ├── metrics_client.py            # urllib.request + Basic Auth contra ${base_url}/metrics (D7)
    ├── metrics_parser.py            # parser Prometheus mínimo — monitor_status/monitor_response_time (D7)
    └── poller.py                    # thread de background + cache lock-guarded (D6, resolve FR-010)

tests/
├── contract/                        # + testes de farol-protocol para Capability[]/required_config e
│                                     # MonitorStatusItem; + testes de contrato do plugin uptime-kuma
│                                     # (Python); tests/contract_schema_validation.rs (crate
│                                     # farol-protocol) precisa de correção/aposentadoria própria (H3
│                                     # — valida hoje contra os 4 schemas v0.1 usando `capabilities:
│                                     # vec!["exec".to_string()]`, que deixa de compilar/fazer sentido
│                                     # depois de T009/T010)
├── integration/                     # + cenários de quickstart.md desta feature
└── unit/                            # + parser Prometheus, mapeamento de status, lógica de cache/poller
```

**Structure Decision**: Mesma estrutura de workspace da feature 001 (dois crates Rust + `protocol/`
irmão de `crates/`), sem nenhum crate novo — esta feature estende `farol-protocol` e `farol-core`
existentes, nunca cria um terceiro. O plugin novo é `plugins/uptime-kuma/`, sibling de
`plugins/git-local/`, seguindo a mesma separação "protocolo é agnóstico de linguagem, plugin de
referência não depende do crate Rust" (D1/D3 da feature 001, reafirmados sem mudança por D7 desta
feature). A única decisão de estrutura genuinamente nova é o diretório `protocol/schema/v0.2/`
sibling a `v0.1/` (em vez de sobrescrever `v0.1/` no lugar) — decisão de filesystem que segue
diretamente de D1 (bump de versão): manter `v0.1/` como registro histórico do que `git-local`
ainda fala até sua migração, sem apagar a evidência do formato antigo que uma issue de débito técnico
(§ Complexity Tracking) vai precisar referenciar.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

Nenhuma violação de princípio a justificar aqui (gate PASS, sem exceções silenciosas — ver
§ Constitution Check). O item abaixo **não é uma violação de princípio** — é uma obrigação de
**Governance** (regra "Dívida técnica rastreável" da constitution v0.3.0) que este plano precisa
deixar registrada de forma que não se perca entre esta sessão de planejamento e a fase de
implementação/fechamento da feature:

| Débito técnico identificado | Por que foi deliberadamente adiado nesta feature | Ação obrigatória antes de encerrar a feature |
|---|---|---|
| Plugin `git-local` (feature 001) declara `capabilities: {"capabilities": ["exec"]}` (formato antigo, `string[]`) sob `protocol_version = "0.1"`. Um core migrado para `"0.2"` (esta feature, D1) recusa esse plugin via `Unavailable{VersionIncompatible}` — `git-local` **para de funcionar** até ser atualizado para declarar `protocol_version = "0.2"` e `capabilities.capabilities` no novo formato estruturado (`[{"kind": "exec"}]`). | A task que gerou este plano restringe explicitamente esta sessão a **planejamento da feature 002** — tocar em `plugins/git-local/` está fora do escopo autorizado aqui, e migrar um plugin já implementado é trabalho de *implementação*, não de *planejamento*. Redesenhar o `CapabilityManifest` para aceitar os dois formatos simultaneamente foi avaliado e rejeitado em `research.md` D1 (complexidade permanente por um benefício que a própria convenção `MAJOR == 0` do protocolo já diz não ser esperado). | Pela regra "Dívida técnica rastreável" (constitution v0.3.0, Governance): **MUST** virar uma issue no tracker do projeto (GitHub Issues) antes de a feature 002 ser considerada encerrada. **Atualização de uma sessão posterior (auditoria pós-plan)**: essa issue **já existe** — issue #4 do tracker do projeto, "plugin git-local precisa migrar para protocol_version 0.2 (CapabilityManifest estruturado)", registrada como task própria em `tasks.md` § Débito técnico. A issue cobre, no mínimo: (a) atualizar `plugins/git-local/main.py` para declarar `protocol_version = "0.2"` e o novo formato de `capabilities`; (b) confirmar que nenhum outro campo do handshake de `git-local` precisa mudar (não deveria — D1 só afeta `CapabilityManifest`); (c) referenciar `research.md` D1 desta feature como a especificação normativa do novo formato. |

