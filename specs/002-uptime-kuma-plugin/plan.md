# Implementation Plan: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Branch**: `002-uptime-kuma-plugin` | **Date**: 2026-08-31 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-uptime-kuma-plugin/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Segunda fatia vertical do Farol depois do walking skeleton (feature 001): um segundo plugin de
referência, `uptime-kuma` (Python, somente leitura), que fala o mesmo protocolo JSON-RPC/NDJSON já
provado, mas exercita pela primeira vez um perfil de capacidade diferente — rede (allowlist de host)
e segredo (credencial via keyring do sistema), nenhum dos dois exercitado pelo `git-local`. O plugin
consulta periodicamente o endpoint `/metrics` (formato Prometheus) de uma instância Uptime Kuma
configurada pelo usuário, autenticando via HTTP Basic com uma credencial resolvida do **1Password**
(CLI `op`) — nunca de arquivo em texto plano — e devolve, como widget declarativo, a lista de
monitores (nome, status, tempo de resposta). Sem nenhuma ação (`action/invoke`): é leitura pura.

A decisão técnica central desta feature (`research.md` D1) é a evolução do `CapabilityManifest` de
`string[]` para uma lista de capacidades estruturadas por `kind` (`exec`/`network`/`secret`) — FR-005
já fixava esse *rumo*; este plano fixa o *schema exato*, o bump de `protocol_version` para `"0.2"`
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
`urllib.request` para HTTP+Basic Auth e um parser Prometheus mínimo próprio (D7); acesso à credencial
via **CLI `op` do 1Password** por `subprocess`, reaproveitando a capacidade `exec` já provada (D8);
e três novos `reason`s de erro de domínio Farol, reaproveitando a máquina de erro pontual de
`widget/get` já existente sem mudança de mecanismo (D9).

## Technical Context

**Language/Version**: Rust (edition 2021, MSRV ≥ 1.75) para `farol-core`/`farol-protocol`
(inalterado da feature 001 — nenhuma mudança de linguagem do core por causa desta feature); Python
3.11+ (somente biblioteca padrão) para o novo plugin de referência `plugins/uptime-kuma` (D7 —
reafirma D3 da feature 001).

**Primary Dependencies**: inalteradas do lado do core (`iced` ~0.13 feature `tokio`, `tokio`,
`serde`/`serde_json`, `thiserror` — D3/D4/D5 de `research.md` desta feature reafirmam D4/D5 da
feature 001 sem mudança). Plugin `uptime-kuma`: nenhuma dependência externa via `pip` — `json`,
`urllib.request`, `subprocess`, `threading`, `tomllib`, `pathlib`, `re`, `base64` da stdlib (D7/D8).
**Dependência de sistema nova** (binário externo, não pacote Python): **CLI `op` do 1Password**,
instalado e **autenticado** (sessão ativa) no ambiente onde `farol-core` roda — pré-requisito de
ambiente análogo ao binário `git` já assumido pela feature 001 para `git-local` (D8).

**Storage**: N/A do lado do core (inalterado). O plugin lê seu próprio arquivo de configuração TOML
(`base_url`, mesmo padrão XDG do `scan_root` de `git-local`) e resolve a credencial de autenticação
via `op read` no arranque do processo — nenhum dos dois é escrito pelo plugin, nenhum é lido/escrito
pelo core (`contracts/uptime-kuma-plugin.md`).

**Testing**: `cargo test` para `farol-protocol` (testes de contrato adicionais para o novo
`Capability` estruturado — serialização de `exec`/`network`/`secret`, e para o novo
`MonitorStatusItem`/`kind: "monitor-status-grid"`) e para `farol-core` (renderização do novo kind de
widget); `pytest` para o plugin `uptime-kuma` (parsing Prometheus com corpos sintéticos válidos e
inválidos, mapeamento de status FR-012, lógica de cache/erro do poller mockando a chamada HTTP e o
`op read`); harness de integração (script, não implementado nesta fase de planejamento) cobrindo os
cenários de `quickstart.md`.

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
| IV | Permissões Explícitas por Manifesto | **PASS, com cumprimento parcial já antecipado pelo spec — mesmo padrão da feature 001, agora estendido a `network` e `secret`** | Esta é a feature que primeiro exercita as três frentes do manifesto que a constitution já previa desde a v0.1: `exec` (reafirmado), **`network`** (allowlist de host — FR-005, D1: capacidade estruturada com `host`/`port`, nunca antes declarada por nenhum plugin) e **`secret`** (credencial via keyring — FR-019, D8: resolvida pelo *plugin* via CLI `op` do 1Password, nunca por arquivo em texto plano gerenciado pelo plugin). Como já era o caso para `exec` na feature 001, **apenas a metade "declarar" é cumprida nesta feature** — o *enforcement* (allowlist de rede de fato restringindo o host acessível, keyring lido/mediado pelo core) é `Out of Scope` explícito do `spec.md`, decisão já tomada na especificação, não uma lacuna descoberta agora. A leitura real da credencial pelo *plugin* (via `op`, D8) não é "enforcement do core" — é o plugin usando uma ferramenta de sistema operacional diretamente, exatamente como `git-local` já usa `git` via `exec`; o core continua sem ler, resolver ou mediar nenhum segredo. |
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

