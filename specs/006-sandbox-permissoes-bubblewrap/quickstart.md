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

## Automação equivalente (a preencher durante `/speckit-implement`)

Como nas features anteriores, os Cenários acima devem, quando possível, ganhar equivalentes
automatizados: testes de unidade puros de composição de argumentos (`sandbox.rs`), testes de
integração real com `bwrap` (`research.md` D11, positivo e negativo para rede/exec), e a suíte de
regressão já existente (`e2e_tests.rs`, `harness.sh`) continuando a passar sem alteração de
expectativa — só passando a exercitar, por baixo dos panos, o caminho sandboxed.
