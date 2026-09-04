//! Manifesto de plugin instalado (`farol-plugin.toml`) e diretório de dados de plugins de
//! terceiros — feature 007 (`specs/007-registry-instalacao-plugins-github`).
//!
//! `farol-plugin.toml` é lido tanto pela descoberta dinâmica
//! (`plugin_worker::discover_installed_plugins`, US1, fora do escopo desta subtarefa) quanto pelo
//! fluxo de instalação (`install::run`, US2, fora do escopo desta subtarefa) — [`parse_manifest`]
//! é a única implementação de validação, compartilhada pelos dois (D7 de `research.md`).
//!
//! Schema exato em `contracts/plugin-manifest-and-install-contract.md` § "Schema de
//! `farol-plugin.toml`": booleanos planos `[capabilities].network`/`exec` mapeando 1:1 para
//! `sandbox::SandboxProfile.allow_network`/`allow_exec` (D1) — sem tradução de formato
//! lista-de-enum-para-booleano como o `CapabilityManifest` do protocolo JSON-RPC usa. Campos
//! desconhecidos MUST ser ignorados na desserialização (D1) — por isso a struct interna de parse
//! não usa `#[serde(deny_unknown_fields)]`.
//!
//! `farol_data_base_dir()`/`installed_plugin_dir()` (D2 de `research.md`) seguem a mesma
//! convenção XDG já usada por `config_store::farol_config_base_dir()`, mas resolvendo
//! `XDG_DATA_HOME` (fallback `~/.local/share`) em vez de `XDG_CONFIG_HOME` (fallback `~/.config`)
//! — código-fonte instalado de um plugin de terceiro é uma categoria de dado distinta de
//! configuração do usuário (rationale completo em `research.md` D2).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Manifesto de um plugin instalado, já validado (`parse_manifest`) — ver `data-model.md`
/// § `PluginManifest`.
///
/// `#[allow(dead_code)]` neste módulo (T002-T004, Foundational): estes tipos/funções só ganham
/// um chamador de produção em US1 (`plugin_worker::discover_installed_plugins`, T008) e US2
/// (`install::run`, T013), ambos fora do escopo desta subtarefa — só os testes de unidade deste
/// arquivo os exercitam por enquanto.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct PluginManifest {
    pub plugin_name: String,
    pub command: String,
    pub args: Vec<String>,
    /// De `[capabilities].network` — ausência da seção inteira equivale a `false` (D1).
    pub allow_network: bool,
    /// De `[capabilities].exec` — ausência da seção inteira equivale a `false` (D1).
    pub allow_exec: bool,
}

/// Erros de validação de `parse_manifest` (`data-model.md` § `ManifestError`, D7). Consumido
/// tanto pela descoberta (US1, erro vira aviso + plugin ignorado, FR-004) quanto pela instalação
/// (US2, erro vira falha do comando com mensagem clara, FR-010).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ManifestError {
    NotFound(PathBuf),
    InvalidToml(String),
    MissingField(&'static str),
    EmptyPluginName,
}

/// Forma bruta desserializada do TOML antes da validação — todos os campos opcionais na struct
/// de `serde` porque a ausência de um campo obrigatório (`plugin_name`/`command`/`args`) precisa
/// virar `ManifestError::MissingField`, não um erro de desserialização genérico do `toml`.
/// Tolerante a campo desconhecido (sem `deny_unknown_fields`, D1).
#[derive(Debug, Deserialize)]
struct RawManifest {
    plugin_name: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    #[serde(default)]
    capabilities: RawCapabilities,
}

/// `[capabilities]` ausente equivale a `{ network: false, exec: false }` (D1) — daí `#[serde(default)]`
/// tanto no campo `capabilities` acima quanto em cada booleano aqui dentro.
#[derive(Debug, Deserialize, Default)]
struct RawCapabilities {
    #[serde(default)]
    network: bool,
    #[serde(default)]
    exec: bool,
}

/// Lê e valida um `farol-plugin.toml` em `path`, seguindo exatamente a tabela de
/// `contracts/plugin-manifest-and-install-contract.md` § "Contrato de `parse_manifest`".
#[allow(dead_code)]
pub fn parse_manifest(path: &Path) -> Result<PluginManifest, ManifestError> {
    let content = fs::read_to_string(path).map_err(|_| ManifestError::NotFound(path.to_path_buf()))?;

    let raw: RawManifest =
        toml::from_str(&content).map_err(|err| ManifestError::InvalidToml(err.to_string()))?;

    let plugin_name = raw
        .plugin_name
        .ok_or(ManifestError::MissingField("plugin_name"))?;
    if plugin_name.trim().is_empty() {
        return Err(ManifestError::EmptyPluginName);
    }

    let command = raw.command.ok_or(ManifestError::MissingField("command"))?;
    let args = raw.args.ok_or(ManifestError::MissingField("args"))?;

    Ok(PluginManifest {
        plugin_name,
        command,
        args,
        allow_network: raw.capabilities.network,
        allow_exec: raw.capabilities.exec,
    })
}

