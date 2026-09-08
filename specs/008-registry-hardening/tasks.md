---
description: "Task list for feature 008 - registry hardening (índice, build, filesystem, UI in-app)"
---

# Tasks: Registry — Índice Central, Instalação com Build e Capability de Filesystem Genérica

**Input**: Design documents from `/specs/008-registry-hardening/`

**Prerequisites**: plan.md, spec.md

**Tests**: incluídas — mesma disciplina das features 001-007 deste projeto.

**Organization**: Foundational bloqueante (extensão do manifesto + resolução de índice), depois US2
(build) e US3 (filesystem) em sequência (ambas tocam `install.rs`), depois US4 (UI in-app) por
último. Ver `plan.md` § Complexity Tracking para a justificativa da ordem.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Criar `crates/farol-core/src/registry_index.rs` (vazio) e declarar `mod registry_index;`
      em `crates/farol-core/src/main.rs` (ordem alfabética)

## Phase 2: Foundational (bloqueante para US2, US3 e US4)

**Purpose**: extensão do manifesto (`build`, `filesystem_read`, `filesystem_read_write`,
`validate_filesystem_paths`) e resolução de índice central (US1) — pré-requisito comum às demais
User Stories.

- [X] T002 [P] Estender `PluginManifest`/`RawManifest` em `crates/farol-core/src/
      plugin_manifest.rs:36-44,63-79` com `build: Option<String>`, `filesystem_read: Vec<String>`
      (default vazio) e `filesystem_read_write: Vec<String>` (default vazio)
- [X] T003 [P] Implementar `validate_filesystem_paths(&[String]) -> Result<(), ManifestError>` em
      `plugin_manifest.rs`: rejeita caminho relativo, rejeita caminho contido na denylist estática
      (constante `FILESYSTEM_CAPABILITY_DENYLIST`: `~/.ssh`, `/etc`, o diretório de
      `secrets_store`/`config_store` do próprio Farol), aceita caminho absoluto fora da denylist
- [X] T004 [P] Testes de unidade de T002/T003 em `plugin_manifest.rs`: manifesto sem `build`/
      filesystem (retrocompatível com os manifestos da feature 007), `build` presente, caminho de
      filesystem relativo rejeitado, caminho na denylist rejeitado, caminho absoluto válido aceito,
      múltiplos caminhos válidos
- [X] T005 [US1] Implementar `registry_index.rs`: struct `RegistryIndexEntry{name, owner, repo}`,
      fn `resolve_name(name: &str, index_url_base: &str) -> Result<RegistryIndexEntry,
      RegistryIndexError>` — busca `index.toml` via `curl` (mesmo padrão de `install.rs`), parseia,
      procura `name` (não encontrado → `RegistryIndexError::NotFound`); suportar override de URL
      base via env var (mesmo padrão de `FAROL_GITHUB_API_BASE`, ex.
      `FAROL_REGISTRY_INDEX_URL`) para permitir teste com servidor HTTP local
- [X] T006 [US1] Implementar `install::run_by_name(name: &str) -> InstallOutcome` em
      `crates/farol-core/src/install.rs`: chama `registry_index::resolve_name`, repassa
      `owner`/`repo` para o `install::run` já existente (feature 007); erro de resolução de nome
      vira uma variante nova de `InstallOutcome`/erro de instalação, consistente com as já existentes
      — **Nota de implementação**: entregue como `install::run_by_name(name: &str) ->
      InstallByNameOutcome` (tipo novo, não uma variante adicionada a `InstallOutcome`). Motivo:
      `crates/farol-core/src/main.rs::handle_install_subcommand` faz hoje um `match` exaustivo
      sobre `InstallOutcome` (T014 da feature 007); acrescentar variante àquele enum exigiria um
      braço novo nesse `match`, e esta subtarefa (Foundational, T001-T007) foi delimitada para não
      tocar `main.rs` além do `mod registry_index;` de T001 — a integração de `farol install
      <nome>` na CLI/UI é US4 (T013-T016), fora deste escopo. `InstallByNameOutcome::Resolved`
      carrega o `InstallOutcome` de sempre para o caminho feliz, preservando FR-003 (nenhuma lógica
      de instalação duplicada). Rever esta decisão ao implementar US4/wiring de CLI: nesse ponto
      `main.rs` já vai precisar de mudança de qualquer forma para expor `farol install <nome>`, e é
      o momento natural para decidir se `InstallOutcome` ganha as variantes novas com o `match`
      ajustado, ou se `InstallByNameOutcome` permanece como tipo próprio.
