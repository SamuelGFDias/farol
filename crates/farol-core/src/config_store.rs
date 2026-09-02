//! Leitura/escrita de `config.toml` — valores **não-secretos** de `required_config` de um
//! plugin (D8 revisado de `specs/002-uptime-kuma-plugin/research.md`).
//!
//! Generaliza, no core, o mecanismo que a feature 001 deixava inteiramente do lado do plugin
//! (`plugins/git-local/config.py`, que lia sozinho um único campo fixo `scan_root`): agora é o
//! **core** (Rust) quem lê/escreve `$XDG_CONFIG_HOME/farol/plugins/<nome>/config.toml` (fallback
//! `~/.config/farol/plugins/<nome>/config.toml` quando `XDG_CONFIG_HOME` não está definida — mesma
//! convenção de `plugins/git-local/config.py:27-33`, aqui reimplementada em Rust), aceitando
//! **qualquer** conjunto de chaves que um plugin declare em `required_config` — não mais um campo
//! fixo como `scan_root` era.
//!
//! Formato do arquivo: TOML simples, um mapa raiz `chave = "valor"` (sem seções) — cada chave é o
//! `name` de um item de `required_config` com `secret: false`. Itens com `secret: true` **nunca**
//! passam por este módulo: vão para `secrets_store` (T017).
//!
//! Consumido por T018 (`plugin_worker.rs`, fora do escopo desta subtarefa) para resolver cada item
//! de `required_config` recebido no handshake antes de injetar como variável de ambiente do
//! processo filho — ver [`resolve_required_config_value`].

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::secrets_store;

/// Diretório base de configuração do Farol: `$XDG_CONFIG_HOME/farol`, com fallback para
/// `~/.config/farol` quando `XDG_CONFIG_HOME` não está definida ou é vazia — mesma convenção de
/// `plugins/git-local/config.py:27-33` (feature 001), agora implementada no core. Reaproveitado
/// por `secrets_store` (T017), já que `secrets.toml` mora na mesma base, só que solto na raiz em
/// vez de sob `plugins/<nome>/`.
pub(crate) fn farol_config_base_dir() -> PathBuf {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home_dir().join(".config"),
    };
    base.join("farol")
}

/// `$HOME` do usuário atual; `"."` como último recurso caso nem `$HOME` esteja definida (ambiente
/// degenerado — nunca deveria ocorrer em uso real, só evita panic).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Caminho de `config.toml` de um plugin: `<base>/plugins/<plugin_name>/config.toml`.
pub fn plugin_config_path(plugin_name: &str) -> PathBuf {
    config_path_under(&farol_config_base_dir(), plugin_name)
}

fn config_path_under(base_dir: &Path, plugin_name: &str) -> PathBuf {
    base_dir
        .join("plugins")
        .join(plugin_name)
        .join("config.toml")
}

/// Lê o `config.toml` de um plugin. Arquivo ausente, ilegível ou malformado resulta em mapa
/// vazio — nunca um erro (mesma tolerância de `plugins/git-local/config.py:load_scan_root`; D8:
/// um arquivo de storage do core ausente/corrompido não impede o processo do plugin de subir, só
/// aquele item de `required_config` fica sem valor injetado).
pub fn load_plugin_config(plugin_name: &str) -> BTreeMap<String, String> {
    load_config_from(&plugin_config_path(plugin_name))
}

fn load_config_from(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

/// Devolve um único valor não-secreto de `config.toml`, se presente.
pub fn get_plugin_config_value(plugin_name: &str, key: &str) -> Option<String> {
    load_plugin_config(plugin_name).get(key).cloned()
}

/// Grava `values` em `config.toml` de um plugin, **mesclando** com o que já existir no arquivo
/// (chaves não presentes em `values` são preservadas) — usado pela tela de setup (D8) ao
/// persistir só os campos não-secretos submetidos no formulário. Cria os diretórios pais quando
/// necessário.
pub fn save_plugin_config(plugin_name: &str, values: &BTreeMap<String, String>) -> io::Result<()> {
    let path = plugin_config_path(plugin_name);
    let mut merged = load_config_from(&path);
    merged.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = toml::to_string_pretty(&merged)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, serialized)
}

/// Resolve um item de `required_config` recebido no handshake contra o armazenamento certo —
/// `config.toml` (`secret == false`, este módulo) ou `secrets.toml` (`secret == true`,
/// [`secrets_store`]) — conforme a flag declarada pelo próprio plugin (D8). `None` quando o item
/// não tem valor armazenado; T018 (fora do escopo desta subtarefa) decide o que fazer nesse caso
/// (não injeta a variável de ambiente correspondente, e a conexão acaba em
/// `PluginState::Unavailable(NotConfigured)`, T019).
pub fn resolve_required_config_value(
    plugin_name: &str,
    name: &str,
    secret: bool,
) -> Option<String> {
    if secret {
        secrets_store::get_plugin_secret_value(plugin_name, name)
    } else {
        get_plugin_config_value(plugin_name, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_from_missing_file_is_empty() {
        let path = PathBuf::from("/definitely/does/not/exist/farol-config-store-test/config.toml");
        assert!(load_config_from(&path).is_empty());
    }

    #[test]
    fn config_path_under_uses_plugins_subdir() {
        let base = PathBuf::from("/tmp/farol-base");
        let path = config_path_under(&base, "uptime-kuma");
        assert_eq!(
            path,
            PathBuf::from("/tmp/farol-base/plugins/uptime-kuma/config.toml")
        );
    }

    #[test]
    fn round_trip_merges_with_existing_keys() {
        let dir = std::env::temp_dir().join(format!(
            "farol-config-store-test-round-trip-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut first = BTreeMap::new();
        first.insert("base_url".to_string(), "https://a.example".to_string());
        fs::write(&path, toml::to_string_pretty(&first).unwrap()).unwrap();

        // Simula o que `save_plugin_config` faz internamente, mas contra um caminho de teste em
        // vez do caminho real (que depende de `$XDG_CONFIG_HOME`/`$HOME` do ambiente e não deve
        // ser mutado por um teste unitário).
        let mut second = BTreeMap::new();
        second.insert("other_field".to_string(), "value".to_string());
        let mut merged = load_config_from(&path);
        merged.extend(second.iter().map(|(k, v)| (k.clone(), v.clone())));
        fs::write(&path, toml::to_string_pretty(&merged).unwrap()).unwrap();

        let loaded = load_config_from(&path);
        assert_eq!(loaded.get("base_url").unwrap(), "https://a.example");
        assert_eq!(loaded.get("other_field").unwrap(), "value");

        let _ = fs::remove_dir_all(&dir);
    }
}
