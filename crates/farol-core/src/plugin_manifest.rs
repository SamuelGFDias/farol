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
    /// Comando de build (feature 008, US2, campo `build` na raiz do manifesto) — `MAY` estar
    /// ausente (`None`), preservando o comportamento da feature 007 para manifesto sem passo de
    /// build. Quando presente, executado pelo instalador (`install::run`, fora do escopo desta
    /// subtarefa) no diretório de código-fonte extraído, antes do rename atômico.
    pub build: Option<String>,
    /// Caminhos absolutos de filesystem concedidos em modo leitura (feature 008, US3, campo
    /// `filesystem_read` na raiz do manifesto) — default vazio (retrocompatível com manifestos da
    /// feature 007). Cada entrada já passou por [`validate_filesystem_paths`] em
    /// [`parse_manifest`].
    pub filesystem_read: Vec<String>,
    /// Mesma semântica de [`PluginManifest::filesystem_read`], em modo leitura/escrita (campo
    /// `filesystem_read_write`).
    pub filesystem_read_write: Vec<String>,
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
    /// Caminho de `filesystem_read`/`filesystem_read_write` que não é absoluto (feature 008, US3,
    /// FR-009) — carrega o caminho exato declarado, para a mensagem de erro identificar qual
    /// entrada falhou.
    RelativeFilesystemPath(String),
    /// Caminho de `filesystem_read`/`filesystem_read_write` que cai na denylist de caminhos
    /// sensíveis (feature 008, US3, FR-008/FR-009) — carrega o caminho exato declarado.
    DenylistedFilesystemPath(String),
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
    /// Feature 008, US2 — ausente equivale a `None` (retrocompatível com feature 007).
    build: Option<String>,
    /// Feature 008, US3 — ausente equivale a lista vazia (retrocompatível com feature 007).
    #[serde(default)]
    filesystem_read: Vec<String>,
    /// Feature 008, US3 — ausente equivale a lista vazia (retrocompatível com feature 007).
    #[serde(default)]
    filesystem_read_write: Vec<String>,
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

    validate_filesystem_paths(&raw.filesystem_read)?;
    validate_filesystem_paths(&raw.filesystem_read_write)?;

    Ok(PluginManifest {
        plugin_name,
        command,
        args,
        allow_network: raw.capabilities.network,
        allow_exec: raw.capabilities.exec,
        build: raw.build,
        filesystem_read: raw.filesystem_read,
        filesystem_read_write: raw.filesystem_read_write,
    })
}

/// Denylist estática de caminhos sensíveis nunca concedidos a um plugin de terceiro via
/// capability de filesystem genérica (feature 008, US3) — resolve `$HOME`/`$XDG_CONFIG_HOME` no
/// momento da validação (edge case de `spec.md`: a denylist MUST cobrir variação de localização
/// por ambiente, nunca comparar contra caminho literal hardcoded que assuma localização default).
/// Cobre, no mínimo (`spec.md` § Assumptions): `~/.ssh`, `/etc`, e o diretório de segredos/
/// configuração do próprio Farol (`config_store`/`secrets_store`, feature 002) — `$XDG_CONFIG_HOME/
/// farol` (fallback `~/.config/farol`).
///
/// Lógica de resolução de `$XDG_CONFIG_HOME`/`$HOME` duplicada deliberadamente de
/// `config_store::farol_config_base_dir()` (privada ao módulo dela), seguindo a mesma disciplina
/// já praticada por [`home_dir`] neste arquivo — mesma lógica mínima, sem acoplar os dois módulos
/// por um detalhe interno.
fn filesystem_capability_denylist() -> Vec<PathBuf> {
    let home = home_dir();
    let config_base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home.join(".config"),
    };

    vec![home.join(".ssh"), PathBuf::from("/etc"), config_base.join("farol")]
}

