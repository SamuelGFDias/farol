# Contrato: `farol-plugin.toml` e fluxo de instalação

**Feature**: `007-registry-instalacao-plugins-github` | **Normativo para**: `plugin_manifest.rs`,
`install.rs`, `plugin_worker::discover_installed_plugins`

## Schema de `farol-plugin.toml`

```toml
# Campos obrigatórios
plugin_name = "string não-vazia"   # MUST ser único entre os plugins ativos (D6)
command = "string"                  # ex. "python3" — resolvido em $PATH como hoje
args = ["string", "..."]            # relativos ao diretório do próprio plugin (code_root)

# Seção opcional — ausente equivale a { network = false, exec = false }
[capabilities]
network = false   # bool, default false
exec = false       # bool, default false
```

Campos fora deste schema MUST ser ignorados na leitura (tolerância a campo desconhecido, D1).

## Contrato de `parse_manifest(path: &Path) -> Result<PluginManifest, ManifestError>`

| Condição de entrada | Resultado |
|---|---|
| Arquivo não existe em `path` | `Err(ManifestError::NotFound(path))` |
| Conteúdo não é TOML válido | `Err(ManifestError::InvalidToml(mensagem))` |
| `plugin_name` ausente | `Err(ManifestError::MissingField("plugin_name"))` |
| `plugin_name` presente mas vazio/só espaço | `Err(ManifestError::EmptyPluginName)` |
| `command` ausente | `Err(ManifestError::MissingField("command"))` |
| `args` ausente | `Err(ManifestError::MissingField("args"))` (`args = []` é válido — lista vazia não é "ausente") |
| Tudo presente e válido | `Ok(PluginManifest { .. })` |

## Contrato de `discover_installed_plugins() -> Vec<PluginSpawnConfig>`

1. Resolve `farol_data_base_dir()/plugins/` (D2) — se o diretório não existir, devolve `vec![]`
   (nenhum aviso — "nenhum plugin instalado" é o estado normal na maioria das máquinas).
2. Para cada subdiretório direto (`std::fs::read_dir`, ordem determinística dentro da execução):
   chama `parse_manifest(subdir.join("farol-plugin.toml"))`.
   - `Err(_)`: emite aviso (`eprintln!`) citando o subdiretório e o erro, **não** interrompe a
     varredura dos demais (FR-004).
   - `Ok(manifest)`: constrói `PluginSpawnConfig { plugin_name: manifest.plugin_name, command:
     manifest.command, args: manifest.args, sandbox_profile: SandboxProfile { allow_network:
     manifest.capabilities.network, allow_exec: manifest.capabilities.exec, extra_binds: vec![] },
     code_root: subdir }`.
3. Devolve a lista de `PluginSpawnConfig` construídos com sucesso, na ordem de varredura.

## Contrato de filtragem de colisão (`main.rs::Farol::default`, D6)

```text
final = known_plugins()  # 4 de referência, sempre presentes
for spawn_config in discover_installed_plugins():
    if spawn_config.plugin_name in known_plugins().map(|c| c.plugin_name):
        eprintln!(aviso de colisão com plugin de referência)
        continue
    if spawn_config.plugin_name in final.map(|c| c.plugin_name):
        eprintln!(aviso de colisão entre dois plugins instalados)
        continue
    final.push(spawn_config)
```

## Contrato do fluxo de instalação (`farol install <owner>/<repo>`)

**Entrada**: `owner/repo` (string, um único `/`) — formato inválido (zero ou mais de um `/`) MUST
falhar imediatamente com mensagem clara, sem tentar rede nenhuma.

**Passos** (D5 de `research.md`, cada um validado empiricamente contra a API real do GitHub nesta
sessão):

1. `GET {FAROL_GITHUB_API_BASE}/repos/{owner}/{repo}/releases/latest`
   (`FAROL_GITHUB_API_BASE` default `https://api.github.com`, override só de teste, D8).
   - Status `404` → `InstallOutcome::NoRelease`.
   - Status diferente de `200`/`404` → `InstallOutcome::DownloadFailed` (mensagem cita o status).
   - Status `200` → extrai `tag_name`/`tarball_url` do JSON. Campo ausente → tratado como
     `DownloadFailed` (resposta inesperada da API).
2. `GET {tarball_url}` → grava em arquivo temporário. Falha de rede/HTTP não-2xx →
   `InstallOutcome::DownloadFailed`.
3. Extrai o tarball (`tar -xzf ... --strip-components=1`) para um diretório de staging temporário
   (`mktemp -d` ou equivalente, fora de `installed_plugin_dir` — nunca escreve direto no destino
   final antes de validar, FR-007).
4. `parse_manifest(staging/farol-plugin.toml)`:
   - `Err(e)` → `InstallOutcome::ManifestInvalid(e)`; staging é removido, nada é publicado.
   - `Ok(manifest)` com `plugin_name` colidindo com um dos 4 de referência →
     `InstallOutcome::NameCollision`; staging é removido, nada é publicado.
   - `Ok(manifest)` válido → segue para o passo 5.
5. Rename atômico: remove `installed_plugin_dir(manifest.plugin_name)` se já existir (FR-008,
   substituição limpa), depois `rename(staging, installed_plugin_dir(manifest.plugin_name))`.
   Sucesso → `InstallOutcome::Installed { plugin_name, path }`.

**Saída do processo**: `InstallOutcome::Installed` → código `0`, mensagem de sucesso citando o
caminho instalado. Qualquer outra variante → código `1`, mensagem específica da causa (nunca
genérica, FR-010).
