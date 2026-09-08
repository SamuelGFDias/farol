//! Fluxo de instalação de plugin de terceiro via GitHub (`farol install <owner>/<repo>`) — feature
//! 007, US2 (T013).
//!
//! Sem dependência Rust nova para HTTP (D5 de `research.md`): o download em si é feito chamando os
//! binários `curl`/`tar` já assumidos disponíveis (mesma disciplina de minimalismo das features
//! anteriores). Só a interpretação da resposta JSON de `/releases/latest` usa `serde_json`, já
//! dependência do workspace.
//!
//! Contrato passo a passo normativo em
//! `specs/007-registry-instalacao-plugins-github/contracts/plugin-manifest-and-install-contract.md`
//! § "Contrato do fluxo de instalação" — este módulo segue essa tabela exatamente.

use std::path::PathBuf;
use std::process::Command;

use crate::plugin_manifest::{self, ManifestError};
use crate::registry_index::{self, RegistryIndexError};

/// Resultado do fluxo de instalação (`data-model.md` § `InstallOutcome`) — traduzido para código
/// de saída do processo por [`crate::main`] (T014), não persistido em lugar nenhum.
#[derive(Debug)]
pub enum InstallOutcome {
    Installed { plugin_name: String, path: PathBuf },
    NoRelease,
    DownloadFailed(String),
    ManifestInvalid(ManifestError),
    NameCollision(String),
}

/// Base da API do GitHub — `FAROL_GITHUB_API_BASE` só existe como escape-hatch de teste (D8);
/// ausente/vazia usa a API real.
fn github_api_base() -> String {
    std::env::var("FAROL_GITHUB_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_string())
}

/// Executa o fluxo completo de instalação de `owner/repo`, seguindo o contrato passo a passo
/// (ver docstring do módulo). Síncrono — chamado por [`crate::main`] antes de montar a aplicação
/// `iced`, nunca a partir da UI.
pub fn run(owner: &str, repo: &str) -> InstallOutcome {
    let base = github_api_base();
    let release_url = format!("{base}/repos/{owner}/{repo}/releases/latest");

    let tarball_url = match fetch_latest_release(&release_url) {
        Ok(FetchOutcome::NoRelease) => return InstallOutcome::NoRelease,
        Ok(FetchOutcome::Found { tarball_url }) => tarball_url,
        Err(message) => return InstallOutcome::DownloadFailed(message),
    };

    let unique = unique_suffix();
    let tarball_path = std::env::temp_dir().join(format!("farol-install-{unique}.tar.gz"));
    let staging_dir = std::env::temp_dir().join(format!("farol-install-staging-{unique}"));

    let download_result = download_tarball(&tarball_url, &tarball_path);
    if let Err(message) = download_result {
        let _ = std::fs::remove_file(&tarball_path);
        return InstallOutcome::DownloadFailed(message);
    }

    let extract_result = extract_tarball(&tarball_path, &staging_dir);
    // O arquivo temporário do tarball é limpo sempre, sucesso ou falha, a partir daqui.
    let _ = std::fs::remove_file(&tarball_path);
    if let Err(message) = extract_result {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return InstallOutcome::DownloadFailed(message);
    }

    let manifest = match plugin_manifest::parse_manifest(&staging_dir.join("farol-plugin.toml")) {
        Err(err) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return InstallOutcome::ManifestInvalid(err);
        }
        Ok(manifest) => manifest,
    };

    if crate::plugin_worker::known_plugins()
        .iter()
        .any(|config| config.plugin_name == manifest.plugin_name)
    {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return InstallOutcome::NameCollision(manifest.plugin_name);
    }

    if let Some(build_command) = &manifest.build {
        if let Err(message) = run_build_command(build_command, &staging_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return InstallOutcome::DownloadFailed(message);
        }
    }

    let destination = plugin_manifest::installed_plugin_dir(&manifest.plugin_name);
    if destination.exists() {
        // FR-008: substituição limpa de uma instalação anterior do mesmo plugin.
        let _ = std::fs::remove_dir_all(&destination);
    }
    if let Some(parent) = destination.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return InstallOutcome::DownloadFailed(format!(
                "falha ao criar o diretório de plugins ({parent:?}): {err}"
            ));
        }
    }
    if let Err(err) = std::fs::rename(&staging_dir, &destination) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return InstallOutcome::DownloadFailed(format!(
            "falha ao publicar o plugin instalado em {destination:?}: {err}"
        ));
    }

    InstallOutcome::Installed {
        plugin_name: manifest.plugin_name,
        path: destination,
    }
}

