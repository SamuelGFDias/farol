# Quickstart: Validação do Sandbox de Plugins via Bubblewrap

**Feature**: `006-sandbox-permissoes-bubblewrap` | **Data**: 2026-09-03

Guia para validar manualmente, ponta a ponta, as três User Stories da spec depois que a feature
estiver implementada. Mesmo padrão de `specs/004-vpn-status-plugin/quickstart.md` — não contém
código de implementação, só comandos e resultados esperados.

## Pré-requisitos

- `bwrap` instalado (`/usr/bin/bwrap`, confirmado nesta máquina — `bubblewrap 0.11.0`).
- `cargo build --workspace` bem-sucedido, com `sandbox.rs` novo integrado a `plugin_worker.rs`.
- Ambiente Linux com suporte a namespaces de usuário/mount/rede (padrão em qualquer distro atual).

## Cenário 1 — Um plugin sem `network` não alcança rede nenhuma (User Story 1, P1)

```bash
cargo run -p farol-core
```

**Esperado** (SC-001): `docker-containers` (o único dos 4 plugins de referência sem `network` nesta
fase, `contracts/bwrap-invocation-contract.md`) continua chegando a `Ready` e populando o widget
normalmente — porque fala só com o socket Unix do Docker, não com rede — mas se tentasse abrir uma
conexão de rede de qualquer tipo, falharia. Confirmar por leitura de log/comportamento que nenhuma
chamada de rede é feita nem teria como ser feita por esse plugin.

## Cenário 2 — Os 4 plugins de referência continuam funcionando sob sandbox (User Story 1+2, regressão SC-002)

```bash
tests/integration/harness.sh
```

**Esperado**: `SUCESSO — 7/7 condições confirmadas em Ns` (mesma saída já observada nas features
004/005), agora com todo processo filho rodando dentro de `bwrap` — sem regressão de comportamento
observável, tempo de execução na mesma ordem de grandeza do já registrado (feature 005: ~9s).

## Cenário 3 — `git-local` continua lendo/escrevendo no `scan_root` sob sandbox (User Story 2, P2)

```bash
cargo run -p farol-core
```

**Esperado** (FR-008a): o widget "Repositórios Git" mostra os repositórios reais de `~/dev` (ou do
`scan_root` configurado), e a ação `git.fetch` continua funcionando normalmente — sem diferença
percebida em relação ao comportamento pré-sandbox da feature 001.

## Cenário 4 — `docker-containers` continua falando com o daemon Docker sob sandbox (User Story 2, P2)

```bash
cargo run -p farol-core
```

**Esperado** (FR-008b): o widget "Containers Docker" mostra os containers reais da máquina (se
Docker estiver instalado e rodando) — a chamada ao socket `/var/run/docker.sock` continua
funcionando mesmo com `allow_network: false` para este plugin, provando que socket Unix local e rede
são coisas diferentes na prática.

## Cenário 5 — Nenhum plugin enxerga o arquivo de segredos por filesystem (User Story 3, P3)

Pré-requisito: `uptime-kuma` configurado com um `base_url`/`api_key` reais (tela de setup).

**Esperado** (SC-005): o widget "Uptime Kuma" continua populado normalmente (o segredo chega por
`FAROL_PLUGIN_UPTIME_KUMA_API_KEY`, variável de ambiente, como já ocorria antes desta feature) — e,
inspecionando de fora (ex.: um `strace`/log de diagnóstico temporário do processo sandboxed durante o
desenvolvimento, removido antes do commit), confirmar que nenhuma tentativa de abrir
`~/.config/farol/secrets.toml` por caminho de filesystem retorna sucesso.

## Cenário 6 — `bwrap` ausente do `PATH` (Edge Case, FR-007)

```bash
PATH=/usr/bin:/bin:$(dirname "$(command -v python3)") cargo run -p farol-core
# (reduzido para excluir o diretório onde bwrap normalmente mora, se for diferente de /usr/bin)
```

Numa máquina onde `bwrap` de fato não esteja instalado, o teste é direto: `sudo dnf remove
bubblewrap` (ou equivalente) antes de rodar — não recomendado na máquina de desenvolvimento principal;
preferir validar este cenário só por leitura de código/teste automatizado (`sandbox.rs`, D8) em vez
de desinstalar `bwrap` de uma máquina em uso.

