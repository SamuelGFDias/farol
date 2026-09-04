# Implementation Plan: Registry — Descoberta e Instalação de Plugins de Terceiros via GitHub

**Branch**: `007-registry-instalacao-plugins-github` | **Date**: 2026-09-04 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/007-registry-instalacao-plugins-github/spec.md`

## Summary

O core passa a descobrir plugins instalados dinamicamente (sem recompilar), lendo um manifesto novo
(`farol-plugin.toml`) de um diretório de dados do usuário (`$XDG_DATA_HOME/farol/plugins/<nome>/`,
distinto do diretório de configuração já existente), e ganha uma subcommand de CLI
(`farol install <owner>/<repo>`) que baixa a release/tag mais recente de um repositório GitHub
público via `curl`/`tar` (sem dependência Rust nova), valida o manifesto, e publica atomicamente no
diretório de dados. A fonte de verdade do `SandboxProfile` (feature 006, D1) generaliza: para um
plugin descoberto, vem do manifesto lido do disco, nunca do handshake em runtime — mesma disciplina
já aplicada aos 4 plugins de referência hardcoded. `sandbox::build_bwrap_args` generaliza o bind de
"código do plugin" de sempre-a-raiz-do-repo para um `code_root` por plugin (a raiz do repo para os
4 de referência, o próprio diretório de instalação para os descobertos). Design validado
empiricamente nesta sessão contra a API real do GitHub (`releases/latest`, download de tarball,
extração com `--strip-components=1`).

## Technical Context

**Language/Version**: Rust (workspace já em uso), TOML (novo formato de manifesto, mesma crate
`toml` já dependência de `farol-core`)

**Primary Dependencies**: `curl`/`tar` (binários externos do sistema, via `std::process::Command` —
sem crate Rust nova; nenhuma dependência nova em `Cargo.toml`)

**Storage**: novo diretório `$XDG_DATA_HOME/farol/plugins/<nome>/` (fallback `~/.local/share/farol/
plugins/`) — distinto de `$XDG_CONFIG_HOME/farol` já existente (`config_store`/`secrets_store`)

**Testing**: `cargo test --workspace` (unidade de `parse_manifest`/`discover_installed_plugins` +
integração do fluxo de instalação contra servidor HTTP local sintético, mesmo padrão de
`MetricsFixtureServer` da feature 002)

**Target Platform**: Linux (já coberto pelo Princípio I)

**Project Type**: Aplicação desktop nativa (core) + plugins como processos separados — sem mudança
de tipo de projeto; ganha uma subcommand de CLI no mesmo binário

**Performance Goals**: instalação de um plugin (download + extração + validação) completa em
segundos para um repositório de tamanho típico — sem meta numérica dura, a operação não é de alta
frequência (Assumptions de `spec.md`)

**Constraints**: rate limit não-autenticado da API do GitHub (60 req/hora por IP, D9); nenhuma
mudança de wire format do protocolo `farol-protocol` (esta feature não bump a versão — o manifesto
é um artefato local novo, não parte do JSON-RPC entre core e plugin)

**Scale/Scope**: os 4 plugins de referência existentes migram só o suficiente para ganhar
`code_root` (D4) — sem mudança de comportamento; nenhum plugin novo de referência é adicionado

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Princípio VII (Registry Federado sem Infra Própria)**: esta feature é a implementação direta
  deste princípio no que cabe ao core (descoberta local + instalação puxando release do
  repositório do plugin) — o repo-índice central em si fica fora de escopo (decisão de infra
  externa, `spec.md` Clarifications). PASS parcial, consciente: a feature entrega o que o core
  precisa, não o ecossistema de índice inteiro.
- **Princípio II (Plugins como Processos Isolados via JSON-RPC)**: reforçado — plugin descoberto
  passa pela mesma máquina de estados/handshake/sandbox de qualquer plugin já existente, nenhum
  caminho especial.
- **Princípio IV (Permissões Explícitas por Manifesto)**: reforçado — a fonte de verdade do
  sandbox (D1 de feature 006) generaliza corretamente para plugins de terceiro: o manifesto local,
  lido antes do spawn, nunca o autorrelato em runtime.
- **Nenhum outro princípio (I, III, V, VI) é tocado por esta feature** — sem violação a justificar
  em Complexity Tracking.

Constitution Check PASS — sem violações a registrar.

## Project Structure

### Documentation (this feature)

```text
specs/007-registry-instalacao-plugins-github/
├── plan.md              # Este arquivo
├── research.md          # Fase 0 — decisões D1-D9, D5/D9 validadas empiricamente
├── data-model.md         # Fase 1 — PluginManifest, ManifestError, extensão de PluginSpawnConfig
├── quickstart.md         # Fase 1 — 6 cenários de validação manual
├── contracts/
│   └── plugin-manifest-and-install-contract.md
└── tasks.md              # Fase 2 — gerado por /speckit-tasks
```

### Source Code (repository root)

```text
crates/farol-core/src/
├── plugin_manifest.rs     # NOVO — PluginManifest, ManifestError, parse_manifest()
├── install.rs              # NOVO — fluxo de instalação (curl/tar), InstallOutcome
├── plugin_worker.rs        # known_plugins() ganha code_root; discover_installed_plugins() novo
├── sandbox.rs               # build_bwrap_args generaliza repo_root → code_root (mesma assinatura,
│                            # só o nome conceitual do parâmetro; sem mudança de comportamento para
│                            # os 4 plugins de referência)
└── main.rs                  # parse de `install` subcommand antes do iced::application;
                             # Farol::default soma known_plugins() + discover_installed_plugins()

templates/
└── plugin-template/         # NOVO — farol-plugin.toml + main.py mínimo (US3)
```

**Structure Decision**: dois módulos novos dentro de `farol-core` (`plugin_manifest.rs`,
`install.rs`), mais um diretório de template fora de `crates/` (nível do repositório, ao lado de
`plugins/` — não é um plugin de referência ativo, só material de referência para quem for escrever
um plugin novo). Nenhum crate novo, nenhuma dependência Rust nova.

## Complexity Tracking

*Sem violações de Constitution Check — seção não aplicável.*