/// Resultado de [`run_by_name`] (feature 008, US1, T006) — **não** estende [`InstallOutcome`]
/// deliberadamente: um erro de resolução de nome (`registry_index`) acontece antes mesmo de
/// existir um `owner/repo` para seguir o fluxo de `run` já existente (feature 007), e é uma
/// categoria de falha distinta o bastante para não caber nas variantes já existentes de
/// `InstallOutcome` sem forçar `main.rs::handle_install_subcommand` (que hoje faz `match`
/// exaustivo sobre `InstallOutcome`, T014 da feature 007) a ganhar um braço novo fora do escopo
/// desta subtarefa (Foundational, T001-T007 — a integração de `farol install <nome>` na CLI/UI é
/// US4, fora daqui). `Resolved` carrega o `InstallOutcome` de sempre para o caso feliz (nome
/// resolvido com sucesso), preservando FR-003 ("nenhuma duplicação de lógica de instalação entre o
/// caminho por nome e o caminho por `owner/repo` direto" — `run_by_name` só resolve o nome e
/// delega para [`run`]).
#[derive(Debug)]
#[allow(dead_code)]
pub enum InstallByNameOutcome {
    /// Nome resolvido com sucesso contra o índice — resultado idêntico ao de `run(owner, repo)`
    /// (feature 007), sem nenhuma lógica de instalação duplicada (FR-003).
    Resolved(InstallOutcome),
    /// `name` não existe no índice central — distinto de `RegistryIndexUnavailable` (edge case de
    /// `spec.md`: indisponibilidade do índice MUST NUNCA ser interpretada como "plugin não
    /// existe").
    NameNotFoundInIndex(String),
    /// Índice central inacessível ou malformado (rede, rate limit, `index.toml` inválido) — MUST
    /// NOT ser confundido com `NameNotFoundInIndex`.
    RegistryIndexUnavailable(String),
}

/// Resolve `name` contra o repositório-índice central (`registry_index::resolve_name`) e, em caso
/// de sucesso, repassa o `owner`/`repo` encontrado para [`run`] — o mesmo fluxo de instalação já
/// definido pela feature 007 (FR-003 de `specs/008-registry-hardening/spec.md`). Erro de resolução
/// de nome vira uma variante de [`InstallByNameOutcome`] distinta de `Resolved`, nunca uma
/// tentativa de seguir adiante com dados incompletos.
#[allow(dead_code)]
pub fn run_by_name(name: &str) -> InstallByNameOutcome {
    match registry_index::resolve_name(name, &registry_index::default_index_url_base()) {
        Ok(entry) => InstallByNameOutcome::Resolved(run(&entry.owner, &entry.repo)),
        Err(RegistryIndexError::NotFound(name)) => InstallByNameOutcome::NameNotFoundInIndex(name),
        Err(RegistryIndexError::FetchFailed(detail)) | Err(RegistryIndexError::MalformedIndex(detail)) => {
            InstallByNameOutcome::RegistryIndexUnavailable(detail)
        }
    }
}

enum FetchOutcome {
    Found { tarball_url: String },
    NoRelease,
}

