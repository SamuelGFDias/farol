# Contrato: Composição dos argumentos de `bwrap` por perfil de sandbox

**Feature**: `006-sandbox-permissoes-bubblewrap` | **Normativo para**: `crates/farol-core/src/sandbox.rs`

Todos os blocos abaixo foram validados empiricamente nesta sessão (`bubblewrap 0.11.0`,
`research.md` D2/D3/D5/D6) antes de virarem contrato. `SRC`/`DEST` sempre idênticos (`data-model.md`,
`BindMount` sem remapeamento).

## Regra de ordem (MUST)

Os argumentos MUST seguir esta ordem relativa. **Correção de escopo (2026-09-03, achada na
verificação da Camada 2/`harness.sh`)**: a primeira versão deste contrato só posicionava
`extra_binds` depois de `--proc`/`--dev`/`--tmpfs /tmp` (D5) — mas o mesmo problema de sombreamento
atinge **qualquer** bind de caminho real que esteja aninhado sob `/tmp`, não só `extra_binds`. Prova
empírica desta correção: o `harness.sh` resolve o interpretador (`python3`) para um shim de teste
sob `$(mktemp -d)` (tipicamente `/tmp/farol-harness-XXXX/bin/python3`) — com o bind do interpretador
posicionado *antes* de `--tmpfs /tmp` (ordem original), `bwrap` falhava com `execvp ...: No such file
or directory`; movendo esse mesmo bind para *depois* de `--tmpfs /tmp`, o mesmo comando funciona
(`SHIM RAN OK`). **Regra corrigida: TODO bind de caminho real do host (interpretador, DNS/TLS, `/usr/
bin` etc., raiz do repositório, `extra_binds`) MUST vir depois dos mounts sintéticos genéricos
(`--proc`, `--dev`, `--tmpfs /tmp`), nunca antes** — só flags de namespace vêm antes deles.

1. Flags de namespace (`--unshare-all`, `--share-net` se rede concedida) + `--die-with-parent`.
2. `--proc /proc`, `--dev /dev`, `--tmpfs /tmp` — **movido para logo depois do passo 1**, antes de
   qualquer bind real, exatamente para que nenhum bind subsequente corra risco de estar aninhado sob
   um desses três caminhos e ser sombreado.
3. Binds de biblioteca/interpretador (sempre presentes, independem de capability).
4. Binds condicionais a `allow_network` (DNS/TLS).
5. Binds condicionais a `allow_exec` (`/usr/bin`, `/bin`, `/usr/local/bin`).
6. Bind read-only da raiz do repositório + `--chdir`.
7. `extra_binds` do `SandboxProfile` (casos especiais nomeados) — continuam por último entre os
   binds reais, por clareza de composição, mas a posição relativa a 3-6 não é mais o que importa
   (o que importa é estar depois do passo 2).
8. `--` + comando (caminho absoluto resolvido, `research.md` D10) + args.

## Base (sempre presente, qualquer perfil)

```text
--unshare-all
--die-with-parent
--proc /proc
--dev /dev
--tmpfs /tmp
--ro-bind-try /usr/lib /usr/lib
--ro-bind-try /usr/lib64 /usr/lib64
--ro-bind-try /lib /lib
--ro-bind-try /lib64 /lib64
--ro-bind-try /etc/ld.so.cache /etc/ld.so.cache
--ro-bind <caminho absoluto resolvido do interpretador> <mesmo caminho>
```

## Quando `allow_network = true`, adicionar (antes do passo 6 acima)

```text
--share-net           # substitui a ausência dele, não é um bind extra
--ro-bind-try /etc/resolv.conf /etc/resolv.conf
--ro-bind-try /etc/hosts /etc/hosts
--ro-bind-try /etc/ssl /etc/ssl
--ro-bind-try /etc/pki /etc/pki
```

## Quando `allow_exec = true`, adicionar

```text
--ro-bind-try /usr/bin /usr/bin
--ro-bind-try /bin /bin
--ro-bind-try /usr/local/bin /usr/local/bin
```

## Código do plugin (sempre presente)

```text
--ro-bind <repo_root> <repo_root>
--chdir <repo_root>
```

## `extra_binds` conhecidos (D5/D6 de `research.md`) — sempre por último entre os binds reais

| Plugin | `host_path` | `writable` | Motivo |
|---|---|---|---|
| `git-local` | `scan_root` resolvido (mesma leitura de `plugins/git-local/config.py::load_scan_root`, default `~/dev`) | `true` | `git fetch` grava em `.git/` do repositório. |
| `docker-containers` | `/var/run/docker.sock` (`--bind-try`, tolerante à ausência em máquina sem Docker) | `true` | Protocolo do daemon Docker via socket Unix exige leitura/escrita. |

Nenhum outro plugin de referência tem `extra_binds` nesta fase.

## Perfis resolvidos por plugin (D1 — `known_plugins()`, não o handshake)

| Plugin | `allow_network` | `allow_exec` | `extra_binds` |
|---|---|---|---|
| `git-local` | `true` (D7 — correção de manifesto) | `true` | `scan_root` |
| `uptime-kuma` | `true` (já declarava) | `false` (não invoca binário externo) | nenhum |
| `openfortivpn-vpn` | `true` (D7 — correção de manifesto) | `true` | nenhum |
| `docker-containers` | `false` (fala só via socket Unix, não rede) | `true` | socket Docker |

## Casos negativos de referência (para os testes de integração de `sandbox.rs`, D11)

Ambos validados manualmente nesta sessão com um perfil `allow_network: false, allow_exec: false`:

```text
$ ... -- python3 -c "socket.create_connection(('1.1.1.1', 80), timeout=2)"
[Errno 101] Network is unreachable

$ ... -- python3 -c "subprocess.run(['/usr/bin/true'])"
[Errno 2] No such file or directory: '/usr/bin/true'
```

## Resolução de `bwrap` ausente (D8)

`Command::new("bwrap")` com `bwrap` fora do `PATH` do processo do Farol MUST resultar no mesmo
caminho de erro já existente (`WorkerEvent::SpawnFailed` → `Unavailable{FailedToStart}`), com a
mensagem distinguindo explicitamente a causa raiz ("bubblewrap não encontrado") de uma falha do
próprio plugin.
