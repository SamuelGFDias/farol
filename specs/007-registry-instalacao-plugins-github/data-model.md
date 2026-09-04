# Data Model: Registry — Descoberta e Instalação de Plugins de Terceiros via GitHub

**Feature**: `007-registry-instalacao-plugins-github` | **Data**: 2026-09-04

## `PluginManifest` (novo tipo, `farol-core`, ex. módulo `plugin_manifest.rs`)

Desserializado de `farol-plugin.toml` (D1 de `research.md`).

| Campo | Tipo TOML | Obrigatório | Descrição |
|---|---|---|---|
| `plugin_name` | string | sim | Nome do plugin — vira a chave de `PluginConnection`/diretório de instalação; MUST ser não-vazio. |
| `command` | string | sim | Comando a spawnar (ex. `"python3"`), resolvido em `$PATH` como hoje (`sandbox::resolve_interpreter_path`). |
| `args` | array de string | sim | Argumentos, relativos ao próprio diretório do plugin (`code_root`, D4) — ex. `["main.py"]`. |
| `capabilities.network` | bool | não (default `false`) | Vira `SandboxProfile.allow_network`. |
| `capabilities.exec` | bool | não (default `false`) | Vira `SandboxProfile.allow_exec`. |

Campos desconhecidos MUST ser ignorados na desserialização (D1). `extra_binds` do `SandboxProfile`
resultante é sempre `vec![]` para plugins instalados via registry (fora de escopo, `spec.md`).

## `ManifestError` (novo tipo)

Enum de erro de `parse_manifest` (D7): `NotFound(PathBuf)`, `InvalidToml(String)`,
`MissingField(&'static str)`, `EmptyPluginName`. Usado tanto por `discover_installed_plugins()`
(US1, erro vira aviso + plugin ignorado, FR-004) quanto pelo fluxo de instalação (US2, erro vira
falha do comando com mensagem clara, FR-010).

## Extensão de `PluginSpawnConfig` (`plugin_worker.rs`, já existente desde a feature 006)

| Campo novo | Tipo | Descrição |
|---|---|---|
| `code_root` | `PathBuf` | Generaliza o antigo cálculo interno de `repo_root` em `worker()` (D4) — raiz do repositório Farol para os 4 plugins de referência, diretório de instalação (`installed_plugin_dir(nome)`) para plugins descobertos. |

Nenhum campo existente (`plugin_name`, `command`, `args`, `sandbox_profile`) muda de forma.

## `InstallOutcome` (novo tipo, resultado do fluxo de instalação, D5)

Não é persistido — só o retorno interno de `install::run(owner, repo)`, traduzido para código de
saída do processo (`0` sucesso, `1` falha) e mensagem em stdout/stderr:

| Variante | Código de saída | Quando |
|---|---|---|
| `Installed { plugin_name, path }` | `0` | Sucesso — manifesto validado, instalado em `path`. |
| `NoRelease` | `1` | `GET /releases/latest` devolveu 404 (Edge Case). |
| `DownloadFailed(String)` | `1` | `curl` do tarball falhou (rede, HTTP não-2xx). |
| `ManifestInvalid(ManifestError)` | `1` | Tarball extraído não contém `farol-plugin.toml` válido. |
| `NameCollision(String)` | `1` | `plugin_name` do manifesto colide com um dos 4 de referência. |

## Relação entre os tipos

```text
farol-plugin.toml (arquivo, no repo do plugin OU já instalado)
        │
        │ parse_manifest() (D7, compartilhado)
        ▼
  PluginManifest ──► SandboxProfile { allow_network, allow_exec, extra_binds: vec![] }
        │
        │ (nome + command + args + code_root do diretório onde foi lido)
        ▼
  PluginSpawnConfig { plugin_name, command, args, sandbox_profile, code_root }
        │
        ├─► discover_installed_plugins() devolve Vec<PluginSpawnConfig> (US1)
        │
        └─► install::run() só usa PluginManifest para VALIDAR antes do rename
            atômico (D5) — não constrói PluginSpawnConfig diretamente; quem faz
            isso é a próxima descoberta, na próxima abertura do Farol (FR não
            promete hot-reload).
```

## Estado que NÃO muda nesta feature

- `known_plugins()` — os 4 `PluginSpawnConfig` hardcoded, só ganham o campo novo `code_root`
  (D4), sem mudança de `plugin_name`/`command`/`args`/`sandbox_profile`.
- `sandbox::SandboxProfile`/`BindMount`/`build_bwrap_args` — nenhuma mudança de forma; só passam a
  ser preenchidos/chamados também a partir de um `PluginManifest` descoberto, além do registro
  estático.
- `model::PluginConnection`/`PluginState` — nenhuma mudança; um plugin descoberto passa pela mesma
  máquina de estados de qualquer plugin já existente.
- `config_store.rs`/`secrets_store.rs` — nenhuma mudança de formato/localização (continuam em
  `$XDG_CONFIG_HOME`, distinto do novo `$XDG_DATA_HOME` desta feature, D2).
