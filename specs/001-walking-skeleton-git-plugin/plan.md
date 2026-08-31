# Implementation Plan: Walking Skeleton — Core, Protocolo de Plugin e Plugin de Referência Git Local

**Branch**: `001-walking-skeleton-git-plugin` | **Date**: 2026-08-31 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-walking-skeleton-git-plugin/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Fatia vertical fim-a-fim que prova, com um consumidor real, os contratos estruturais do Farol antes
de qualquer feature de produto: o core (Rust + iced) abre uma janela única, sobe UM plugin de
referência (`git-local`, Python) como processo filho, os dois negociam versão de protocolo num
handshake JSON-RPC sobre stdin/stdout, o plugin declara identidade, manifesto de capacidades
(`exec`), o widget que oferece (status-grid de repositórios git) e as ações que expõe (`git.fetch`
por repositório); o core renderiza os dados declarativos recebidos, atualiza-os em ciclo periódico,
permite disparar `git fetch` pela UI com o resultado refletido de volta, e sobrevive a crash/trava
do plugin sinalizando-o como indisponível sem cair.

Abordagem técnica (detalhada em `research.md`): protocolo definido por especificação agnóstica de
linguagem (JSON Schema + documento versionado, D1) com framing NDJSON justificado contra
`Content-Length` do LSP (D2); plugin de referência escrito em Python puro para provar
estruturalmente que nenhum consumidor precisa do crate Rust (D3); toda I/O de plugin roda dentro do
próprio executor tokio que o iced já embarca, nunca um segundo runtime (D4), isolada de
`update`/`view` por um worker `Subscription` + canal (D5); crash e trava são detectados por dois
mecanismos independentes — `child.wait()` e timeout de RPC (D6); versão do protocolo é
`MAJOR.MINOR` com regra de comparação que já prevê evolução pós-1.0 (D7); e o widget é atualizado
por polling core-iniciado, não push (D8).

## Technical Context

**Language/Version**: Rust (edition 2021, MSRV ≥ 1.75) para `farol-core` e `farol-protocol`;
Python 3.11+ (somente biblioteca padrão) para o plugin de referência `plugins/git-local` (D3).

**Primary Dependencies**: `iced` ~0.13 com feature `tokio` (executor async nativo — D4);
`tokio` (usado diretamente para `process::Command`, `io::{AsyncBufReadExt, BufReader}`,
`sync::mpsc`, `time::{timeout, interval}` dentro de `Task`/`Subscription` — nunca um
`tokio::Runtime` próprio); `serde`/`serde_json` (serialização das mensagens NDJSON — D2);
`thiserror` (tipos de erro do binding). Plugin `git-local`: nenhuma dependência externa
(`json`, `subprocess`, `sys`, `tomllib`, `pathlib` da stdlib).

**Storage**: N/A — sem persistência do lado do core nesta feature; o plugin lê seu próprio arquivo
de configuração TOML (`contracts/git-local-plugin.md`), somente leitura, sem escrita de estado.

**Testing**: `cargo test` para `farol-protocol` (testes de contrato: codec NDJSON, comparação de
versão D7, (de)serialização de cada forma de mensagem contra os exemplos de `contracts/`) e para
`farol-core` (testes de unidade da máquina de estados `PluginState`, `data-model.md` § 3); `pytest`
para o plugin `git-local` (varredura, mapeamento de `no_remote`, execução de `git fetch` mockada);
um harness de integração (script) que sobe `farol-core` real contra o plugin real, cobrindo os 7
cenários de `quickstart.md` — não implementado nesta fase de planejamento, apenas descrito.

**Target Platform**: Linux desktop (nativo, sem navegador — Princípio I), mesma máquina para core e
plugin (Assumptions da spec — sem execução remota nesta feature).

**Project Type**: Aplicação desktop nativa (GUI) com processo filho de plugin — não se encaixa nos
padrões "single project"/"web app"/"mobile" do template genérico; estrutura própria descrita abaixo
em § Project Structure.