- [X] T007 [US1] Testes de unidade/integração de `registry_index.rs` e `install::run_by_name` contra
      servidor HTTP local de fixture (mesmo padrão de `MetricsFixtureServer`/D8 da feature 007, nunca
      contra o GitHub real): nome resolvido com sucesso, nome não encontrado, índice malformado

**Checkpoint**: com T001-T007 completas, o manifesto aceita os novos campos e `farol install <nome>`
(por nome, via índice) funciona de ponta a ponta — as demais User Stories constroem sobre isso.

## Phase 3: User Story 2 - Instalar plugin com passo de build (P2)

**Goal**: manifesto com `build` executa o comando no `staging_dir` antes do rename atômico; falha de
build aborta a instalação sem publicar nada.

**Independent Test**: instalar um plugin de fixture cujo manifesto declara `build = "true"` (sucesso)
e outro com `build = "false"` (falha esperada, sem publicação).

- [X] T008 [US2] Em `crates/farol-core/src/install.rs:82-100`, após `parse_manifest` e antes do
      `rename` atômico: se `manifest.build.is_some()`, executar via
      `std::process::Command::new("sh").arg("-c").arg(build).current_dir(staging_dir)`; `exit code
      != 0` aborta a instalação (reaproveitar o mecanismo de limpeza já existente para
      `ManifestInvalid`), sem mover nada para o diretório final — **Nota de implementação**: o
      erro de build vira `InstallOutcome::DownloadFailed(mensagem)` (mensagem prefixada "comando de
      build falhou: ..."), não uma variante nova. Motivo: `main.rs::handle_install_subcommand`
      (T014 da feature 007) faz `match` exaustivo sobre `InstallOutcome` sem braço `_`, e esta
      subtarefa foi delimitada para tocar só `install.rs`/`tasks.md` — adicionar variante exigiria
      editar `main.rs`, fora do escopo autorizado. `DownloadFailed` foi escolhida por já ser a
      variante genérica de falha de execução externa (`curl`/`tar`) mais próxima semanticamente;
      revisar ao integrar US4 (UI in-app), quando `main.rs` já vai precisar de mudança de qualquer
      forma.
- [X] T009 [US2] Testes de unidade/integração de T008 usando comandos de fixture (`true`, `false`,
      `exit 1`) — nunca toolchains reais: build bem-sucedido publica o plugin, build falho não
      publica nada e o `staging_dir` é limpo, plugin sem `build` continua funcionando como antes
      (regressão da feature 007) — `install_with_successful_build_publishes_the_plugin` (`build =
      "true"`), `install_with_failing_build_does_not_publish_and_cleans_up` (`build = "exit 1"`,
      confirma `DownloadFailed` com mensagem "comando de build falhou" e que
      `installed_plugin_dir` não existe), regressão coberta pelos testes já existentes de
      `install_succeeds_and_publishes_the_plugin` (manifesto sem `build`)

**Checkpoint**: US2 entregue e testável de forma independente, sem afetar US1.

## Phase 4: User Story 3 - Capability de filesystem genérica (P3)

**Goal**: `filesystem_read`/`filesystem_read_write` do manifesto viram `BindMount` reais no
`SandboxProfile` do plugin instalado.

**Independent Test**: instalar um plugin de fixture com `filesystem_read_write = ["/caminho/
absoluto/valido"]` e confirmar (teste de integração real com `bwrap`) que o caminho fica acessível
dentro do sandbox do plugin.

- [X] T010 [US3] Em `crates/farol-core/src/plugin_worker.rs:246-281`
      (`discover_installed_plugins`), popular `SandboxProfile.extra_binds` a partir de
      `manifest.filesystem_read`/`filesystem_read_write` (hoje sempre `vec![]`), convertendo cada
      caminho validado (T003) em um `BindMount` (somente-leitura ou leitura-escrita conforme o
      campo de origem)
- [X] T011 [US3] Testes de unidade de T010 com diretório de dados temporário (fixture, sem depender
      de `~/.local/share/farol` real): manifesto sem capability de filesystem (regressão — binds
      vazios como hoje), manifesto com `filesystem_read`, manifesto com `filesystem_read_write`,
      manifesto com caminho inválido (não deveria nem passar de T003/`parse_manifest`)
      — **Nota de implementação**: caminho inválido não tem teste dedicado aqui porque não é
      alcançável neste ponto — `discover_installed_plugins` só recebe um `PluginManifest` já
      validado por `parse_manifest`/`validate_filesystem_paths` (T003/T004 cobrem a rejeição na
      camada de parse, antes de qualquer `PluginSpawnConfig` existir).
- [X] T012 [US3] Teste de integração real (mesmo padrão de `sandbox::sandbox_integration_tests`,
      `sandbox.rs:563+`, exige `bwrap` instalado): plugin de fixture com `filesystem_read_write`
      escreve em um arquivo dentro do caminho liberado e o processo pai confirma a escrita fora do
      sandbox; tentativa de escrita fora do(s) caminho(s) liberado(s) falha — **Nota de
      implementação**: o teste vive em `plugin_worker.rs`
      (`filesystem_capability_integration_tests::
      discovered_plugin_with_filesystem_read_write_can_write_inside_and_not_outside_the_allowed_path`),
      não em `sandbox.rs` — esta subtarefa (T010-T012) foi delimitada para tocar só
      `plugin_worker.rs`/`tasks.md`; o teste importa `crate::sandbox::build_bwrap_args`/
      `BindMount` (já públicos) em vez de acrescentar uma função nova em `sandbox.rs`, e monta o
      `PluginSpawnConfig` via `discover_installed_plugins` (T010, o código sob teste) a partir de
      um manifesto real em disco, preservando o mesmo caminho de produção ponta a ponta.

**Checkpoint**: US3 entregue e testável de forma independente, sem afetar US1/US2.

## Phase 5: User Story 4 - Instalação in-app via UI do iced (P4)

**Goal**: fluxo de instalação (por nome, via índice) disponível dentro da própria UI do Farol, sem
travar a interface durante download/build.

**Independent Test**: abrir o Farol, digitar um nome de plugin de fixture no formulário de
instalação, confirmar, e ver o plugin passar a aparecer na lista sem reiniciar a aplicação.

- [X] T013 [US4] Estender `crates/farol-core/src/model.rs`: novo estado de formulário de instalação
      (nome digitado, status idle/em-progresso/sucesso/erro), seguindo o padrão de `SetupForm`
      (`model.rs:396-403`)
- [X] T014 [US4] Estender `crates/farol-core/src/main.rs`: novas variantes de `Message` (ex.
      `InstallByNameSubmitted`, `InstallOutcomeReceived`), e em `update.rs` disparar
      `install::run_by_name` via `tokio::task::spawn_blocking` dentro de `Command::perform`/`Task`
      do `iced` (API exata a confirmar na versão 0.14 em uso, ver `plan.md` § Complexity Tracking),
      mantendo a UI responsiva
- [X] T015 [US4] Implementar `view_install_form` em `crates/farol-core/src/view.rs`, reaproveitando o
      padrão visual de `view_setup_form` (`view.rs:562-582`); adicionar o ponto de entrada desse
      formulário na view principal
- [X] T016 [US4] Teste de integração real (`iced_test::Emulator`, mesmo padrão de `e2e_tests.rs`)
      confirmando que submeter o formulário de instalação com um plugin de fixture resulta no plugin
      aparecendo na lista sem reiniciar a aplicação

**Checkpoint**: US4 entregue. Com T001-T016 completas, as 4 issues (#15, #16, #17, #18 na parte de
código) estão fechadas — falta só a publicação do repositório-índice real (T017).

## Phase 6: Infraestrutura externa (issue #18 — checkpoint sensível)

**ATENÇÃO**: T017 cria e publica um repositório GitHub público. Esta é uma ação visível
externamente — o arquiteto MUST confirmar com o usuário o nome/owner exatos antes de disparar esta
task, mesmo que T001-T016 já estejam completas e verificadas. Não presumir autorização implícita.

- [ ] T017 Criar e publicar o repositório-índice `index.toml` + `.github/workflows/validate.yml`
      (CI valida: nome único, `owner/repo` bem-formado, manifesto do repo referenciado é alcançável
      e parseável) + `README.md` com instruções de submissão — só após confirmação explícita do
      usuário sobre nome/owner do repositório
