---
description: "Task list for feature 007 - registry de instalação de plugins"
---

# Tasks: Registry — Descoberta e Instalação de Plugins de Terceiros via GitHub

**Input**: Design documents from `/specs/007-registry-instalacao-plugins-github/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md,
`contracts/plugin-manifest-and-install-contract.md`, quickstart.md

**Tests**: incluídas — mesma disciplina das features 001-006 deste projeto.

**Organization**: Foundational bloqueante (manifesto + generalização de `code_root`), depois as 3
User Stories, independentes entre si.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Criar `crates/farol-core/src/plugin_manifest.rs` (vazio) e `crates/farol-core/src/
      install.rs` (vazio), declarar `mod plugin_manifest;`/`mod install;` em
      `crates/farol-core/src/main.rs` (ordem alfabética, entre `plugin_worker` e `sandbox`/`secrets_store`)

## Phase 2: Foundational (bloqueante para as 3 User Stories)

**Purpose**: manifesto (`PluginManifest`/`parse_manifest`), diretório de dados, e a generalização de
`code_root` no sandbox — pré-requisito comum.

- [X] T002 [P] Implementar `PluginManifest`/`ManifestError`/`parse_manifest` (`data-model.md`,
      `contracts/plugin-manifest-and-install-contract.md`) em `crates/farol-core/src/
      plugin_manifest.rs` — desserialização TOML tolerante a campo desconhecido, validação exata da
      tabela de `ManifestError` do contrato
- [X] T003 [P] Testes de unidade de `parse_manifest` em `plugin_manifest.rs`: arquivo ausente, TOML
      inválido, `plugin_name` ausente/vazio, `command` ausente, `args` ausente (`args = []` válido),
      `capabilities` ausente (default `false`/`false`), campo desconhecido ignorado, caso de sucesso
      completo
- [X] T004 Implementar `farol_data_base_dir()`/`installed_plugin_dir(nome)` (`research.md` D2) em
      `plugin_manifest.rs` (ou módulo próprio `data_store.rs`, à sua escolha — documentar a decisão),
      mesma convenção de `config_store::farol_config_base_dir()` mas para `XDG_DATA_HOME`/
      `~/.local/share`
- [X] T005 Estender `PluginSpawnConfig` com o campo `code_root: PathBuf` em
      `crates/farol-core/src/plugin_worker.rs`; atualizar `known_plugins()` para preencher esse
      campo com a raiz do repositório Farol (mesmo cálculo que `worker()` já fazia via
      `env!("CARGO_MANIFEST_DIR")`, movido para o ponto de construção de `known_plugins()` ou
      calculado uma vez e reutilizado)
- [X] T006 Alterar `worker()` (`plugin_worker.rs`) para usar `config.code_root` diretamente em vez de
      calcular `repo_root` internamente — mesma chamada a `sandbox::build_bwrap_args`, só a origem
      do parâmetro muda; `sandbox.rs` não precisa mudar de assinatura (o parâmetro já se chamava
      `repo_root: &Path` — pode ser renomeado para `code_root: &Path` por clareza, mas isso é
      cosmético, não funcional)
- [X] T007 Testes de unidade/integração confirmando que a mudança de T005/T006 não altera o
      comportamento dos 4 plugins de referência (regressão) — reaproveitar/ajustar os testes já
      existentes de `sandbox_integration_tests` que usam os perfis reais de `known_plugins()`

**Checkpoint**: com T001-T007 completas, o manifesto pode ser lido e validado, e o sandbox já sabe
bindar um `code_root` por plugin — as 3 User Stories seguintes constroem sobre isso.

## Phase 3: User Story 1 - Farol carrega plugins instalados sem recompilar o core (P1)

**Goal**: descoberta dinâmica de plugins instalados, somada aos 4 de referência, com filtragem de
colisão de nome.

**Independent Test**: colocar um manifesto válido manualmente no diretório de dados e confirmar que
o Farol o descobre e spawna.

- [ ] T008 [US1] Implementar `discover_installed_plugins() -> Vec<PluginSpawnConfig>` em
      `plugin_worker.rs`, seguindo exatamente o contrato de
      `contracts/plugin-manifest-and-install-contract.md` § "Contrato de
      `discover_installed_plugins`" (diretório ausente → `vec![]`; manifesto inválido → aviso +
      pular; sucesso → `PluginSpawnConfig` com `sandbox_profile`/`code_root` corretos)
- [ ] T009 [US1] Implementar a filtragem de colisão de nome (`research.md` D6,
      `contracts/plugin-manifest-and-install-contract.md` § "Contrato de filtragem de colisão") no
      ponto de montagem `crates/farol-core/src/main.rs::Farol::default` — soma
      `known_plugins()` + `discover_installed_plugins()` filtrado
- [ ] T010 [US1] Testes de unidade de `discover_installed_plugins` (diretório de dados temporário via
      fixture, sem depender de `~/.local/share/farol` real): zero plugins instalados, um plugin
      válido, um manifesto malformado ao lado de um válido (o malformado não impede o válido), dois
      plugins com nomes colidindo entre si
- [ ] T011 [US1] Teste de integração real (`iced_test::Emulator`, mesmo padrão de `e2e_tests.rs`
      já existente) confirmando que um plugin descoberto (fixture apontando `XDG_DATA_HOME` para um
      diretório temporário hermético, mesmo padrão de `XDG_CONFIG_HOME` já usado por
      `HarnessFixture`) chega a `Ready` pela máquina de estados real
- [ ] T012 [US1] Rodar `cargo test --package farol-core e2e_tests` e `tests/integration/harness.sh`
      — confirmar que os 4 plugins de referência continuam chegando a `Ready` sem regressão
      (SC-002/FR-009)

**Checkpoint**: US1 entregue e testável de forma independente.

## Phase 4: User Story 2 - Instalar um plugin a partir do repositório GitHub dele (P2)

**Goal**: `farol install <owner>/<repo>` funcional, com download/validação/publicação atômica.

**Independent Test**: rodar o comando contra um servidor HTTP local de fixture e inspecionar o
diretório de dados resultante.

- [ ] T013 [US2] Implementar `InstallOutcome` e o fluxo de instalação (`research.md` D5,
      `contracts/plugin-manifest-and-install-contract.md` § "Contrato do fluxo de instalação") em
      `crates/farol-core/src/install.rs` — `curl`/`tar` via `std::process::Command`, respeitando
      `FAROL_GITHUB_API_BASE` (`research.md` D8) para permitir override em teste
- [ ] T014 [US2] Adicionar o parse da subcommand `install <owner>/<repo>` em `main()`
      (`crates/farol-core/src/main.rs`), antes de montar o `iced::application` — formato inválido de
      `owner/repo` falha sem tentar rede; chama `install::run`, traduz `InstallOutcome` para
      código de saída (`0`/`1`) e mensagem em stdout/stderr
- [ ] T015 [US2] Servidor HTTP local de fixture para teste (`research.md` D8, mesmo padrão de
      `MetricsFixtureServer` da feature 002) em `install.rs` (módulo `#[cfg(test)]`) — serve
      `/repos/<owner>/<repo>/releases/latest` sintético e um tarball construído em runtime (via
      `tar`/`gzip` reais, não mockado)