**Performance Goals**: Sem meta numérica de throughput (não é serviço de rede). Meta qualitativa
central, derivada da restrição de arquitetura #4: `update`/`view` do iced nunca bloqueiam
esperando I/O de plugin — toda latência de plugin (varredura de filesystem, `git fetch` de rede)
fica isolada no worker (D5) e não deve introduzir frame drop perceptível na janela.

**Constraints**: Ciclo de refresh do widget = 30000ms default, ou o valor sugerido pelo plugin no
handshake (FR-011). Timeout de requisição JSON-RPC separado por classe de chamada (D6), não
normativo do protocolo em si (o protocolo exige apenas que o core não bloqueie indefinidamente, não
valores específicos): `RPC_TIMEOUT_CONTROL` (handshake e `widget/get`, IPC local sem rede) =
5000ms default; `RPC_TIMEOUT_ACTION` (`action/invoke`, pode ir à rede) = 120000ms default, ou o
valor sugerido pelo plugin por ação no handshake quando presente.

**Scale/Scope**: Um único plugin ativo por vez (Assumptions da spec); número de repositórios sob
`scan_root` limitado pelo que o usuário tem em disco — sem meta numérica; protocolo desenhado para
não assumir "um único plugin" como regra permanente (Assumptions), ainda que só um rode nesta
feature.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Avaliação contra os 7 Core Principles da constitution v0.2.0. Nenhum item abaixo mudou entre a
checagem pré-Fase 0 e a pós-Fase 1 (design não introduziu violação nova) — reportado uma única vez
com a coluna "Como esta feature cumpre".

| # | Princípio | Status | Como esta feature cumpre |
|---|---|---|---|
| I | Nativo e Sem Navegador | **PASS** | Core é `iced` (GUI nativa Rust), sem motor de navegador embutido. |
| II | Plugins como Processos Isolados via JSON-RPC | **PASS** | FR-002/FR-003 + D1–D6: processo filho separado, JSON-RPC sobre stdin/stdout, isolamento de crash (D6), e o plugin de referência é escrito em Python justamente para provar — não só declarar — que qualquer linguagem serve (D3), sem depender do crate Rust do core. |
| III | Widgets Declarativos, Core Renderiza | **PASS** | FR-009/FR-010; `data-model.md` § 1.3/1.5 define o widget como dado puro (`GitRepository[]`), sem nenhum campo de markup/desenho; `widget-protocol.md` reforça que `kind` é vocabulário fechado do core. |
| IV | Permissões Explícitas por Manifesto | **PASS, com deferral explícito e já previsto pela spec** | FR-007/FR-008 cobrem a declaração (manifesto com `exec`); o *enforcement* (allowlist de rede, keyring, sandbox) está no `Out of Scope` do `spec.md` por decisão já tomada na especificação, não uma lacuna descoberta agora no plano. O princípio não é violado — só parcialmente realizado nesta feature (a metade "declarar" sim, a metade "core não concede além do declarado" fica para a feature de sandbox futura, citada no Roadmap do README). |
| V | Espaços (Workspaces) por Contexto | **N/A nesta feature** | `Out of Scope` explícito do spec.md; nenhuma decisão deste plano assume workspace único como regra permanente do protocolo (o protocolo não tem conceito de "workspace" embutido, nem positivo nem negativo). |
| VI | Paleta de Comandos Universal | **N/A nesta feature, mas protocolo já é compatível** | Ctrl+K é `Out of Scope`, mas a decisão já fechada em Clarifications (Q1 do spec.md) — ações declaradas simetricamente a widgets, com id estável/rótulo/alvo (FR-006a) — é exatamente o que torna uma paleta de comandos futura possível sem retrofit: ela só precisa agregar `ActionDeclaration[]` de todo plugin ativo, estrutura que já existe desde este protocolo v0. |
| VII | Registry Federado sem Infra Própria | **N/A nesta feature** | `Out of Scope` explícito; nenhuma decisão deste plano assume um único plugin instalado manualmente como forma permanente de distribuição. |

**Gate**: PASS. Nenhuma violação — os itens "N/A" são deferrals já registrados no `Out of Scope` do
`spec.md` (não descobertos por este plano), e o item IV é um cumprimento parcial explicitamente
antecipado pela spec, não uma exceção não justificada. Nenhuma entrada precisa de
`Complexity Tracking`.

