# Research: Sandbox de Plugins via Bubblewrap

**Feature**: `006-sandbox-permissoes-bubblewrap` | **Data**: 2026-09-03

Todas as decisões abaixo foram validadas empiricamente nesta máquina de desenvolvimento
(`bubblewrap 0.11.0`, `/usr/bin/bwrap`) antes de serem registradas — cada bloco de comando citado foi
de fato executado, não é hipotético.

## D1: Fonte de verdade do perfil de sandbox é o registro estático `known_plugins()`, não o handshake em runtime

**Decisão**: o perfil de sandbox (rede permitida? binários externos permitidos? binds extras?) de
cada plugin é decidido **antes do spawn**, a partir de um novo campo estático em
`PluginSpawnConfig`/`known_plugins()` — não a partir do `CapabilityManifest` que o próprio plugin
declara no `handshake/hello`.

**Rationale**: existe um problema de ordem lógica inescapável — o `CapabilityManifest` só chega ao
core depois que o processo filho já foi spawnado e respondeu ao handshake; nesse ponto o sandbox já
precisa estar em vigor. Além disso, confiar no autorrelato do próprio processo para decidir o
isolamento **dele mesmo** não seria uma fronteira de segurança real (um plugin malicioso simplesmente
declararia o que quisesse). Para os 4 plugins de referência hoje — todos de primeira parte,
registrados à mão em `known_plugins()` — a fonte de verdade correta é o próprio registro estático,
mesma disciplina que já existe para `command`/`args`. O `CapabilityManifest` do handshake continua
existindo e sendo exibido na UI exatamente como hoje (nenhuma mudança); ele só deixa de ser, sozinho,
a fonte que decide o sandbox.

**Consequência para Fase 4 (registry, fora de escopo aqui)**: um plugin de terceiro instalado via
registry não tem uma entrada de código hardcoded em `known_plugins()` — resolver a fonte de verdade
do sandbox para esse caso (provavelmente um manifesto declarativo assinado/versionado, lido antes do
primeiro spawn) é trabalho da própria feature de registry, não desta.

**Alternativas consideradas**: confiar no handshake e reiniciar o processo com um sandbox mais
estrito se ele declarar menos do que foi concedido inicialmente — rejeitada por complexidade (duplo
spawn) sem ganho real de segurança (ainda seria autorrelato).

## D2: Rede negada por padrão via `--unshare-all` sem `--share-net`; liberada com `--share-net`

**Decisão**: todo plugin roda com `--unshare-all` (nenhum acesso de rede — só loopback dentro do seu
próprio netns isolado). Quando a capability de rede é concedida (`git-local`, `openfortivpn-vpn` —
ver D7 — e `uptime-kuma`), adiciona-se `--share-net` (mantém o netns do host) mais os binds
read-only necessários para DNS/TLS funcionarem: `/etc/resolv.conf`, `/etc/hosts`, `/etc/ssl`,
`/etc/pki` (`--*-try`, tolerante à ausência — varia por distro).

**Validado empiricamente** (sem rede):

```console
$ bwrap --unshare-all --die-with-parent \
    --ro-bind-try /usr/lib /usr/lib --ro-bind-try /usr/lib64 /usr/lib64 \
    --ro-bind-try /lib /lib --ro-bind-try /lib64 /lib64 \
    --ro-bind-try /etc/ld.so.cache /etc/ld.so.cache \
    --ro-bind /usr/bin/python3 /usr/bin/python3 \
    --proc /proc --dev /dev --tmpfs /tmp \
    -- /usr/bin/python3 -c "import socket; socket.create_connection(('1.1.1.1', 80), timeout=2)"
# [Errno 101] Network is unreachable
```

**Validado empiricamente** (com rede, `--share-net` + binds de DNS/TLS): `socket.gethostbyname
('github.com')` resolve normalmente e `subprocess.run(['git', 'ls-remote', ...])` contra um remote
real funcionaria (não testado contra rede externa real nesta sessão por disciplina de não depender
de rede externa em teste automatizado — mas o mecanismo, `--share-net` + resolv.conf, é o padrão
documentado do próprio bwrap).