- [ ] T016 [US2] Testes de integração do fluxo de instalação contra o servidor de fixture: sucesso
      (manifesto válido, plugin publicado no diretório de dados temporário), 404 sem release,
      download falhando, manifesto ausente/inválido no tarball, `plugin_name` colidindo com um dos
      4 de referência, reinstalação limpa por cima de uma instalação anterior (FR-008)
- [ ] T017 [US2] Rodar `cargo test --package farol-core` completo e confirmar 0 falhas

**Checkpoint**: US2 entregue e testável de forma independente.

## Phase 5: User Story 3 - Template de referência para escrever um plugin novo (P3)

**Goal**: `templates/plugin-template/` funcional, mínimo, documentado.

**Independent Test**: copiar o template, registrar manualmente como plugin instalado, confirmar
handshake válido.

- [ ] T018 [US3] Criar `templates/plugin-template/farol-plugin.toml` (exemplo mínimo, comentado) e
      `templates/plugin-template/main.py` (handshake mínimo respondendo `handshake/hello` com
      `capabilities`/`required_config`/`widgets`/`actions` vazios, mesmo nível de simplicidade do
      exemplo do Cenário 1 de `quickstart.md`) + `templates/plugin-template/README.md` explicando
      como usar (copiar, ajustar nome, colocar em `installed_plugin_dir`)
- [ ] T019 [US3] Teste de integração real (`iced_test::Emulator`) confirmando que o template, sem
      nenhuma edição de lógica (só o `plugin_name` ajustado para um nome de teste), completa
      handshake e chega a `Ready`

**Checkpoint**: US3 entregue. Todas as 3 User Stories completas.

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T020 [P] `cargo clippy --workspace --all-targets` limpo, sem warning novo
- [ ] T021 [P] `ruff check` limpo em `templates/plugin-template/` (se tiver `pyproject.toml` próprio
      — mesma disciplina dos plugins de referência)
- [ ] T022 Rodar `cargo test --workspace -- --test-threads=1` completo — confirmar 0 falhas
- [ ] T023 Rodar `tests/integration/harness.sh` completo — confirmar `SUCESSO — 7/7 condições` sem
      regressão (os 4 plugins de referência continuam sendo os únicos ativos nesse ambiente, sem
      diretório de dados de terceiros presente)
- [ ] T024 Criar issue(s) GitHub para os itens de débito técnico identificados no planejamento desta
      feature (instalação in-app na UI gráfica em vez de CLI; plugin exigindo build/asset binário
      próprio; capability de filesystem genérica para plugin de terceiro; repo-índice central + CI
      de validação como trabalho futuro de infraestrutura) — sem pedir permissão manual, conforme
      instrução vigente da sessão
- [ ] T025 Atualizar `AGENTS.md` com a entrada da feature 007, mesmo padrão das entradas 004/005/006
- [ ] T026 Atualizar `README.md` — roadmap item 4 ("Registry") passa a descrever o que foi entregue
- [ ] T027 Preencher a seção "Automação equivalente" de `quickstart.md` com os nomes reais dos
      testes escritos

## Dependencies & Execution Order

- **Setup (T001)** → bloqueia tudo.
- **Foundational (T002-T007)** → bloqueia as 3 User Stories.
- **US1 (T008-T012)**, **US2 (T013-T017)** e **US3 (T018-T019)** são independentes entre si depois
  do Foundational — US1 toca `plugin_worker.rs`/`main.rs`; US2 toca `install.rs`/`main.rs` (ponto
  de entrada de CLI, mesma área de `main.rs` que US1 — coordenar para não editar `main.rs`
  simultaneamente, mesma disciplina de "um agente por área de arquivo" já usada na feature 006);
  US3 só cria arquivos novos em `templates/`, sem tocar em nenhum arquivo de `crates/`.
- **Polish (T020-T027)** → depois de todas as User Stories completas.

## Implementation Strategy

**MVP = US1 sozinha** (T001-T012): já resolve a lacuna arquitetural central (plugin não precisa
mais recompilar o core) e é a User Story de maior prioridade (P1). US2 (instalação automatizada) e
US3 (template) são incrementos que tornam a US1 utilizável por alguém de fora sem editar arquivos à
mão.
