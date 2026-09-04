---
description: "Task list for feature 006 - sandbox de plugins via bubblewrap"
---

# Tasks: Sandbox de Plugins via Bubblewrap e Aplicação Real do Manifesto de Capacidades

**Input**: Design documents from `/specs/006-sandbox-permissoes-bubblewrap/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/bwrap-invocation-contract.md, quickstart.md

**Tests**: incluídas — mesma disciplina das features 001-005 deste projeto (testes de unidade,
integração real e regressão fazem parte do critério de pronto de cada task de código).

**Organization**: agrupadas por User Story (spec.md), com uma fase Foundational bloqueante antes
delas (o módulo `sandbox.rs` é pré-requisito comum às três).

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Criar módulo `crates/farol-core/src/sandbox.rs` (arquivo vazio + `mod sandbox;` em
      `crates/farol-core/src/main.rs`), sem lógica ainda — só o esqueleto para as tasks seguintes.

## Phase 2: Foundational (bloqueante para as 3 User Stories)

**Purpose**: implementar a composição dos argumentos de `bwrap` (`contracts/bwrap-invocation-contract.md`)
e envolver o spawn já existente em `plugin_worker.rs` — nenhuma User Story é testável sem isso.

- [X] T002 [P] Implementar `SandboxProfile`/`BindMount` (`data-model.md`) em `crates/farol-core/src/sandbox.rs`
- [X] T003 Implementar resolução do caminho absoluto do interpretador (`research.md` D10,
      `resolve_interpreter_path`) em `crates/farol-core/src/sandbox.rs` — busca em `$PATH` do
      processo do Farol, sem crate nova
- [X] T004 Implementar `build_bwrap_args(repo_root, interpreter_path, profile, command, args) ->
      Vec<String>` em `crates/farol-core/src/sandbox.rs`, seguindo exatamente a ordem normativa de
      `contracts/bwrap-invocation-contract.md` (base → condicional rede → condicional exec → código
      do repo → mounts genéricos → `extra_binds` por último → `--` + comando)
- [X] T005 [P] Testes de unidade de `build_bwrap_args` em `crates/farol-core/src/sandbox.rs`: perfil
      sem rede/sem exec, com rede, com exec, com `extra_binds`, e um teste que trava a ordem relativa
      dos blocos (regressão do bug real de ordem encontrado em `research.md` D5)
- [X] T006 Estender `PluginSpawnConfig` com o campo `sandbox_profile: SandboxProfile` em
      `crates/farol-core/src/plugin_worker.rs`
- [X] T007 Atualizar `known_plugins()` em `crates/farol-core/src/plugin_worker.rs` com o perfil de
      cada um dos 4 plugins, exatamente conforme a tabela "Perfis resolvidos por plugin" de
      `contracts/bwrap-invocation-contract.md` (`git-local`: rede+exec+`scan_root`; `uptime-kuma`:
      rede, sem exec; `openfortivpn-vpn`: rede+exec; `docker-containers`: exec+socket Docker, sem rede)
- [X] T008 Alterar `worker()` em `crates/farol-core/src/plugin_worker.rs` para spawnar via
      `Command::new("bwrap")` com os argumentos de `sandbox::build_bwrap_args(...)` seguidos de `--`
      + `config.command` + `config.args`, preservando `.env(...)` (T018 original, D8/D9 —
      **nenhuma mudança** no mecanismo de env var), `.stdin/.stdout/.stderr/.kill_on_drop` já
      existentes
- [X] T009 Diferenciar, em `worker()`, a mensagem de erro quando o processo `bwrap` em si não é
      encontrado (`std::io::ErrorKind::NotFound` do comando `"bwrap"`) de uma falha do plugin real —
      ambas continuam mapeando para `Unavailable{FailedToStart}` (`research.md` D8), só o texto muda
- [X] T010 Testes de integração real com `bwrap` instalado (novo módulo/seção `#[cfg(test)]` em
      `crates/farol-core/src/sandbox.rs` ou arquivo próprio, `#[ignore]`able se `bwrap` ausente do
      `PATH` do ambiente, `research.md` D11): reproduzir os dois experimentos negativos já validados
      manualmente nesta sessão (rede negada → `ConnectionRefused`/`Network is unreachable`; exec
      negado → `NotFound`) com um `python3 -c "..."` real sob o sandbox construído