## Project Structure

### Documentation (this feature)

```text
specs/001-walking-skeleton-git-plugin/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output — decisões D1–D8
├── data-model.md        # Phase 1 output — entidades de protocolo + estado interno do core
├── quickstart.md        # Phase 1 output — 7 cenários de validação manual
├── contracts/            # Phase 1 output
│   ├── framing-and-versioning.md
│   ├── handshake.md
│   ├── widget-protocol.md
│   ├── action-protocol.md
│   ├── error-model.md
│   └── git-local-plugin.md
└── tasks.md              # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

Estrutura alvo para as fases de implementação (`/speckit-tasks` + `/speckit-implement` —
**nenhum arquivo abaixo é criado por este plano**; documentado aqui só para orientar as tasks
futuras, conforme decisões D1–D8 de `research.md`):

```text
Cargo.toml                        # workspace root (core + binding do protocolo)
crates/
├── farol-core/                   # binário: aplicação iced (Model-Update-View)
│   └── src/
│       ├── main.rs
│       ├── model.rs               # PluginConnection, PluginState, RepositoryViewModel (data-model.md §2)
│       ├── update.rs              # ciclo update — nunca bloqueia (D5)
│       ├── view.rs                # renderização do widget status-grid
│       └── plugin_worker.rs       # Subscription + canal mpsc (D5), spawn do processo filho (D4)
└── farol-protocol/                # binding Rust da especificação — NUNCA a fonte da verdade (D1)
    └── src/
        ├── framing.rs             # codec NDJSON (D2)
        ├── version.rs             # comparação MAJOR.MINOR (D7)
        └── messages.rs            # tipos gerados/mantidos a partir de protocol/schema/

protocol/                          # FONTE DA VERDADE do protocolo (D1) — agnóstica de linguagem
├── SPEC.md                        # prosa normativa (framing, handshake, versionamento, erros)
└── schema/
    └── v0.1/
        ├── handshake.schema.json
        ├── widget.schema.json
        ├── action.schema.json
        └── error.schema.json

plugins/
└── git-local/                     # plugin de referência — Python stdlib, SEM depender de farol-protocol (D3)
    ├── main.py                    # entrypoint: loop NDJSON sobre stdin/stdout
    ├── scan.py                    # varredura de scan_root, chamadas a `git`
    └── config.py                  # leitura de ~/.config/farol/plugins/git-local/config.toml

tests/
├── contract/                      # farol-protocol vs. protocol/schema/ (Rust) + testes de contrato do plugin (Python)
├── integration/                   # farol-core real + plugin real, cenários de quickstart.md
└── unit/                          # PluginState, comparação de versão, parsing de config
```

**Structure Decision**: Workspace Cargo com dois crates (`farol-core` binário, `farol-protocol`
biblioteca) mais um diretório `protocol/` na raiz do repositório — irmão de `crates/`, não dentro
dele — para deixar fisicamente visível que a especificação não é propriedade de nenhum crate Rust
(D1). O plugin de referência vive fora da árvore Cargo (`plugins/git-local/`, Python), reforçando a
mesma separação. `tests/` na raiz segue a convenção do template do spec-kit (`contract/`,
`integration/`, `unit/`), mapeada aqui para: contrato = validação de mensagens contra
`protocol/schema/`; integração = cenários ponta a ponta de `quickstart.md`; unidade = lógica pura
(máquina de estados, comparação de versão, parsing).

## Complexity Tracking

*Sem violações da constitution a justificar (ver Constitution Check acima — gate PASS sem
exceções).* As duas escolhas que adicionam superfície além do "caminho mais simples possível" —
(a) manter `protocol/` como artefato separado do crate Rust, e (b) escrever o plugin de referência
em Python em vez de Rust — não são desvios de princípio a justificar aqui: são, ao contrário, o
que torna os princípios II e a restrição de arquitetura #1 verificáveis em vez de apenas
declarados. A justificativa de cada uma está registrada como decisão em `research.md` (D1 e D3,
respectivamente), não como violação a compensar.
