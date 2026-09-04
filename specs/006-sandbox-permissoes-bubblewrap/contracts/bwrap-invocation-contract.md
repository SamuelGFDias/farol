# Contrato: Composição dos argumentos de `bwrap` por perfil de sandbox

**Feature**: `006-sandbox-permissoes-bubblewrap` | **Normativo para**: `crates/farol-core/src/sandbox.rs`

Todos os blocos abaixo foram validados empiricamente nesta sessão (`bubblewrap 0.11.0`,
`research.md` D2/D3/D5/D6) antes de virarem contrato. `SRC`/`DEST` sempre idênticos (`data-model.md`,
`BindMount` sem remapeamento).

## Regra de ordem (MUST)

Os argumentos MUST seguir esta ordem relativa — violá-la reproduz o bug real encontrado e corrigido
nesta sessão (`research.md` D5, bind sob `/tmp` some depois de `--tmpfs /tmp` montar por cima):

1. Flags de namespace (`--unshare-all`, `--share-net` se rede concedida) + `--die-with-parent`.
2. Binds de biblioteca/interpretador (sempre presentes, independem de capability).
3. Binds condicionais a `allow_network` (DNS/TLS).
4. Binds condicionais a `allow_exec` (`/usr/bin`, `/bin`, `/usr/local/bin`).
5. Bind read-only da raiz do repositório + `--chdir`.
6. `--proc /proc`, `--dev /dev`, `--tmpfs /tmp`.
7. **`extra_binds` do `SandboxProfile` (casos especiais nomeados) — sempre por último, depois do
   passo 6.**
8. `--` + comando (caminho absoluto resolvido, `research.md` D10) + args.

## Base (sempre presente, qualquer perfil)

```text
--unshare-all
--die-with-parent
--ro-bind-try /usr/lib /usr/lib
--ro-bind-try /usr/lib64 /usr/lib64
--ro-bind-try /lib /lib
--ro-bind-try /lib64 /lib64
--ro-bind-try /etc/ld.so.cache /etc/ld.so.cache
--ro-bind <caminho absoluto resolvido do interpretador> <mesmo caminho>
```

## Quando `allow_network = true`, adicionar (antes do passo 5 acima)

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

## Mounts genéricos (sempre presentes, antes de `extra_binds`)

```text
--proc /proc
--dev /dev
--tmpfs /tmp
```

## `extra_binds` conhecidos (D5/D6 de `research.md`) — sempre por último

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