**Checkpoint**: com T001-T010 completas, `sandbox.rs` está pronto e `worker()` já spawna todo plugin
sob `bwrap` — as 3 User Stories seguintes são, cada uma, sobre **provar** aspectos específicos desse
mesmo mecanismo já implementado.

## Phase 3: User Story 1 - Um plugin sem a capacidade de rede não alcança rede nenhuma (P1)

**Goal**: rede negada por padrão de fato aplicada; `git-local`/`openfortivpn-vpn` corrigem o
manifesto que sempre precisou de rede sem nunca ter declarado (`research.md` D7).

**Independent Test**: `docker-containers` (sem `network`) não alcança rede alguma; `uptime-kuma`
(com `network`) continua alcançando `/metrics` normalmente.

- [X] T011 [P] [US1] Corrigir `handshake_hello` em `plugins/git-local/main.py` para declarar
      `{"kind": "network"}` além de `{"kind": "exec"}` em `capabilities.capabilities`
      (`research.md` D7)
- [X] T012 [P] [US1] Corrigir `handshake_hello` em `plugins/openfortivpn-vpn/main.py` para declarar
      `{"kind": "network"}` além de `{"kind": "exec"}` em `capabilities.capabilities`
      (`research.md` D7)
- [X] T013 [US1] Teste de integração real (junto de T010) provando que `docker-containers`
      (`allow_network: false`) não alcança rede alguma dentro do sandbox
- [X] T014 [US1] Teste de integração real provando que `uptime-kuma` (`allow_network: true`)
      continua alcançando um endpoint HTTP local de teste (mesmo padrão de fixture de
      `MetricsFixtureServer` já usado na feature 002) normalmente dentro do sandbox
- [X] T015 [US1] Rodar `cargo test --package farol-core e2e_tests` — confirmar que os 4 plugins
      continuam chegando a `Ready` normalmente agora rodando sob sandbox com rede aplicada
      (regressão zero, SC-002/FR-011)

**Checkpoint**: US1 entregue e testável de forma independente.

## Phase 4: User Story 2 - Um plugin roda isolado do restante do filesystem do usuário (P2)

**Goal**: `exec` mediado por visibilidade seletiva de filesystem; os dois casos especiais nomeados
(`git-local`/`scan_root`, `docker-containers`/socket do Docker) continuam funcionando.

**Independent Test**: inspecionar, de dentro do processo sandboxed, quais caminhos estão visíveis;
`git.fetch` e o widget de containers continuam funcionando.

- [X] T016 [US2] Implementar em `crates/farol-core/src/sandbox.rs` a resolução do `scan_root` de
      `git-local` do lado do core (replicar a leitura de `plugins/git-local/config.py::
      load_scan_root` — mesmo caminho de config, mesmo default `~/dev`), usada para popular o
      `extra_binds` desse plugin em `known_plugins()`
- [X] T017 [US2] Implementar em `crates/farol-core/src/sandbox.rs`/`known_plugins()` o `extra_binds`
      de `docker-containers` apontando para `/var/run/docker.sock` (bind read-write, tolerante à
      ausência — `--bind-try`)
- [X] T018 [US2] Teste de integração real provando que um plugin sem `exec` concedida não consegue
      iniciar processo filho nenhum (`FileNotFoundError`/equivalente, SC-003) — junto de T010
- [X] T019 [US2] Teste de integração real provando que `git-local` continua lendo/escrevendo
      (`git fetch` contra um remote bare local de teste) no `scan_root` bindado, sob sandbox
- [X] T020 [US2] Teste de integração real provando que `docker-containers` continua falando com o
      daemon Docker pelo socket, sob sandbox, sem `network` concedida — usar Docker real se
      disponível no ambiente de teste, ou pular graciosamente (`#[ignore]`) se ausente, mesma
      disciplina de `docker_cli.py`/fixture da feature 005
- [X] T021 [US2] Rodar `cargo test --package farol-core e2e_tests` e `tests/integration/harness.sh`
      novamente — confirmar `git-local`/`docker-containers` sem regressão sob os dois `extra_binds`
      novos (SC-002/FR-011)

**Checkpoint**: US2 entregue e testável de forma independente.

## Phase 5: User Story 3 - Segredos configurados nunca ficam expostos além do que o plugin já recebia (P3)

**Goal**: confirmar (não construir — já garantido por omissão, `research.md` D9) que nenhum bind do
sandbox inclui `~/.config/farol`.