/// Valida cada caminho de `filesystem_read`/`filesystem_read_write` do manifesto (feature 008,
/// US3): rejeita caminho relativo ([`ManifestError::RelativeFilesystemPath`]) e caminho contido na
/// denylist de caminhos sensíveis ([`ManifestError::DenylistedFilesystemPath`]) —
/// "contido" inclui o próprio caminho denylistado e qualquer caminho descendente dele
/// (`Path::starts_with`), para que um caminho como `~/.ssh/id_rsa` também seja rejeitado, não só
/// `~/.ssh` exato. Lista vazia é sempre válida (retrocompatível com manifesto sem capability de
/// filesystem, feature 007).
#[allow(dead_code)]
pub fn validate_filesystem_paths(paths: &[String]) -> Result<(), ManifestError> {
    let denylist = filesystem_capability_denylist();
    for raw_path in paths {
        let path = Path::new(raw_path);
        if !path.is_absolute() {
            return Err(ManifestError::RelativeFilesystemPath(raw_path.clone()));
        }
        if denylist.iter().any(|denied| path.starts_with(denied)) {
            return Err(ManifestError::DenylistedFilesystemPath(raw_path.clone()));
        }
    }
    Ok(())
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
                build: None,
                filesystem_read: Vec::new(),
                filesystem_read_write: Vec::new(),
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_without_build_or_filesystem_fields_defaults_to_empty() {
        let dir = temp_manifest_dir("no-build-no-filesystem");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.build, None);
        assert_eq!(manifest.filesystem_read, Vec::<String>::new());
        assert_eq!(manifest.filesystem_read_write, Vec::<String>::new());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_with_build_command_is_parsed() {
        let dir = temp_manifest_dir("with-build");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nbuild = \"cargo build --release\"\n",
        );
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.build, Some("cargo build --release".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_filesystem_read_path_is_rejected() {
        let dir = temp_manifest_dir("relative-fs-path");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read = [\"relativo/sem/barra\"]\n",
        );
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::RelativeFilesystemPath(
                "relativo/sem/barra".to_string()
            ))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn denylisted_etc_filesystem_read_write_path_is_rejected() {
        let dir = temp_manifest_dir("denylisted-etc");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read_write = [\"/etc\"]\n",
        );
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::DenylistedFilesystemPath("/etc".to_string()))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn denylisted_etc_descendant_path_is_also_rejected() {
        let dir = temp_manifest_dir("denylisted-etc-descendant");
        let path = write_manifest(
            &dir,
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read = [\"/etc/passwd\"]\n",
        );
        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::DenylistedFilesystemPath(
                "/etc/passwd".to_string()
            ))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Guarda de ambiente para os testes de denylist que dependem de `$HOME`/`$XDG_CONFIG_HOME`
    /// (edge case do `spec.md`: a denylist MUST resolver essas variáveis, não assumir localização
    /// default) — restaura o valor anterior no `Drop`, mesmo padrão de `install::tests::InstallTestEnv`.
    /// Serializado por [`fs_denylist_test_guard`] porque `$HOME`/`$XDG_CONFIG_HOME` são globais ao
    /// processo e `cargo test` roda testes deste módulo em threads concorrentes por padrão.
    static FS_DENYLIST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fs_denylist_test_guard() -> std::sync::MutexGuard<'static, ()> {
        FS_DENYLIST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct FsDenylistTestEnv {
        prev_home: Option<std::ffi::OsString>,
        prev_xdg_config_home: Option<std::ffi::OsString>,
    }

    impl FsDenylistTestEnv {
        fn set(home: &Path, xdg_config_home: &Path) -> Self {
            let prev_home = std::env::var_os("HOME");
            let prev_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_CONFIG_HOME", xdg_config_home);
            Self {
                prev_home,
                prev_xdg_config_home,
            }
        }
    }

    impl Drop for FsDenylistTestEnv {
        fn drop(&mut self) {
            match &self.prev_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_xdg_config_home {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn denylisted_home_ssh_path_is_rejected_resolving_custom_home() {
        let _guard = fs_denylist_test_guard();
        let dir = temp_manifest_dir("denylisted-home-ssh");
        let fake_home = dir.join("fake-home");
        let fake_xdg_config = dir.join("fake-xdg-config");
        fs::create_dir_all(&fake_home).unwrap();
        fs::create_dir_all(&fake_xdg_config).unwrap();
        let _env = FsDenylistTestEnv::set(&fake_home, &fake_xdg_config);

        let ssh_path = fake_home.join(".ssh").join("id_rsa");
        let content = format!(
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read = [\"{}\"]\n",
            ssh_path.display()
        );
        let path = write_manifest(&dir, &content);

        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::DenylistedFilesystemPath(
                ssh_path.display().to_string()
            ))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn denylisted_custom_xdg_config_home_farol_path_is_rejected() {
        let _guard = fs_denylist_test_guard();
        let dir = temp_manifest_dir("denylisted-xdg-config");
        let fake_home = dir.join("fake-home");
        let fake_xdg_config = dir.join("fake-xdg-config");
        fs::create_dir_all(&fake_home).unwrap();
        fs::create_dir_all(&fake_xdg_config).unwrap();
        let _env = FsDenylistTestEnv::set(&fake_home, &fake_xdg_config);

        let secrets_path = fake_xdg_config.join("farol").join("secrets.toml");
        let content = format!(
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read_write = [\"{}\"]\n",
            secrets_path.display()
        );
        let path = write_manifest(&dir, &content);

        assert_eq!(
            parse_manifest(&path),
            Err(ManifestError::DenylistedFilesystemPath(
                secrets_path.display().to_string()
            ))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn absolute_path_outside_denylist_is_accepted() {
        let dir = temp_manifest_dir("valid-absolute-path");
        let allowed_dir = dir.join("dados-do-plugin");
        fs::create_dir_all(&allowed_dir).unwrap();
        let content = format!(
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read = [\"{}\"]\n",
            allowed_dir.display()
        );
        let path = write_manifest(&dir, &content);

        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.filesystem_read, vec![allowed_dir.display().to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiple_valid_filesystem_paths_are_accepted() {
        let dir = temp_manifest_dir("multiple-valid-paths");
        let first = dir.join("primeiro");
        let second = dir.join("segundo");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let content = format!(
            "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\nfilesystem_read_write = [\"{}\", \"{}\"]\n",
            first.display(),
            second.display()
        );
        let path = write_manifest(&dir, &content);

        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(
            manifest.filesystem_read_write,
            vec![first.display().to_string(), second.display().to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