/// Passo 1 do contrato: `GET {base}/repos/{owner}/{repo}/releases/latest`, via `curl -sL -w
/// "\n%{http_code}"` — o corpo vem antes da última linha, que é o status HTTP.
fn fetch_latest_release(url: &str) -> Result<FetchOutcome, String> {
    let output = Command::new("curl")
        .args(["-sL", "-w", "\n%{http_code}", url])
        .output()
        .map_err(|err| format!("falha ao executar curl para {url}: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "curl falhou ao consultar {url} (código de saída {:?})",
            output.status.code()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body, status_code) = match stdout.trim_end_matches('\n').rsplit_once('\n') {
        Some((body, status)) => (body, status),
        None => ("", stdout.trim()),
    };

    match status_code.trim() {
        "404" => Ok(FetchOutcome::NoRelease),
        "200" => {
            let json: serde_json::Value = serde_json::from_str(body).map_err(|err| {
                format!("resposta de {url} não é JSON válido: {err}")
            })?;
            let tarball_url = json
                .get("tarball_url")
                .and_then(|value| value.as_str())
                .ok_or_else(|| format!("resposta de {url} não contém `tarball_url`"))?
                .to_string();
            if json.get("tag_name").and_then(|value| value.as_str()).is_none() {
                return Err(format!("resposta de {url} não contém `tag_name`"));
            }
            Ok(FetchOutcome::Found { tarball_url })
        }
        other => Err(format!("{url} devolveu status HTTP inesperado {other}")),
    }
}

/// Passo 2 do contrato: baixa `tarball_url` para `destination` via `curl -sL -f -o`. `-f` faz o
/// `curl` falhar (código de saída != 0) em qualquer status HTTP não-2xx, em vez de gravar a
/// página de erro como se fosse o tarball.
fn download_tarball(tarball_url: &str, destination: &std::path::Path) -> Result<(), String> {
    let output = Command::new("curl")
        .args(["-sL", "-f", "-o"])
        .arg(destination)
        .arg(tarball_url)
        .output()
        .map_err(|err| format!("falha ao executar curl para baixar {tarball_url}: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "download de {tarball_url} falhou (curl código de saída {:?})",
            output.status.code()
        ));
    }
    Ok(())
}

