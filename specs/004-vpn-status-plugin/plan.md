# Implementation Plan: Plugin de Status de VPN (openfortivpn-gui)

**Branch**: `004-vpn-status-plugin` (trabalho até aqui feito direto em `main`, mesmo padrão das
features 001-003 deste repositório — sem branch de feature separada)

**Date**: 2026-09-03

**Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/004-vpn-status-plugin/spec.md`

## Summary

Plugin de referência `openfortivpn-vpn` que expõe o estado da conexão VPN gerenciada pelo
`openfortivpn-gui` (desconectado/conectando/conectado, perfil ativo, perfis disponíveis, duração da
sessão) como um widget declarativo do core, e permite conectar/desconectar diretamente pelo widget
— sem duplicar lógica de conexão, apenas invocando a CLI já exposta por aquele projeto
(`openfortivpn-gui status|connect|disconnect --json`, contrato em
`../../../openfortivpn-gui/specs/001-add-cli-interface/contracts/`). Segue o mesmo modelo de plugin
JSON-RPC dos plugins existentes (`git-local`, `uptime-kuma`): processo Python independente,
protocolo `farol-protocol` sobre stdin/stdout.

Abordagem técnica: extensão aditiva do protocolo (`0.2` → `0.3`) — novo `kind` de widget
`"vpn-status"`/`VpnStatusItem`, e generalização de `ActionInvokeResult` para um `oneOf` que também
aceita o novo campo `vpn_status` ao lado do `repo` já existente — sem quebrar o formato de wire já
emitido por `git-local`/`uptime-kuma`. Como a série `0.x` do protocolo exige igualdade exata de
versão (`ProtocolVersion::is_compatible_with`, `crates/farol-protocol/src/version.rs`), os dois
plugins existentes precisam, mesmo assim, atualizar sua constante `PROTOCOL_VERSION` para `"0.3"`
como parte desta mesma feature (decisão D2 de `research.md` — aprendizado da dívida técnica #4 da
feature 002, que deixou essa migração pendente e virou issue separada; não repetir o padrão).

O plugin em si não roda um poller em background: como `git-local`, ele reconsulta o estado a cada
`widget/get` chamando `openfortivpn-gui status --json` de forma síncrona (chamada local, sem
latência de rede a esconder) — sem cache entre chamadas.

## Technical Context

**Language/Version**: Rust (core, `crates/farol-core`/`crates/farol-protocol`, mesma toolchain das
features 001-003) + Python 3 stdlib (plugin `openfortivpn-vpn`, mesmo padrão de `git-local`/
`uptime-kuma` — sem dependências externas, só `json`/`sys`/`subprocess`/`shutil`).

**Primary Dependencies**: `iced 0.14` (core, já em uso); nenhuma dependência nova. O plugin invoca o
binário `openfortivpn-gui` via `subprocess` — não importa nenhum módulo daquele projeto.

**Storage**: N/A — plugin não persiste nada; todo estado de VPN vem da CLI a cada chamada.

**Testing**: `cargo test --workspace` (core/protocolo, mesmo padrão das features anteriores,
incluindo Camada 1 e-2e via `iced_test::Emulator` e o gerador de casos de borda de contrato);
`python3 -m unittest discover -p "test_*.py"` dentro de `plugins/openfortivpn-vpn/` (mesmo padrão de
`uptime-kuma`, testes colocados junto do código); harness de smoke `tests/integration/harness.sh`
(Camada 2) estendido para confirmar que `openfortivpn-vpn` também chega a `Ready`.

**Target Platform**: Linux desktop (mesmo do restante do Farol) — depende de `openfortivpn-gui` já
instalado e no `PATH` da mesma máquina (Assumption do `spec.md`).

**Project Type**: Desktop app + plugin de referência (mesma forma das features 001/002 — não é
"web application" nem "mobile").

**Performance Goals**: Sem meta numérica nova além do já implícito no modelo de polling existente
(refresh a cada `suggested_refresh_interval_ms`, default 30000ms se ausente); a chamada síncrona
`status --json` é local e rápida, sem orçamento de latência de rede a considerar.

**Constraints**: `connect` da CLI bloqueia até ~20s (timeout default documentado no contrato
daquele projeto) — a ação `vpn.connect` MUST declarar `timeout_hint_ms` compatível (> 20s) para não
disparar `action_timeout` sintetizado pelo core antes da CLI resolver por conta própria.

**Scale/Scope**: Uma única sessão VPN por vez (Assumption do `spec.md`) — não há necessidade de
modelar múltiplas conexões simultâneas.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Princípio | Avaliação |
|---|---|
| I. Nativo e sem navegador | N/A a esta feature — não introduz nenhum motor de navegador. |
| II. Plugins como processos isolados via JSON-RPC | Satisfeito — `openfortivpn-vpn` é mais um processo Python independente falando o mesmo protocolo, mesmo padrão de `git-local`/`uptime-kuma`. |
| III. Widgets declarativos, core renderiza | Satisfeito — `VpnStatusItem` é dado declarativo (estado, perfil, sessão, ações); a `view.rs` do core decide como desenhar (seletor de perfil, botões conectar/desconectar), o plugin nunca emite markup. |
| IV. Permissões explícitas por manifesto | Satisfeito — o plugin declara `capabilities: [{"kind": "exec"}]` (mesma capability já usada por `git-local` para o binário `git`) para justificar invocar `openfortivpn-gui`; sem segredo algum a gerenciar (`required_config: []`, sem tela de setup — CLI não pede credencial ao Farol, ver Assumptions do `spec.md`); sem capability de rede declarada pelo plugin (a VPN em si é responsabilidade da CLI, não do plugin, mesmo raciocínio já aceito para `git-local`/`git fetch`). |
| V. Espaços por contexto | N/A a esta feature — plugin é ativado/desativado por espaço como qualquer outro, sem lógica nova. |
| VI. Paleta de comandos universal | Satisfeito "de graça" — `vpn.connect`/`vpn.disconnect` são `ActionDeclaration`s como qualquer outra; a paleta de comandos já agrega toda ação declarada por todo plugin ativo, sem trabalho adicional de design nesta feature. |
| VII. Registry federado sem infra própria | N/A a esta feature — plugin entra pelo mesmo registro hardcoded (`plugin_worker::known_plugins()`) das features anteriores; descoberta via GitHub é fora de escopo do projeto neste estágio. |

Nenhuma violação — sem necessidade de `Complexity Tracking`.

## Project Structure

### Documentation (this feature)

```text
specs/004-vpn-status-plugin/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md         # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
│   ├── protocol-delta-v0.3.md
│   └── openfortivpn-cli-mapping.md
├── checklists/
│   └── requirements.md
└── tasks.md              # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
protocol/
├── SPEC.md                       # +§ novo kind "vpn-status", ActionInvokeResult vira oneOf (0.3)
└── schema/
    ├── v0.2/                     # congelado, inalterado (histórico, mesmo tratamento de v0.1)
    └── v0.3/                     # novo — widget.schema.json/action.schema.json/error.schema.json
                                   # com as adições; handshake.schema.json sem mudança de forma
                                   # (copiado, só o comentário de versão atualiza)