```text
Cargo.toml                          # workspace root (inalterado — nenhum crate novo)
crates/
├── farol-core/
│   └── src/
│       ├── main.rs
│       ├── model.rs                 # + MonitorStatusItem/MonitorWidgetState (novo, análogo a RepositoryViewModel)
│       ├── update.rs                # inalterado em arquitetura — só reage a mais um Message de widget
│       ├── view.rs                  # + renderização do kind "monitor-status-grid" (data-model.md § 3)
│       └── plugin_worker.rs         # inalterado — worker genérico já cobre qualquer plugin (D3)
└── farol-protocol/
    └── src/
        ├── framing.rs               # inalterado (D2)
        ├── version.rs               # inalterado em lógica — só o valor "0.2" passa a ser o suportado
        └── messages.rs              # CapabilityManifest: Vec<String> -> Vec<Capability> (D1);
                                      # + MonitorStatusItem, + kind "monitor-status-grid" no vocabulário
                                      # de renderização (não um tipo Rust novo por si só — ver data-model.md)

protocol/                            # FONTE DA VERDADE do protocolo (D1 da feature 001 — inalterado)
├── SPEC.md                          # título/versão passam a descrever "0.2"; §6.3/§10 ganham a nova
│                                     # forma de CapabilityManifest e os novos error reasons
└── schema/
    ├── v0.1/                        # RETIDO como registro histórico — não é mais a versão corrente
    │   └── *.schema.json            # (inalterado nesta feature — não editado)
    └── v0.2/                        # NOVO diretório — sibling de v0.1/, mesma convenção de nomeação
        ├── handshake.schema.json    # CapabilityManifest com Capability[] estruturada (D1);
        │                            # WidgetDeclaration/ActionDeclaration inalterados em forma
        ├── widget.schema.json       # + MonitorStatusItem, WidgetItem existente inalterado (D4)
        ├── action.schema.json       # inalterado em forma (copiado/referenciado — ainda usado pelo
        │                            # git-local futuro migrado, que continua tendo ação de fetch)
        └── error.schema.json        # descrição/catálogo textual + novos reasons -32005..-32007 (D9);
                                      # forma do ErrorObject em si inalterada

plugins/
├── git-local/                       # INALTERADO por esta feature — mas incompatível com um core em
│                                     # "0.2" até ser migrado (débito técnico registrado, D1/§ Complexity
│                                     # Tracking) — migração é OUTRA feature/task, não criada aqui.
└── uptime-kuma/                     # NOVO plugin de referência — Python stdlib, sem depender de
    ├── main.py                      # farol-protocol (mesmo princípio de D3 da feature 001)
    ├── config.py                    # leitura de ~/.config/farol/plugins/uptime-kuma/config.toml (base_url)
    ├── secrets.py                   # `op read "op://.../.../..."` via subprocess (D8)
    ├── metrics_client.py            # urllib.request + Basic Auth contra ${base_url}/metrics (D7)
    ├── metrics_parser.py            # parser Prometheus mínimo — monitor_status/monitor_response_time (D7)
    └── poller.py                    # thread de background + cache lock-guarded (D6, resolve FR-010)

tests/
├── contract/                        # + testes de farol-protocol para Capability[] e MonitorStatusItem;
│                                     # + testes de contrato do plugin uptime-kuma (Python)
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
| Plugin `git-local` (feature 001) declara `capabilities: {"capabilities": ["exec"]}` (formato antigo, `string[]`) sob `protocol_version = "0.1"`. Um core migrado para `"0.2"` (esta feature, D1) recusa esse plugin via `Unavailable{VersionIncompatible}` — `git-local` **para de funcionar** até ser atualizado para declarar `protocol_version = "0.2"` e `capabilities.capabilities` no novo formato estruturado (`[{"kind": "exec"}]`). | A task que gerou este plano restringe explicitamente esta sessão a **planejamento da feature 002** — tocar em `plugins/git-local/` está fora do escopo autorizado aqui, e migrar um plugin já implementado é trabalho de *implementação*, não de *planejamento*. Redesenhar o `CapabilityManifest` para aceitar os dois formatos simultaneamente foi avaliado e rejeitado em `research.md` D1 (complexidade permanente por um benefício que a própria convenção `MAJOR == 0` do protocolo já diz não ser esperado). | Pela regra "Dívida técnica rastreável" (constitution v0.3.0, Governance): **MUST** virar uma issue no tracker do projeto (GitHub Issues) — não criada por este plano (fora do escopo explícito desta sessão) — antes de a feature 002 ser considerada encerrada. A issue precisa cobrir, no mínimo: (a) atualizar `plugins/git-local/main.py` para declarar `protocol_version = "0.2"` e o novo formato de `capabilities`; (b) confirmar que nenhum outro campo do handshake de `git-local` precisa mudar (não deveria — D1 só afeta `CapabilityManifest`); (c) referenciar `research.md` D1 desta feature como a especificação normativa do novo formato. |

