# Data Model: Sandbox de Plugins via Bubblewrap

**Feature**: `006-sandbox-permissoes-bubblewrap` | **Data**: 2026-09-03

Nenhum tipo novo em `farol-protocol` (o `CapabilityManifest` já existente não muda de forma — ver
`research.md` D1). Todos os tipos abaixo são internos a `farol-core`, novo módulo `sandbox.rs`.

## `SandboxProfile`

Resolvido pelo core, por plugin, a partir do registro estático em `known_plugins()` (D1) —
**nunca** a partir do `CapabilityManifest` recebido em runtime.

| Campo | Tipo | Descrição |
|---|---|---|
| `allow_network` | `bool` | `true` ⟹ `--share-net` + binds de DNS/TLS; `false` ⟹ `--unshare-all` puro (D2). |
| `allow_exec` | `bool` | `true` ⟹ bind read-only de `/usr/bin`, `/bin`, `/usr/local/bin`; `false` ⟹ nenhum desses (D3). |
| `extra_binds` | `Vec<BindMount>` | Binds adicionais nomeados por plugin (D5/D6) — vazio para a maioria; um item para `git-local` (`scan_root`), um item para `docker-containers` (socket Docker). |

## `BindMount`

| Campo | Tipo | Descrição |
|---|---|---|
| `host_path` | `PathBuf` | Caminho no host a bindar. Resolvido antes do spawn (ex.: `scan_root` lido do `config.toml` de `git-local`, replicando `plugins/git-local/config.py::load_scan_root`, D5). |
| `writable` | `bool` | `true` para os dois casos especiais conhecidos (D5/D6) — ambos precisam escrever (`git fetch` grava em `.git/`; o socket Docker precisa de leitura/escrita para o protocolo do daemon). |

`sandbox_path` não é um campo separado — todo bind desta feature usa o mesmo caminho no host e no
sandbox (`SRC == DEST`), sem remapeamento; simplifica a composição dos argumentos de `bwrap` e evita
qualquer plugin precisar saber que está rodando sob um caminho diferente do real.

## `PluginSpawnConfig` (extensão do tipo já existente em `plugin_worker.rs`)

Ganha um novo campo:

| Campo novo | Tipo | Descrição |
|---|---|---|
| `sandbox_profile` | `SandboxProfile` | Perfil resolvido estaticamente para este plugin, junto de `plugin_name`/`command`/`args` já existentes — mesma disciplina de "registro hardcoded mantido por quem edita `known_plugins()`" já documentada para os campos existentes. |

Nenhum campo existente (`plugin_name`, `command`, `args`) muda de forma.

## Relação com o `CapabilityManifest` do protocolo (inalterado)

```text
                    handshake/hello (runtime, pós-spawn)
Plugin  ───────────────────────────────────────────────►  CapabilityManifest
                                                            (exibido na UI — view.rs,
                                                             SEM mudança nesta feature)

known_plugins() (estático, pré-spawn)
   │
   ├─► PluginSpawnConfig.sandbox_profile ──► argumentos de `bwrap` ──► processo filho real
   │
   └─► (mantido em sincronia manualmente com o que o plugin declara no handshake —
        mesma responsabilidade humana já documentada para `plugin_name` bater entre os dois lados)
```

## Estado que NÃO muda nesta feature

- `model::PluginConnection`, `model::PluginState`/`UnavailableReason` — nenhuma variante nova
  (`research.md` D8).
- `secrets_store.rs`/`config_store.rs` — nenhuma mudança de assinatura ou formato (`research.md` D9).
- Qualquer schema em `protocol/schema/` — esta feature não bump a versão do protocolo (nenhuma
  mudança de wire format; `CapabilityManifest` já suporta o `kind: "network"` que `git-local`/
  `openfortivpn-vpn` passam a declarar, D7 — é só um novo valor de um enum já existente, não um
  campo novo).