crates/
├── farol-protocol/
│   └── src/
│       ├── messages.rs           # +VpnConnectionState, VpnProfile, VpnStatusItem;
│       │                         # WidgetItems ganha variante Vpn(Vec<VpnStatusItem>);
│       │                         # ActionInvokeResult vira enum untagged (Git{repo}/Vpn{vpn_status})
│       └── version.rs            # PROTOCOL_VERSION continua "MAJOR.MINOR" livre de mudança de forma
└── farol-core/
    └── src/
        ├── model.rs               # +VpnWidgetViewModel (status/connect_in_flight/
        │                          # disconnect_in_flight/last_error), +PluginConnection::vpn_widget
        ├── update.rs              # normalize_widget_items reconhece "vpn-status"; handle_widget_
        │                          # outcome/handle_action_outcome roteiam para vpn_widget
        ├── view.rs                # renderização do widget: estado, perfil ativo, tempo de sessão,
        │                          # seletor de perfil (US1/US3), botões conectar/desconectar (US2)
        ├── plugin_worker.rs       # known_plugins() ganha entrada "openfortivpn-vpn"
        └── snapshots/             # +cenário(s) de snapshot visual para o novo widget

plugins/
├── git-local/main.py             # PROTOCOL_VERSION "0.2" → "0.3" (mecânico, sem mudança de wire)
├── uptime-kuma/main.py           # idem
└── openfortivpn-vpn/             # NOVO plugin de referência
    ├── main.py                   # dispatch handshake/widget/action, mesmo esqueleto de git-local
    ├── vpn_cli.py                # wrapper subprocess sobre `openfortivpn-gui status|connect|
    │                             # disconnect --json`, tradução ErrorPayload -> ErrorObject
    ├── pyproject.toml            # config ruff, mesmo padrão dos outros dois plugins
    └── test_vpn_cli.py           # testes colocados junto do código (padrão uptime-kuma, D7)

tests/
└── integration/harness.sh        # Camada 2 — mais uma condição: openfortivpn-vpn chega a Ready
```

**Structure Decision**: mesma forma de projeto das features 001-003 (core Rust em `crates/`,
protocolo compartilhado em `protocol/`, plugins Python independentes em `plugins/<nome>/`) — esta
feature não introduz nenhum diretório de topo novo, só um novo plugin e uma nova versão de schema
lado a lado com as anteriores (congeladas).

## Complexity Tracking

> Sem violações de Constitution Check — seção não aplicável.
