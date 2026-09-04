# Implementation Plan: Sandbox de Plugins via Bubblewrap e Aplicação Real do Manifesto de Capacidades

**Branch**: `006-sandbox-permissoes-bubblewrap` | **Date**: 2026-09-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/006-sandbox-permissoes-bubblewrap/spec.md`

## Summary

O core passa a spawnar todo processo filho de plugin dentro de um sandbox `bwrap`, aplicando de
verdade a restrição de rede/filesystem/exec que hoje só é declarada e exibida na UI (Princípio IV da
constitution). Abordagem técnica (`research.md`, todas as decisões validadas empiricamente nesta
sessão com `bubblewrap 0.11.0` já instalado): um novo módulo `crates/farol-core/src/sandbox.rs`
resolve, por plugin, um `SandboxProfile` a partir do registro estático `known_plugins()` — não do
`CapabilityManifest` recebido em runtime (chicken-and-egg: o handshake só chega depois do spawn) — e
traduz esse perfil em argumentos concretos de `bwrap` que envolvem o `Command` já existente em
`plugin_worker.rs::worker()`. Rede é negada por padrão (`--unshare-all`) e liberada só quando
concedida (`--share-net` + binds de DNS/TLS); `exec` é mediado por visibilidade seletiva de
filesystem (não seccomp, débito rastreável); dois casos especiais nomeados (`git-local`/`scan_root`,
`docker-containers`/socket do Docker) recebem um bind extra cada. Achado de planejamento incorporado
à spec: `git-local` e `openfortivpn-vpn` corrigem seus manifestos para declarar `network`, que suas
ações reais (`git.fetch` contra remote real, `vpn.connect`) sempre precisaram sem nunca terem
declarado.

## Technical Context

**Language/Version**: Rust (workspace já em uso, `farol-core`/`farol-protocol`), Python 3 (os 4
plugins de referência, sem mudança de versão)

**Primary Dependencies**: `bwrap` (bubblewrap 0.11.0+, binário externo do sistema — sem crate Rust
nova; nenhuma dependência nova em `Cargo.toml`), `tokio::process::Command` (já em uso)

**Storage**: N/A — nenhuma mudança de armazenamento (`secrets_store.rs`/`config_store.rs`
inalterados, `research.md` D9)

**Testing**: `cargo test --workspace` (unidade pura de `sandbox.rs` + integração real com `bwrap`
instalado, `research.md` D11), `tests/integration/harness.sh` (regressão dos 4 plugins sob sandbox)

**Target Platform**: Linux com suporte a namespaces de kernel (user/mount/network) — já coberto pelo
Princípio I da constitution

**Project Type**: Aplicação desktop nativa (core) + plugins como processos separados — sem mudança de
tipo de projeto

**Performance Goals**: tempo de handshake/refresh por plugin na mesma ordem de grandeza observada sem
sandbox (SC-006) — sem meta numérica dura, verificação qualitativa via `harness.sh`

**Constraints**: nenhuma mudança de wire format do protocolo (sem bump de versão — `CapabilityManifest`
já suporta o valor `network` que `git-local`/`openfortivpn-vpn` passam a declarar, D7); `bwrap`
ausente MUST falhar fechado, nunca degradar para subprocess sem sandbox (FR-007)

**Scale/Scope**: 4 plugins de referência existentes migrados para rodar sob sandbox; nenhum plugin
novo adicionado nesta feature

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Princípio II (Plugins como Processos Isolados via JSON-RPC)**: reforçado, não violado — o
  isolamento por processo já existente ganha uma camada real de isolamento de kernel (namespaces)
  por baixo, sem mudar o modelo de comunicação (stdin/stdout, JSON-RPC inalterado).
- **Princípio IV (Permissões Explícitas por Manifesto)**: esta feature é a implementação direta
  deste princípio, hoje só parcialmente satisfeito (manifesto declarado, não aplicado). PASS.
- **Nenhum outro princípio (I, III, V, VI, VII) é tocado por esta feature** — sem violação a
  justificar em Complexity Tracking.

Constitution Check PASS — sem violações a registrar.

## Project Structure

### Documentation (this feature)

```text
specs/006-sandbox-permissoes-bubblewrap/
├── plan.md              # Este arquivo
├── research.md          # Fase 0 — decisões D1-D12, todas validadas empiricamente
├── data-model.md         # Fase 1 — SandboxProfile, BindMount, extensão de PluginSpawnConfig
├── quickstart.md         # Fase 1 — 6 cenários de validação manual
├── contracts/
│   └── bwrap-invocation-contract.md   # Contrato normativo de composição dos argumentos de bwrap
└── tasks.md              # Fase 2 — gerado por /speckit-tasks, não por este comando
```

### Source Code (repository root)

```text
crates/
├── farol-protocol/
│   └── src/messages.rs           # SEM mudança de forma — CapabilityManifest já suporta "network"
└── farol-core/
    └── src/
        ├── sandbox.rs             # NOVO — SandboxProfile, BindMount, build_bwrap_args()
        ├── plugin_worker.rs       # worker() passa a envolver Command::new(&config.command) com
        │                          # bwrap; PluginSpawnConfig ganha campo sandbox_profile;
        │                          # known_plugins() ganha o perfil de cada um dos 4 plugins
        ├── model.rs                # SEM mudança de forma (research.md D8 — reaproveita
        │                          # Unavailable{FailedToStart})
        └── (sandbox_integration_tests, dentro de plugin_worker.rs ou módulo próprio #[cfg(test)])

plugins/
├── git-local/main.py               # handshake ganha {"kind": "network"} nas capabilities (D7)
└── openfortivpn-vpn/main.py        # idem (D7)

tests/integration/
└── harness.sh                      # sem mudança de contrato — continua confirmando 7/7 condições,
                                     # agora exercitando o caminho sandboxed por baixo dos panos
```

**Structure Decision**: nenhuma pasta nova de projeto — a feature vive inteiramente dentro do crate
`farol-core` já existente (um módulo novo, `sandbox.rs`) mais dois ajustes de handshake nos plugins
Python já existentes (D7). Não há novo plugin, novo crate, nem novo diretório de nível superior.

## Complexity Tracking

*Sem violações de Constitution Check — seção não aplicável.*