**Independent Test**: `uptime-kuma` com segredo configurado continua recebendo o valor por env var;
o arquivo `secrets.toml` não é localizável de dentro do sandbox por nenhum caminho.

- [X] T022 [US3] Teste de unidade em `crates/farol-core/src/sandbox.rs` auditando os argumentos
      produzidos por `build_bwrap_args` para todos os 4 perfis de `known_plugins()`, confirmando que
      nenhum bind (`--ro-bind`/`--bind`/`--ro-bind-try`/`--bind-try`) tem como `SRC`/`DEST` um
      caminho dentro de `~/.config/farol` (nem `secrets.toml`, nem o `config.toml` de qualquer
      plugin, inclusive o dele mesmo)
- [X] T023 [US3] Teste de integração real confirmando que `uptime-kuma` continua recebendo
      `FAROL_PLUGIN_UPTIME_KUMA_API_KEY` via variável de ambiente normalmente sob sandbox (sem
      regressão do mecanismo de `secrets_store.rs` já existente) e que uma tentativa de abrir
      `secrets.toml` por caminho absoluto de dentro do processo sandboxed falha

**Checkpoint**: US3 entregue e testável de forma independente. Todas as 3 User Stories completas.

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T024 [P] `cargo clippy --workspace --all-targets` limpo, sem warning novo introduzido por
      `sandbox.rs`/`plugin_worker.rs`
- [ ] T025 [P] `ruff check` limpo em `plugins/git-local/` e `plugins/openfortivpn-vpn/` (arquivos
      tocados em T011/T012)
- [ ] T026 Rodar `cargo test --workspace -- --test-threads=1` completo — confirmar 0 falhas
      (mitiga a flakiness de SIGSEGV já documentada em `AGENTS.md` para execução paralela)
- [ ] T027 Rodar `tests/integration/harness.sh` completo — confirmar `SUCESSO — 7/7 condições` e
      registrar o tempo observado, comparando com o baseline de ~9s da feature 005 (SC-006)
- [ ] T028 Criar issue GitHub rastreando o débito técnico de `research.md` D3 (mediação de `exec`
      só por visibilidade de filesystem, não por `seccomp` real — caminho residual de um plugin
      hostil autoproduzindo um executável em `tmpfs` gravável) — sem pedir permissão manual,
      conforme instrução vigente da sessão
- [ ] T029 Criar issue GitHub rastreando o débito técnico de `spec.md` FR-010 (capability `network`
      tratada como liga/desliga nesta fase, não allowlist real por `allowed_hosts`) — sem pedir
      permissão manual, conforme instrução vigente da sessão
- [ ] T030 Atualizar `AGENTS.md` com a entrada da feature 006, mesmo padrão das entradas 004/005 —
      cobrir: módulo `sandbox.rs`, decisão D1 (fonte de verdade estática, não handshake), os dois
      débitos técnicos registrados como issues (T028/T029), e a correção de manifesto de
      `git-local`/`openfortivpn-vpn` (D7)
- [ ] T031 Atualizar `README.md` — roadmap item 3 ("Sandbox e permissões") passa de "não iniciado"
      para descrever o que foi entregue nesta feature, mesmo padrão do item 2 (Docker) já atualizado
- [ ] T032 Preencher a seção "Automação equivalente" de `quickstart.md` com os nomes reais dos
      testes escritos nas tasks anteriores, mesmo padrão das features 002-005

## Dependencies & Execution Order

- **Setup (T001)** → bloqueia tudo.
- **Foundational (T002-T010)** → bloqueia as 3 User Stories; `sandbox.rs` e o novo caminho de
  spawn em `worker()` são pré-requisito comum.
- **US1 (T011-T015)**, **US2 (T016-T021)** e **US3 (T022-T023)** são independentes entre si depois
  do Foundational — podem ser implementadas em qualquer ordem ou em paralelo (arquivos distintos:
  US1 toca `plugins/git-local/main.py`/`plugins/openfortivpn-vpn/main.py`; US2 toca
  `sandbox.rs`/`known_plugins()` nos pontos de `extra_binds`; US3 só adiciona teste, não toca
  produção).
- **Polish (T024-T032)** → depois de todas as User Stories completas.

## Implementation Strategy

**MVP = US1 sozinha** (T001-T015): já entrega o núcleo do Princípio IV (rede negada por padrão,
aplicada de verdade) e é a User Story de maior prioridade (P1). US2/US3 são incrementos sobre a
mesma infraestrutura do Foundational, não dependências entre si.