/// Diretório base de dados do Farol: `$XDG_DATA_HOME/farol`, com fallback para
/// `~/.local/share/farol` quando `XDG_DATA_HOME` não está definida ou é vazia — mesma convenção
/// de `config_store::farol_config_base_dir()`, mas para `XDG_DATA_HOME`/`~/.local/share` em vez
/// de `XDG_CONFIG_HOME`/`~/.config` (D2 de `research.md`: código-fonte instalado é uma categoria
/// de dado distinta de configuração).
#[allow(dead_code)]
pub fn farol_data_base_dir() -> PathBuf {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home_dir().join(".local").join("share"),
    };
    base.join("farol")
}

/// `$HOME` do usuário atual; `"."` como último recurso caso nem `$HOME` esteja definida (ambiente
/// degenerado — nunca deveria ocorrer em uso real, só evita panic). Duplicado deliberadamente de
/// `config_store::home_dir` (privada ao módulo dela) em vez de exportada de lá — mesma lógica
/// mínima, sem acoplar os dois módulos por um detalhe interno.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Diretório de instalação de um plugin: `<farol_data_base_dir()>/plugins/<plugin_name>/`.
#[allow(dead_code)]
pub fn installed_plugin_dir(plugin_name: &str) -> PathBuf {
    farol_data_base_dir().join("plugins").join(plugin_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diretório temporário único por teste, sob `std::env::temp_dir()` — evita colisão entre
    /// testes rodando em paralelo (nenhum mock de filesystem; arquivos reais, mesma disciplina de
    /// `config_store::tests::round_trip_merges_with_existing_keys`).
    fn temp_manifest_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "farol-plugin-manifest-test-{}-{}-{}",
            test_name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_manifest(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("farol-plugin.toml");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn missing_file_is_not_found() {
        let dir = temp_manifest_dir("missing-file");
        let path = dir.join("does-not-exist.toml");
        assert_eq!(parse_manifest(&path), Err(ManifestError::NotFound(path)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_toml_is_invalid_toml_error() {
        let dir = temp_manifest_dir("invalid-toml");
        let path = write_manifest(&dir, "this is not [ valid toml");
        match parse_manifest(&path) {
            Err(ManifestError::InvalidToml(_)) => {}
            other => panic!("esperava InvalidToml, obteve {other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_plugin_name_is_missing_field() {
        let dir = temp_manifest_dir("missing-plugin-name");
        let path = write_manifest(&dir, "command = \"python3\"\nargs = [\"main.py\"]\n");
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::MissingField("plugin_name"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_plugin_name_is_empty_plugin_name() {
        let dir = temp_manifest_dir("empty-plugin-name");
        let path = write_manifest(
            &dir,
            "plugin_name = \"   \"\ncommand = \"python3\"\nargs = [\"main.py\"]\n",
        );
        assert_eq!(parse_manifest(&path), Err(ManifestError::EmptyPluginName));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_command_is_missing_field() {
        let dir = temp_manifest_dir("missing-command");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\nargs = [\"main.py\"]\n",
        );
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::MissingField("command"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_args_is_missing_field() {
        let dir = temp_manifest_dir("missing-args");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\n",
        );
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::MissingField("args"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_args_list_is_valid() {
        let dir = temp_manifest_dir("empty-args-list");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = []\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.args, Vec::<String>::new());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_capabilities_defaults_to_false() {
        let dir = temp_manifest_dir("missing-capabilities");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert!(!manifest.allow_network);
        assert!(!manifest.allow_exec);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_field_is_ignored() {
        let dir = temp_manifest_dir("unknown-field");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nunknown_field = \"whatever\"\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.plugin_name, "exemplo");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_valid_manifest_succeeds() {
        let dir = temp_manifest_dir("full-valid");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo-plugin\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n\n[capabilities]\nnetwork = false\nexec = true\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(
            manifest,
            PluginManifest {
                plugin_name: "exemplo-plugin".to_string(),
                command: "python3".to_string(),
                args: vec!["main.py".to_string()],
                allow_network: false,
                allow_exec: true,
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
