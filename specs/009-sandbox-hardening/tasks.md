---
description: "Task list for feature 009 - sandbox hardening (seccomp exec + nftables network allowlist)"
---

# Tasks: Sandbox — Mediação Real de Exec via Seccomp e Allowlist de Rede por Host

**Input**: Design documents from `/specs/009-sandbox-hardening/`

**Prerequisites**: plan.md, spec.md

**Tests**: incluídas — mesma disciplina das features 001-008 deste projeto.

**Organization**: Setup (dependência + módulos), depois US1 (exec/seccomp) e US2 (network/nftables)
em sequência (ambas tocam `sandbox.rs`). Ver `plan.md` § Complexity Tracking.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Adicionar dependência `seccompiler` em `crates/farol-core/Cargo.toml`
- [X] T002 [P] Criar `crates/farol-core/src/sandbox_seccomp.rs` (vazio) e `crates/farol-core/src/
      sandbox_network.rs` (vazio); declarar `mod sandbox_seccomp;`/`mod sandbox_network;` em
      `crates/farol-core/src/main.rs` (ordem alfabética, próximo a `sandbox`)

## Phase 2: Foundational (bloqueante para US1 e US2)

**Purpose**: mecanismo comum de falha explícita (fail-closed) quando o mecanismo de enforcement não
puder ser aplicado no sistema atual (FR-004).

- [X] T003 Estender o tipo de erro de montagem do sandbox (`sandbox.rs`, o mesmo usado hoje para
      erros de `bwrap`) com variantes `SeccompUnavailable`/`NetworkFirewallUnavailable`, cada uma
      com mensagem clara indicando o que falta no sistema (kernel sem suporte a seccomp-bpf, ou
      `nft`/`iptables` ausentes) — nenhum caminho de fallback silencioso para o comportamento antigo

**Checkpoint**: com T001-T003 completas, US1 e US2 têm onde reportar falha de forma consistente.

## Phase 3: User Story 1 - Exec bloqueado de verdade via seccomp (P1, issue #12)

**Goal**: `allow_exec=false` bloqueia `execve`/`execveat`/`fexecve` no kernel, fechando o vetor de
escrever-e-executar um binário em `tmpfs` gravável.

**Independent Test**: plugin com `exec=false` escreve um binário em `/tmp` (dentro do próprio
sandbox) e tenta executá-lo — deve falhar.

- [X] T004 [US1] **(Revisado — D1, achado empírico: `bwrap --seccomp FD` instala o filtro no
      PRÓPRIO processo `bwrap`, antes do seu `execvp()` final, bloqueando também o exec interno que
      lança o plugin — inviável, ver `plan.md` § D1 e § Complexity Tracking.)** Criar o crate
      `cdylib` `crates/farol-seccomp-preload/` (membro novo do workspace): gera o filtro BPF via
      `seccompiler` bloqueando `execve`/`execveat` (cobre `fexecve` por consequência, ver doc de
      `farol_seccomp_preload::build_exec_deny_filter`) e o aplica ao PRÓPRIO processo
      (`seccompiler::apply_filter`) a partir de um construtor ELF (`#[ctor::ctor]`) que roda quando o
      `.so` é carregado via `LD_PRELOAD` — depois que o processo alvo (o plugin) já foi `exec`ado com
      sucesso pelo `bwrap`, mas antes do seu `main()`. `sandbox_seccomp.rs` (`farol-core`) passa a só
      localizar esse `.so` já compilado em `target/{debug,release}/libfarol_seccomp_preload.so`
      (`sandbox_seccomp::locate_seccomp_preload_library`), sem gerar filtro no processo do Farol.
- [X] T005 [US1] Em `sandbox.rs` (`build_bwrap_args`), quando `allow_exec=false`: localizar o `.so`
      de T004 via `sandbox_seccomp::locate_seccomp_preload_library`, bind-montá-lo read-only dentro
      do sandbox e anexar `--setenv LD_PRELOAD <caminho>` aos argumentos do `bwrap` — posicionado
      DEPOIS do bind da raiz do repo/`code_root` (D5, regra de shadowing por bind mais amplo
      posterior, mesma classe de bug já corrigida para `--tmpfs /tmp` na feature 006); quando
      `allow_exec=true`, NÃO aplicar o filtro (mantém o comportamento atual de bind read-only de
      `/usr/bin`/`/bin`/`/usr/local/bin`, sem mudança)