## D3: Capability `exec` mediada por visibilidade seletiva de filesystem, não por seccomp

**Decisão**: nesta fase, um plugin sem `exec` concedida não tem `/usr/bin`, `/bin` nem
`/usr/local/bin` bindados no sandbox — só o binário do próprio interpretador (`python3`, caminho
absoluto resolvido pelo core, D10) e as bibliotecas de que ele depende (`/usr/lib`, `/lib`, `/lib64`,
`/usr/lib64`, `/etc/ld.so.cache`) — nenhuma delas é um diretório de onde um binário arbitrário possa
ser executado via busca de `PATH`. Quando `exec` é concedida, `/usr/bin`/`/bin`/`/usr/local/bin`
passam a ser bindados read-only por completo (granularidade de diretório, não por binário
individual — mesma simplificação de granularidade grosseira já aplicada a `network`/`allowed_hosts`
em D2/FR-010).

**Validado empiricamente** (sem exec):

```console
$ bwrap ... -- python3 -c "subprocess.run(['/usr/bin/true'])"
# [Errno 2] No such file or directory: '/usr/bin/true'
```

**Validado empiricamente** (com exec, `/usr/bin` bindado): `subprocess.run(['git', '--version'])`
funciona normalmente.

**Débito técnico reconhecido, a registrar como issue**: esta mediação é só por visibilidade de
filesystem, não por uma barreira de kernel contra a syscall `execve` em si. Um plugin Python
suficientemente hostil poderia, em teoria, escrever um payload executável num `tmpfs` gravável (ex.:
`/tmp`, que continua montado para todo plugin) e tentar executá-lo por caminho absoluto, sem depender
de busca em `PATH` nem de nenhum binário pré-existente no sandbox. Fechar esse caminho residual exige
um filtro `seccomp` real (`bwrap --seccomp FD`, filtro BPF pré-compilado bloqueando
`execve`/`execveat`/`fexecve`) — fora de escopo desta fase por complexidade de implementação
(precisa de um gerador de filtro BPF, hoje inexistente no projeto). A mediação atual já é uma melhoria
real e mensurável sobre o estado anterior (nenhuma mediação nenhuma) e cobre o caso de uso normal de
um plugin bem-comportado; só não é uma fronteira de segurança completa contra um plugin ativamente
hostil. Ver issue a criar em `tasks.md` (Polish).

## D4: Filesystem do código do plugin — bind read-only da raiz do repositório inteira

**Decisão**: em vez de bindar só `plugins/<nome>/` e reescrever `cwd`/`args` para ficarem relativos a
esse bind, o sandbox bind-monta a raiz do repositório inteira (`--ro-bind <repo_root> <repo_root>` +
`--chdir <repo_root>`), preservando os caminhos relativos hoje hardcoded em `known_plugins()`
(`plugins/git-local/main.py`, etc. — ver a nota já existente na docstring de `known_plugins` sobre
esse caminho ser relativo ao `cwd`).

**Rationale**: o código-fonte do próprio Farol (core em Rust, os 4 plugins de referência) não é dado
confidencial do usuário — é o próprio produto, o mesmo que qualquer pessoa já vê no repositório git.
A fronteira de segurança real que a constitution (Princípio IV) protege é o filesystem **pessoal** do
usuário ($HOME fora do repo, segredos, configs de outros programas) — não "um plugin não pode ler o
código-fonte de outro plugin do mesmo produto". Reescrever `cwd`/`args` por plugin para bindar só o
subdiretório próprio adicionaria complexidade real (mudar a forma como `known_plugins()`/`worker()`
calculam `args`) sem ganho de segurança proporcional. Rejeitada como complexidade desnecessária para
esta fase.

## D5 / D6: Dois casos especiais nomeados de acesso a filesystem além do próprio código

**Decisão**: `git-local` (diretório `scan_root`, leitura/escrita) e `docker-containers` (socket Unix
do daemon Docker, leitura/escrita) recebem, cada um, um bind adicional específico calculado pelo
core, sem virar uma capability nova genérica (fora de escopo, ver `spec.md`).