**Esperado**: todo plugin fica `Unavailable{FailedToStart}` com mensagem clara mencionando
"bubblewrap"/"bwrap" — não roda sem sandbox como fallback silencioso.

## Automação equivalente

Como previsto, os Cenários acima ganharam equivalentes automatizados em dois módulos novos dentro de
`crates/farol-core/src/sandbox.rs` (`research.md` D11), mais a suíte de regressão já existente das
features anteriores, agora exercitando o caminho sandboxed por baixo dos panos sem alteração de
expectativa:

- `sandbox_unit_tests` — composição pura do `Vec<String>` de argumentos do `bwrap`, sem spawnar nada
  de verdade:
  - `no_network_no_exec_omits_share_net_and_usr_bin` — perfil mínimo (Cenário 1, metade "sem rede").
  - `network_profile_adds_share_net_and_dns_binds` — `--share-net` + binds de DNS/TLS quando a
    capability `network` está presente (Cenário 1, metade "com rede").
  - `exec_profile_adds_usr_bin_and_bin` — `/usr/bin`/`/bin` só entram com `exec` concedida.
  - `extra_binds_appear_and_reflect_writable_flag` — binds extras (`scan_root`, socket Docker)
    aparecem com a flag de leitura/escrita correta (Cenário 3/4).
  - `extra_binds_come_after_tmpfs_tmp` — regressão nomeada da armadilha de ordem de argumentos (D5):
    os mounts genéricos (`--proc`/`--dev`/`--tmpfs`) MUST vir antes de qualquer bind extra.
  - `no_known_plugin_binds_the_farol_config_base_dir` — nenhum perfil dos 4 plugins de referência
    bind-monta `~/.config/farol` (Cenário 5).
  - `final_segment_uses_absolute_interpreter_path_and_preserves_args` — o `COMMAND` final do `bwrap`
    usa o caminho absoluto do interpretador resolvido pelo core (D10), preservando `args` do plugin.
- `sandbox_integration_tests` — spawn real de `bwrap`, **sem `#[ignore]`** (D11: `bwrap` está
  confirmado presente no `PATH` desta máquina de desenvolvimento; num ambiente sem `bwrap`
  instalado, estes testes falham alto — `.expect(...)`/`assert!` não batendo — em vez de serem
  pulados silenciosamente):
  - `network_denied_blocks_outbound_connection` / `exec_denied_blocks_external_binary` — os dois
    experimentos negativos genéricos de D2/D3 (Cenário 1, metade "sem rede"; base do Cenário 6 por
    leitura de código).
  - `docker_containers_real_profile_denies_network` — perfil real de `docker-containers` (único dos
    4 sem `network`) confirma rede bloqueada (Cenário 1).
  - `uptime_kuma_real_profile_denies_exec` / `uptime_kuma_real_profile_allows_local_tcp_connection` —
    perfil real de `uptime-kuma` (sem `exec`, com `network`) nos dois sentidos (Cenário 1 positivo).
  - `git_local_profile_allows_git_fetch_via_extra_bind` — `git fetch` real contra um remote bare
    local dentro do `scan_root` bindado (Cenário 3).
  - `docker_containers_real_profile_allows_docker_ps_via_socket` — `docker ps` real via
    `--bind-try` do socket Unix, sem capability `network` nenhuma (Cenário 4).
  - `uptime_kuma_real_profile_cannot_read_farol_secrets` — `secrets.toml` inacessível de dentro do
    sandbox mesmo para o único plugin com segredo real configurado (Cenário 5).
- Regressão dos 4 plugins de referência (SC-002, FR-011): `cargo test --package farol-core e2e_tests`
  e `tests/integration/harness.sh` (Cenário 2) continuam passando sem teste novo dedicado — `worker()`
  passou a usar o sandbox incondicionalmente, então a cobertura já existente das features 001/002/
  004/005 passou a provar, de graça, que nenhum dos 4 regride sob `bwrap`.

O Cenário 6 (`bwrap` ausente do `PATH`) permanece validável só por leitura de código (D8) — não há
teste automatizado que desinstale `bubblewrap` da máquina de CI. A parte de rede real contra um
servidor genuíno (`git.fetch`/`vpn.connect` fora de um remote/fixture local) também segue só manual,
mesma ressalva já registrada em `research.md` (achado 2 da seção Clarifications) e no quickstart da
feature 004.