- [X] T006 [US1] Estender `sandbox_unit_tests`: quando `allow_exec=false`, o `Vec<String>` retornado
      por `build_bwrap_args` contém o bind read-only do `.so` de `farol-seccomp-preload` e o
      `--setenv LD_PRELOAD <caminho>` correspondente; quando `allow_exec=true`, não contém nenhum dos
      dois
- [X] T007 [US1] Estender `sandbox_integration_tests` (exige `bwrap` instalado, e o `.so` de T004 já
      compilado — via helper `ensure_seccomp_preload_library_is_built()`) com um cenário real: plugin
      com `exec=false` escreve um binário executável em `/tmp` dentro do sandbox e tenta executá-lo —
      o processo pai confirma que a execução falhou (mesmo padrão de asserção via stdout
      "BLOCKED"/"LEAKED" já usado pelos testes existentes); mais um teste confirmando que a operação
      normal do plugin (handshake/widget/get, sem tentar exec) continua funcionando sem nenhum
      `execve`/`execveat`
- [X] T008 [US1] Testar explicitamente o caso de falha do mecanismo (FR-004): via escape-hatch de
      teste `FAROL_SANDBOX_TEST_FORCE_SECCOMP_PRELOAD_MISSING` (simula o `.so` de `farol-seccomp-
      preload` ausente/não compilado) confirmar que `sandbox_seccomp::locate_seccomp_preload_library`
      devolve `Err(SandboxMountError::SeccompUnavailable)`, e que `build_bwrap_args` propaga essa
      falha via `panic!` (nunca monta o sandbox sem a proteção real de exec) — mensagem clara, nunca
      degradando silenciosamente

**Checkpoint**: US1 entregue e testável de forma independente (issue #12 fechada).

## Phase 4: User Story 2 - Allowlist de rede por host via nftables (P2, issue #13)

**Goal**: `network=true` com hosts declarados restringe o tráfego de saída aos IPs desses hosts, em
vez do atual liga/desliga total via `--share-net`.

**Independent Test**: plugin com `network` declarando um host específico consegue conectar a esse
host e falha ao conectar a qualquer outro.

- [ ] T009 [US2] Implementar em `sandbox_network.rs`: resolução de host→IP no processo pai (fora do
      sandbox), antes de montar os argumentos do `bwrap` (D4 do `plan.md`)
- [ ] T010 [US2] Implementar em `sandbox_network.rs`: geração de um script wrapper (inline, string
      Rust — sem exigir binário/passo de build extra, ver `plan.md` § Complexity Tracking) que
      aplica regras `nftables` (fallback `iptables` se `nft` ausente) restringindo saída aos IPs
      resolvidos em T009, e então faz `exec` do comando real do plugin
- [ ] T011 [US2] Em `sandbox.rs:170-206`, quando `allow_network=true` com allowlist de hosts não
      vazia: usar o wrapper de T010 como comando efetivo dentro do `bwrap` (em vez de exec direto do
      comando do plugin), preservando o `--share-net`/binds de DNS/TLS já existentes
- [ ] T012 [US2] Estender `sandbox_unit_tests`: quando `network=true` com hosts declarados, os
      argumentos/wrapper gerados refletem os hosts esperados; quando `network=true` sem allowlist
      (comportamento atual), nada muda (regressão)
- [ ] T013 [US2] Estender `sandbox_integration_tests` com dois servidores HTTP de fixture locais em
      endereços/portas de loopback distintos simulando "host declarado" vs. "host não declarado":
      plugin com allowlist consegue conectar ao host declarado e falha ao conectar ao outro
- [ ] T014 [US2] Testar explicitamente o caso de falha do mecanismo (FR-004): simular ausência de
      `nft`/`iptables` e confirmar que a montagem do sandbox é recusada com mensagem clara
      (`NetworkFirewallUnavailable`, T003), nunca voltando ao comportamento antigo (liga/desliga
      total) silenciosamente

**Checkpoint**: US2 entregue e testável de forma independente (issue #13 fechada), sem regressão em
US1.