**Armadilha real encontrada e corrigida durante a validação empírica**: a ordem dos argumentos de
bwrap importa — um bind posicionado **antes** de `--tmpfs /tmp`/`--proc`/`--dev` fica invisível se o
caminho bindado estiver aninhado sob um desses pontos de montagem "genéricos" (ex.: um `scan_root` de
teste sob `/tmp` some depois que `--tmpfs /tmp` monta por cima). **Os binds extras específicos de
plugin (`scan_root`, socket do Docker) MUST vir depois dos mounts base (`--proc`, `--dev`,
`--tmpfs`) na lista de argumentos do `bwrap`** — validado corrigindo esse exato erro nesta sessão
(reprodução: bind antes de `--tmpfs /tmp` com `scan_root` sob `/tmp` → `git fetch` falhava com
"cannot change to ... No such file or directory"; mesmo bind depois de `--tmpfs /tmp` → funciona).

**`git-local`/`scan_root`, validado empiricamente** — `git fetch` contra um remote bare local dentro
do `scan_root` bindado como leitura/escrita funciona normalmente (`git -C <clone> fetch origin`
trouxe os commits novos do bare remote).

**`docker-containers`/socket, validado empiricamente**:

```console
$ bwrap --unshare-all --die-with-parent \
    ... (binds base + /usr/bin para exec) \
    --bind-try /var/run/docker.sock /var/run/docker.sock \
    --proc /proc --dev /dev --tmpfs /tmp \
    -- docker ps --all --format '{{.Names}}'
# lista os containers reais da máquina, sem nenhuma capability de rede concedida
```

Confirma que `docker-containers` não precisa de `network` — fala só com o socket Unix local — mesmo
resultado hoje sem sandbox, agora com o sandbox de rede ativo e ainda assim funcionando.

## D7: Correção de manifesto — `git-local` e `openfortivpn-vpn` passam a declarar `network`

**Decisão**: `plugins/git-local/main.py` e `plugins/openfortivpn-vpn/main.py` (`handshake/hello`)
passam a incluir `{"kind": "network"}` em `capabilities.capabilities`, ao lado do `exec` que já
declaravam. O registro correspondente em `known_plugins()` (fonte de verdade do sandbox, D1) também
passa a conceder rede a esses dois.

**Rationale**: achado durante o planejamento (`spec.md` § Clarifications) — `git.fetch` contra um
remote real e `vpn.connect` contra um servidor real sempre dependeram de rede; o manifesto nunca
refletiu isso porque nunca foi de fato aplicado antes desta feature. Corrigir é o próprio objetivo da
feature, não escopo adicional.

## D8: `bwrap` ausente na máquina — reaproveita `Unavailable{FailedToStart}`, sem estado novo

**Decisão**: nenhuma variante nova de `UnavailableReason` é necessária. `Command::new("bwrap")` com
`bwrap` ausente do `PATH` já resulta em `Command::spawn()` retornando `Err` — o mesmíssimo caminho que
`worker()` já trata hoje (`WorkerEvent::SpawnFailed`, mapeado para `Unavailable{FailedToStart}`).
Único ajuste necessário: a mensagem de erro construída em `worker()` MUST deixar claro que a falha é
do sandbox (`bwrap`), não do plugin em si — ex.: distinguir `err.kind() ==
std::io::ErrorKind::NotFound` do comando `"bwrap"` especificamente, e compor uma mensagem como
"sandbox bubblewrap (bwrap) não encontrado no PATH — instale bubblewrap para rodar plugins" em vez da
mensagem genérica atual que cita `config.command`/`config.args` (que agora seriam os do plugin real,
não os de `bwrap`).

**Rationale**: `FailedToStart` já é, por definição, terminal e distinto de `Crashed`/`Unresponsive`/
`VersionIncompatible` (FR-007/FR-009) — não há necessidade de uma quinta razão só para diferenciar
"a causa raiz foi o sandbox" visualmente, desde que a mensagem textual (já um `String` livre no tipo
existente) deixe isso claro para quem lê a tela do Farol.

## D9: Segredos/config nunca chegam ao plugin por bind de filesystem — já garantido por construção