/// Passo 3 do contrato: `tar -xzf <tarball> -C <staging> --strip-components=1`, para um diretório
/// de staging criado antes (fora de `installed_plugin_dir`, FR-007).
fn extract_tarball(tarball_path: &std::path::Path, staging_dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(staging_dir)
        .map_err(|err| format!("falha ao criar diretório de staging {staging_dir:?}: {err}"))?;

    let output = Command::new("tar")
        .arg("-xzf")
        .arg(tarball_path)
        .arg("-C")
        .arg(staging_dir)
        .arg("--strip-components=1")
        .output()
        .map_err(|err| format!("falha ao executar tar sobre {tarball_path:?}: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "extração de {tarball_path:?} falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Passo opcional do contrato (feature 008, US2, FR-005/FR-006): quando o manifesto declara
/// `build`, executa esse comando via `sh -c` dentro de `staging_dir` — mesmo diretório onde o
/// manifesto já foi validado, antes do `rename` atômico para o diretório final. Código de saída
/// diferente de `0` (ou falha ao sequer executar o `sh`, ex. ausência do próprio interpretador)
/// aborta a instalação com uma mensagem específica identificando que a falha veio do build, nunca
/// da extração ou validação de manifesto (FR-006) — a limpeza do `staging_dir` fica a cargo do
/// chamador, mesmo mecanismo já usado para `ManifestInvalid`/`NameCollision`.
fn run_build_command(build_command: &str, staging_dir: &std::path::Path) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(build_command)
        .current_dir(staging_dir)
        .status()
        .map_err(|err| format!("comando de build falhou: falha ao executar sh: {err}"))?;

    if !status.success() {
        return Err(format!(
            "comando de build falhou: código de saída {:?}",
            status.code()
        ));
    }
    Ok(())
}

/// Sufixo único para nomear os caminhos temporários desta chamada (`std::process::id()` +
/// timestamp em nanossegundos) — mesma disciplina já usada pelos diretórios temporários de teste
/// deste crate (`plugin_manifest::tests::temp_manifest_dir`, `plugin_worker::tests::temp_xdg_data_home`).
fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread::JoinHandle;
    use std::time::Duration;

    /// Serializa os testes deste módulo entre si — ambos mexem em variáveis de ambiente globais
    /// ao processo (`FAROL_GITHUB_API_BASE`, `XDG_DATA_HOME`), mesma disciplina de
    /// `e2e_tests::E2E_LOCK`/`plugin_worker::tests::XDG_DATA_HOME_LOCK` (locks distintos por
    /// módulo de teste, cada um protegendo as variáveis que aquele módulo mexe).
    static INSTALL_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn install_test_guard() -> MutexGuard<'static, ()> {
        INSTALL_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "farol-install-test-{}-{}-{}",
            test_name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Constrói um tarball `.tar.gz` real (via `tar`/`gzip` do sistema, nunca bytes escritos à
    /// mão) contendo um único diretório-raiz `repo-fixture/` — reproduz a forma que o GitHub gera
    /// para tarballs de código-fonte (`<repo>-<sha>/`), que `--strip-components=1` remove.
    /// `manifest_content: None` gera um tarball sem `farol-plugin.toml` (cenário "manifesto
    /// ausente").
    fn build_fixture_tarball(work_dir: &Path, manifest_content: Option<&str>) -> Vec<u8> {
        let source_root = work_dir.join("source");
        let inner_dir = source_root.join("repo-fixture-abc123");
        std::fs::create_dir_all(&inner_dir).unwrap();
        if let Some(content) = manifest_content {
            std::fs::write(inner_dir.join("farol-plugin.toml"), content).unwrap();
        } else {
            std::fs::write(inner_dir.join("README.md"), "sem manifesto\n").unwrap();
        }

        let archive_path = work_dir.join("archive.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&source_root)
            .arg("repo-fixture-abc123")
            .status()
            .expect("tar deve estar disponível para construir a fixture");
        assert!(status.success(), "tar da fixture falhou");

        std::fs::read(&archive_path).unwrap()
    }

    /// Uma rota servida pelo servidor de fixture: corpo de resposta fixo para um caminho exato.
    /// O cenário de "download falha" é coberto com um status `5xx` na rota do tarball (ver
    /// `install_with_failing_tarball_download_returns_download_failed`), não por derrubar a
    /// conexão — mais simples e igualmente coberto pelo contrato (D5/D8 de `research.md`
    /// mencionam ambos como equivalentes).
    enum FixtureRoute {
        Response {
            status: u16,
            content_type: &'static str,
            body: Vec<u8>,
        },
    }

    /// Servidor HTTP local de fixture para a API do GitHub — mesmo padrão de
    /// `e2e_tests::MetricsFixtureServer` (`TcpListener` em porta efêmera, thread própria, laço de
    /// `accept` não-bloqueante até `shutdown`), servindo rotas fixas por caminho exato em vez de
    /// um único corpo (T015: precisa de duas rotas distintas, `/repos/.../releases/latest` e o
    /// tarball).
    struct GithubFixtureServer {
        base_url: String,
        shutdown: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for GithubFixtureServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn serve_connection(mut stream: TcpStream, routes: &[(String, FixtureRoute)]) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    request.extend_from_slice(&buffer[..n]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => return,
            }
        }

        let request = String::from_utf8_lossy(&request);
        let request_line = request.lines().next().unwrap_or_default();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();

        match routes.iter().find(|(route_path, _)| *route_path == path) {
            Some((_, FixtureRoute::Response { status, content_type, body })) => {
                let reason = if *status == 200 { "OK" } else { "Error" };
                let header = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
            None => {
                let header = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.flush();
            }
        }
    }

    fn release_json_route(server_base: &str, tag_name: &str) -> FixtureRoute {
        let body = serde_json::json!({
            "tag_name": tag_name,
            "tarball_url": format!("{server_base}/tarball"),
        })
        .to_string();
        FixtureRoute::Response {
            status: 200,
            content_type: "application/json",
            body: body.into_bytes(),
        }
    }

    /// Guarda de ambiente comum aos testes de integração: aponta `FAROL_GITHUB_API_BASE` para a
    /// fixture e `XDG_DATA_HOME` para um diretório temporário isolado — mesmo padrão de
    /// isolamento já usado por `plugin_worker::tests`/`e2e_tests`.
    struct InstallTestEnv {
        xdg_data_home: PathBuf,
    }

    impl InstallTestEnv {
        fn set(github_base: &str, test_name: &str) -> Self {
            let xdg_data_home = temp_test_dir(&format!("{test_name}-xdg"));
            std::env::set_var("FAROL_GITHUB_API_BASE", github_base);
            std::env::set_var("XDG_DATA_HOME", &xdg_data_home);
            Self { xdg_data_home }
        }
    }

    impl Drop for InstallTestEnv {
        fn drop(&mut self) {
            std::env::remove_var("FAROL_GITHUB_API_BASE");
            std::env::remove_var("XDG_DATA_HOME");
            let _ = std::fs::remove_dir_all(&self.xdg_data_home);
        }
    }

    /// Guarda de ambiente para `FAROL_REGISTRY_INDEX_URL` (T007, `registry_index::resolve_name` é
    /// chamado por `run_by_name` através de `registry_index::default_index_url_base()`, que lê
    /// essa variável) — restaura o valor anterior no `Drop`, mesmo padrão de `InstallTestEnv`
    /// acima. Serializado pelo mesmo `install_test_guard()` (ambos mutam variáveis de ambiente
    /// globais ao processo).
    struct RegistryIndexTestEnv {
        prev: Option<String>,
    }

    impl RegistryIndexTestEnv {
        fn set(index_url_base: &str) -> Self {
            let prev = std::env::var("FAROL_REGISTRY_INDEX_URL").ok();
            std::env::set_var("FAROL_REGISTRY_INDEX_URL", index_url_base);
            Self { prev }
        }
    }

    impl Drop for RegistryIndexTestEnv {
        fn drop(&mut self) {
            match &self.prev {
                Some(value) => std::env::set_var("FAROL_REGISTRY_INDEX_URL", value),
                None => std::env::remove_var("FAROL_REGISTRY_INDEX_URL"),
            }
        }
    }

    #[test]
    fn install_succeeds_and_publishes_the_plugin() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("success-tarball");
        let manifest = "plugin_name = \"exemplo-instalado\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n";
        let tarball_bytes = build_fixture_tarball(&work_dir, Some(manifest));

        // `base_url` da fixture só é conhecida depois do bind, mas as rotas precisam ser
        // fechadas antes do `start` — resolve-se com um bind em duas fases: primeiro descobre a
        // porta com um listener descartável, depois monta as rotas já com o `base_url` correto.
        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "success");

        let outcome = run("owner", "repo");
        match outcome {
            InstallOutcome::Installed { plugin_name, path } => {
                assert_eq!(plugin_name, "exemplo-instalado");
                assert!(path.join("farol-plugin.toml").exists());
                assert_eq!(
                    path,
                    plugin_manifest::installed_plugin_dir("exemplo-instalado")
                );
            }
            other => panic!("esperava Installed, obteve {other:?}"),
        }

        drop(server);
    }

    #[test]
    fn install_without_release_returns_no_release() {
        let _guard = install_test_guard();

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);

        let server = start_fixture_on_port(
            port,
            vec![(
                "/repos/owner/repo/releases/latest".to_string(),
                FixtureRoute::Response {
                    status: 404,
                    content_type: "application/json",
                    body: b"{\"message\":\"Not Found\"}".to_vec(),
                },
            )],
        );

        let _env = InstallTestEnv::set(&server.base_url, "no-release");

        let outcome = run("owner", "repo");
        assert!(matches!(outcome, InstallOutcome::NoRelease), "obteve {outcome:?}");

        drop(server);
    }

    #[test]
    fn install_with_failing_tarball_download_returns_download_failed() {
        let _guard = install_test_guard();

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 500,
                        content_type: "text/plain",
                        body: b"internal error".to_vec(),
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "download-failed");

        let outcome = run("owner", "repo");
        assert!(
            matches!(outcome, InstallOutcome::DownloadFailed(_)),
            "obteve {outcome:?}"
        );

        drop(server);
    }

    #[test]
    fn install_with_missing_manifest_returns_manifest_invalid() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("missing-manifest-tarball");
        let tarball_bytes = build_fixture_tarball(&work_dir, None);

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "missing-manifest");

        let outcome = run("owner", "repo");
        assert!(
            matches!(
                outcome,
                InstallOutcome::ManifestInvalid(ManifestError::NotFound(_))
            ),
            "obteve {outcome:?}"
        );

        drop(server);
    }

    #[test]
    fn install_with_malformed_manifest_returns_manifest_invalid() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("malformed-manifest-tarball");
        let tarball_bytes = build_fixture_tarball(&work_dir, Some("isto não é [ toml válido"));

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "malformed-manifest");

        let outcome = run("owner", "repo");
        assert!(
            matches!(
                outcome,
                InstallOutcome::ManifestInvalid(ManifestError::InvalidToml(_))
            ),
            "obteve {outcome:?}"
        );

        drop(server);
    }

    #[test]
    fn install_with_name_colliding_with_a_reference_plugin_returns_name_collision() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("collision-tarball");
        let manifest =
            "plugin_name = \"git-local\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n";
        let tarball_bytes = build_fixture_tarball(&work_dir, Some(manifest));

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "collision");

        let outcome = run("owner", "repo");
        match outcome {
            InstallOutcome::NameCollision(name) => assert_eq!(name, "git-local"),
            other => panic!("esperava NameCollision, obteve {other:?}"),
        }

        drop(server);
    }

    /// T009 (feature 008, US2): manifesto com `build = "true"` (comando de fixture, nunca
    /// toolchain real) executa com sucesso e a instalação publica o plugin normalmente — mesmo
    /// contrato de `install_succeeds_and_publishes_the_plugin`, só com o campo `build` novo.
    #[test]
    fn install_with_successful_build_publishes_the_plugin() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("successful-build-tarball");
        let manifest = "plugin_name = \"exemplo-com-build\"\ncommand = \"python3\"\n\
                         args = [\"main.py\"]\nbuild = \"true\"\n";
        let tarball_bytes = build_fixture_tarball(&work_dir, Some(manifest));

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "successful-build");

        let outcome = run("owner", "repo");
        match outcome {
            InstallOutcome::Installed { plugin_name, path } => {
                assert_eq!(plugin_name, "exemplo-com-build");
                assert!(path.join("farol-plugin.toml").exists());
            }
            other => panic!("esperava Installed, obteve {other:?}"),
        }

        drop(server);
    }

    /// T009 (feature 008, US2): manifesto com `build = "false"` (código de saída != 0) aborta a
    /// instalação inteira — nada é publicado em `installed_plugin_dir`, e o `staging_dir` não fica
    /// visível para a descoberta de plugins (FR-006/FR-007).
    #[test]
    fn install_with_failing_build_does_not_publish_and_cleans_up() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("failing-build-tarball");
        let manifest = "plugin_name = \"exemplo-build-falho\"\ncommand = \"python3\"\n\
                         args = [\"main.py\"]\nbuild = \"exit 1\"\n";
        let tarball_bytes = build_fixture_tarball(&work_dir, Some(manifest));

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let _env = InstallTestEnv::set(&server.base_url, "failing-build");

        let outcome = run("owner", "repo");
        match outcome {
            InstallOutcome::DownloadFailed(detail) => {
                assert!(
                    detail.contains("comando de build falhou"),
                    "mensagem não identifica falha de build: {detail}"
                );
            }
            other => panic!("esperava DownloadFailed (build falho), obteve {other:?}"),
        }
        assert!(
            !plugin_manifest::installed_plugin_dir("exemplo-build-falho").exists(),
            "plugin não deveria ter sido publicado após falha de build"
        );

        drop(server);
    }

    #[test]
    fn reinstalling_over_a_previous_install_replaces_it_cleanly() {
        let _guard = install_test_guard();

        // Primeira instalação.
        let work_dir_v1 = temp_test_dir("reinstall-v1-tarball");
        let manifest_v1 = "plugin_name = \"exemplo-reinstalado\"\ncommand = \"python3\"\n\
                            args = [\"v1.py\"]\n";
        let tarball_v1 = build_fixture_tarball(&work_dir_v1, Some(manifest_v1));

        let port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let server_base = format!("http://127.0.0.1:{port}");

        let server_v1 = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_v1,
                    },
                ),
            ],
        );

        let env = InstallTestEnv::set(&server_v1.base_url, "reinstall");

        let first = run("owner", "repo");
        assert!(matches!(first, InstallOutcome::Installed { .. }), "obteve {first:?}");
        drop(server_v1);

        // Segunda instalação, mesmo `plugin_name`, conteúdo diferente (`args` distinto) — a
        // mesma porta é reaproveitada; `FAROL_GITHUB_API_BASE` (com a porta embutida no host)
        // continua válido.
        let work_dir_v2 = temp_test_dir("reinstall-v2-tarball");
        let manifest_v2 = "plugin_name = \"exemplo-reinstalado\"\ncommand = \"python3\"\n\
                            args = [\"v2.py\"]\n";
        let tarball_v2 = build_fixture_tarball(&work_dir_v2, Some(manifest_v2));

        let server_v2 = start_fixture_on_port(
            port,
            vec![
                (
                    "/repos/owner/repo/releases/latest".to_string(),
                    release_json_route(&server_base, "v2.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_v2,
                    },
                ),
            ],
        );

        let second = run("owner", "repo");
        let installed_path = match second {
            InstallOutcome::Installed { plugin_name, path } => {
                assert_eq!(plugin_name, "exemplo-reinstalado");
                path
            }
            other => panic!("esperava Installed na reinstalação, obteve {other:?}"),
        };

        let installed_manifest =
            plugin_manifest::parse_manifest(&installed_path.join("farol-plugin.toml")).unwrap();
        assert_eq!(installed_manifest.args, vec!["v2.py".to_string()]);

        drop(server_v2);
        drop(env);
    }

    /// T007 (feature 008, US1): `run_by_name` resolvendo com sucesso contra um índice de fixture
    /// e seguindo para o mesmo fluxo de `run` já testado acima — dois servidores HTTP locais
    /// distintos (índice + "GitHub"), reaproveitando `GithubFixtureServer`/`start_fixture_on_port`
    /// para ambos (infraestrutura genérica por rota exata, nome herdado do uso original).
    #[test]
    fn install_by_name_resolves_successfully_and_installs() {
        let _guard = install_test_guard();
        let work_dir = temp_test_dir("by-name-success-tarball");
        let manifest =
            "plugin_name = \"exemplo-por-nome\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n";
        let tarball_bytes = build_fixture_tarball(&work_dir, Some(manifest));

        let github_port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let github_port = github_port_probe.local_addr().unwrap().port();
        drop(github_port_probe);
        let github_base = format!("http://127.0.0.1:{github_port}");

        let github_server = start_fixture_on_port(
            github_port,
            vec![
                (
                    "/repos/owner-do-indice/repo-do-indice/releases/latest".to_string(),
                    release_json_route(&github_base, "v1.0.0"),
                ),
                (
                    "/tarball".to_string(),
                    FixtureRoute::Response {
                        status: 200,
                        content_type: "application/gzip",
                        body: tarball_bytes,
                    },
                ),
            ],
        );

        let index_port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let index_port = index_port_probe.local_addr().unwrap().port();
        drop(index_port_probe);
        let index_base = format!("http://127.0.0.1:{index_port}");
        let index_toml = "[[plugin]]\nname = \"exemplo-por-nome\"\nowner = \"owner-do-indice\"\n\
                           repo = \"repo-do-indice\"\n";
        let index_server = start_fixture_on_port(
            index_port,
            vec![(
                "/index.toml".to_string(),
                FixtureRoute::Response {
                    status: 200,
                    content_type: "text/plain",
                    body: index_toml.as_bytes().to_vec(),
                },
            )],
        );

        let _github_env = InstallTestEnv::set(&github_base, "by-name-success");
        let _index_env = RegistryIndexTestEnv::set(&index_base);

        let outcome = run_by_name("exemplo-por-nome");
        match outcome {
            InstallByNameOutcome::Resolved(InstallOutcome::Installed { plugin_name, .. }) => {
                assert_eq!(plugin_name, "exemplo-por-nome");
            }
            other => panic!("esperava Resolved(Installed), obteve {other:?}"),
        }

        drop(github_server);
        drop(index_server);
    }

    #[test]
    fn install_by_name_with_unknown_name_returns_name_not_found_in_index() {
        let _guard = install_test_guard();

        let index_port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let index_port = index_port_probe.local_addr().unwrap().port();
        drop(index_port_probe);
        let index_base = format!("http://127.0.0.1:{index_port}");
        let index_toml = "[[plugin]]\nname = \"outro-plugin\"\nowner = \"dono\"\nrepo = \"repo\"\n";
        let index_server = start_fixture_on_port(
            index_port,
            vec![(
                "/index.toml".to_string(),
                FixtureRoute::Response {
                    status: 200,
                    content_type: "text/plain",
                    body: index_toml.as_bytes().to_vec(),
                },
            )],
        );

        let _index_env = RegistryIndexTestEnv::set(&index_base);

        let outcome = run_by_name("nome-inexistente");
        match outcome {
            InstallByNameOutcome::NameNotFoundInIndex(name) => {
                assert_eq!(name, "nome-inexistente")
            }
            other => panic!("esperava NameNotFoundInIndex, obteve {other:?}"),
        }

        drop(index_server);
    }

    #[test]
    fn install_by_name_with_malformed_index_returns_registry_index_unavailable() {
        let _guard = install_test_guard();

        let index_port_probe = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let index_port = index_port_probe.local_addr().unwrap().port();
        drop(index_port_probe);
        let index_base = format!("http://127.0.0.1:{index_port}");
        let index_server = start_fixture_on_port(
            index_port,
            vec![(
                "/index.toml".to_string(),
                FixtureRoute::Response {
                    status: 200,
                    content_type: "text/plain",
                    body: b"isto nao e [ toml valido".to_vec(),
                },
            )],
        );

        let _index_env = RegistryIndexTestEnv::set(&index_base);

        let outcome = run_by_name("qualquer-nome");
        assert!(
            matches!(outcome, InstallByNameOutcome::RegistryIndexUnavailable(_)),
            "obteve {outcome:?}"
        );

        drop(index_server);
    }

    /// Variante de [`GithubFixtureServer::start`] que faz o `bind` numa porta já conhecida (em
    /// vez de `:0`) — usada quando o `tarball_url` embutido na resposta JSON precisa apontar de
    /// volta para o mesmo servidor antes dele existir (ou, na reinstalação, para reaproveitar a
    /// mesma porta entre duas instâncias sequenciais do servidor).
    fn start_fixture_on_port(port: u16, routes: Vec<(String, FixtureRoute)>) -> GithubFixtureServer {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .expect("bind da fixture de GitHub na porta reservada");
        listener
            .set_nonblocking(true)
            .expect("listener não-bloqueante (para observar o shutdown)");

        let shutdown = Arc::new(AtomicBool::new(false));
        let routes = Arc::new(routes);

        let thread = {
            let shutdown = Arc::clone(&shutdown);
            std::thread::spawn(move || accept_until_shutdown(&listener, &routes, &shutdown))
        };

        GithubFixtureServer {
            base_url: format!("http://127.0.0.1:{port}"),
            shutdown,
            thread: Some(thread),
        }
    }

    /// Laço de `accept` da fixture, até `shutdown` ser sinalizado no `Drop` — extraído do corpo
    /// de [`start_fixture_on_port`] para não ultrapassar o nível de aninhamento aceito pelo
    /// clippy (mesmo padrão de `e2e_tests::accept_until_shutdown`).
    fn accept_until_shutdown(
        listener: &TcpListener,
        routes: &[(String, FixtureRoute)],
        shutdown: &AtomicBool,
    ) {
        while !shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => serve_connection(stream, routes),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    }
}
