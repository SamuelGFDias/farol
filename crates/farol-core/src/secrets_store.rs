//! Leitura/escrita de `secrets.toml` — valores **secretos** de `required_config` (D8 revisado de
//! `specs/002-uptime-kuma-plugin/research.md`).
//!
//! Um único arquivo para **todos** os plugins — `$XDG_CONFIG_HOME/farol/secrets.toml` (fallback
//! `~/.config/farol/secrets.toml`, mesma base de [`crate::config_store::farol_config_base_dir`]),
//! uma seção TOML por plugin (`[uptime-kuma]`, chave = `name` do item de `required_config` com
//! `secret: true`). Este arquivo é **exclusivo do core**: nenhum plugin lê ou escreve nele
//! diretamente — o plugin só recebe o valor já resolvido como variável de ambiente no spawn do
//! processo (T018, fora do escopo deste módulo; garantido porque só o core tem este caminho no
//! seu vocabulário de I/O).
//!
//! A permissão do arquivo é forçada a `0600` (Unix) logo após cada escrita (criação ou
//! atualização) — segredo nunca fica legível por outros usuários do sistema, mesmo que o `umask`
//! do processo seja mais permissivo.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config_store::farol_config_base_dir;

/// Mapa de segredos de todos os plugins: `plugin_name -> (key -> value)`.
type SecretsByPlugin = BTreeMap<String, BTreeMap<String, String>>;

/// Caminho de `secrets.toml`: `<base>/secrets.toml` (mesma base de `config_store`, mas arquivo
/// solto na raiz — não em `plugins/<nome>/`, ao contrário de `config.toml`, porque é um único
/// arquivo compartilhado por todos os plugins, D8).
pub fn secrets_path() -> PathBuf {
    farol_config_base_dir().join("secrets.toml")
}

/// Lê `secrets.toml` inteiro. Arquivo ausente, ilegível ou malformado resulta em mapa vazio —
/// nunca um erro (mesma tolerância de `config_store`, D8: um `secrets.toml` corrompido do lado do
/// core não deve impedir o processo do plugin de subir — ele só sobe sem aquela variável de
/// ambiente, e a conexão acaba em `PluginState::Unavailable(NotConfigured)`, T019).
fn load_all_secrets_from(path: &Path) -> SecretsByPlugin {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

/// Lê `secrets.toml` inteiro (todos os plugins).
pub fn load_all_secrets() -> SecretsByPlugin {
    load_all_secrets_from(&secrets_path())
}

/// Lê só a seção de um plugin.
pub fn load_plugin_secrets(plugin_name: &str) -> BTreeMap<String, String> {
    load_all_secrets().remove(plugin_name).unwrap_or_default()
}

/// Devolve um único valor secreto, se presente.
pub fn get_plugin_secret_value(plugin_name: &str, key: &str) -> Option<String> {
    load_plugin_secrets(plugin_name).get(key).cloned()
}

/// Grava `values` na seção de um plugin em `secrets.toml`, **mesclando** com o que já existir
/// (outras seções/chaves são preservadas) — usado pela tela de setup (D8) ao persistir os campos
/// marcados `secret: true` do formulário. Cria os diretórios pais quando necessário e, logo após
/// escrever, força a permissão do arquivo a `0600` (Unix) — MUST em toda escrita (criação ou
/// atualização, D8), nunca deixando o arquivo com a permissão herdada do `umask` do processo.
pub fn save_plugin_secrets(plugin_name: &str, values: &BTreeMap<String, String>) -> io::Result<()> {
    let path = secrets_path();
    let mut all = load_all_secrets_from(&path);
    all.entry(plugin_name.to_string())
        .or_default()
        .extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = toml::to_string_pretty(&all)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(&path, serialized)?;
    force_owner_only_permissions(&path)
}

/// Restringe a permissão do arquivo a `0600` (leitura/escrita só pelo dono) — MUST após toda
/// escrita de `secrets.toml` (D8). Só se aplica em Unix (`PermissionsExt`); farol-core hoje só
/// roda em Linux (nada em `plan.md`/`constitution.md` cita suporte Windows), então o branch
/// `not(unix)` é só uma salvaguarda de compilação cruzada, não um caminho esperado em uso real.
#[cfg(unix)]
fn force_owner_only_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn force_owner_only_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_all_secrets_from_missing_file_is_empty() {
        let path =
            PathBuf::from("/definitely/does/not/exist/farol-secrets-store-test/secrets.toml");
        assert!(load_all_secrets_from(&path).is_empty());
    }

    #[test]
    fn save_and_load_round_trip_forces_0600() {
        let dir =
            std::env::temp_dir().join(format!("farol-secrets-store-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        let mut values = BTreeMap::new();
        values.insert("api_key".to_string(), "s3cr3t".to_string());

        // Exercita a mesma lógica de `save_plugin_secrets`, mas contra um caminho de teste em vez
        // do caminho real de `secrets_path()` (que depende de `$XDG_CONFIG_HOME`/`$HOME` do
        // ambiente e não deve ser mutado por um teste unitário).
        let mut all = load_all_secrets_from(&path);
        all.entry("uptime-kuma".to_string())
            .or_default()
            .extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
        fs::write(&path, toml::to_string_pretty(&all).unwrap()).unwrap();
        force_owner_only_permissions(&path).unwrap();

        let loaded = load_all_secrets_from(&path);
        assert_eq!(
            loaded.get("uptime-kuma").unwrap().get("api_key").unwrap(),
            "s3cr3t"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