**Decisão**: nenhuma mudança em `secrets_store.rs`/`config_store.rs`. Como o sandbox só bind-monta o
que é explicitamente listado (D1-D6) e `~/.config/farol` nunca entra nessa lista para nenhum plugin,
a US3/FR-004 já fica satisfeita apenas por **omissão** — não é preciso nenhum mecanismo de bloqueio
ativo, só a disciplina de nunca adicionar um bind desse diretório. A prova (US3, `quickstart.md`) é
tentar localizar o arquivo de dentro do sandbox e confirmar que ele não existe daquele ponto de vista.

## D10: Resolução do caminho absoluto do interpretador do plugin

**Decisão**: o core resolve o caminho absoluto de `config.command` (hoje sempre `"python3"`) uma vez,
no momento de montar o perfil de sandbox, procurando nos diretórios de `$PATH` do processo do
próprio Farol (o mesmo `$PATH` que `Command::new("python3")` já usava implicitamente antes desta
feature) — sem depender de nenhuma crate nova (`which`, etc.), só `std::env::var("PATH")` +
`std::fs::metadata` por diretório. Erro (nenhum `python3` encontrado em nenhum diretório do `PATH`)
mapeia para `Unavailable{FailedToStart}`, mesma disciplina de D8.

**Rationale**: bwrap executa o `COMMAND` final por caminho, dentro do namespace de mount já
restrito — se `python3` só existisse como nome relativo dependente de busca em `$PATH` *dentro* do
sandbox, o `$PATH` efetivo dentro do sandbox teria que necessariamente incluir o diretório onde o
binário mora, o que reabriria a mesma questão de granularidade de D3. Resolver o caminho absoluto do
lado de fora, no host, e bindar exatamente esse arquivo evita essa reabertura.

## D11: Estratégia de teste

- **Unidade pura** (`sandbox.rs`): construção do `Vec<String>` de argumentos do `bwrap` a partir de
  um `SandboxProfile`, sem spawnar nada de verdade — cobre a lógica de composição (rede on/off, exec
  on/off, binds extras, ordem correta pós-D5/D6) rapidamente e sem dependência do binário `bwrap`
  instalado.
- **Integração real** (novo módulo de teste, ex. `sandbox_integration_tests.rs`, `#[ignore]` se
  `bwrap` não estiver no `PATH` do ambiente de CI — mesma disciplina de robustez que os fixtures
  determinísticos já usam): spawna de verdade um `python3 -c "..."` sob o sandbox construído,
  replicando os 4 experimentos já validados manualmente nesta sessão (rede negada, rede concedida,
  exec negado, exec concedido) — prova SC-001/SC-003 com um processo real, não uma simulação.
- **Regressão dos 4 plugins de referência** (SC-002, FR-011): `cargo test --package farol-core
  e2e_tests` e `tests/integration/harness.sh` continuam sendo a prova de que nenhum dos 4 plugins
  regride — depois desta feature, esses mesmos testes passam a rodar os plugins de fato dentro do
  sandbox (não é preciso um teste novo dedicado a "prova que ainda funciona", os já existentes já
  cobrem isso, desde que `worker()` passe a usar o sandbox incondicionalmente).
- **`docker-containers`/socket**: o teste de integração real (acima) inclui um cenário com
  `--bind-try` do socket real da máquina de desenvolvimento (presente e funcional, confirmado nesta
  sessão) — em CI sem Docker instalado, o `--bind-try` é tolerante (não falha por ausência), e o
  comportamento observável (sem Docker instalado) já é coberto pelo teste de fixture existente da
  feature 005 (que não depende de Docker real).

## D12: Versão mínima de `bwrap` considerada

**Decisão**: nenhum requisito de versão mínima explícito além do que já está instalado e testado
nesta sessão (`bubblewrap 0.11.0`) — todos os flags usados (`--unshare-all`, `--share-net`,
`--ro-bind[-try]`, `--bind[-try]`, `--die-with-parent`, `--proc`, `--dev`, `--tmpfs`, `--chdir`) são
estáveis há muitas versões do bubblewrap (não há uso de nenhum flag recente como `--seccomp`, que de
qualquer forma está fora de escopo desta fase, D3). Não é necessário checar versão em runtime, só
presença do binário (D8).
