//! Resolução de nome de plugin → `owner/repo` via o repositório-índice central (feature 008,
//! `specs/008-registry-hardening`, US1) — fecha o débito técnico da issue #17 registrada ao final
//! da feature 007 ("criar e publicar de verdade um repositório-índice GitHub central").
//!
//! O índice é um único arquivo `index.toml` na raiz do repositório-índice (D1 de `plan.md`):
//! `[[plugin]]` com `name`/`owner`/`repo` por entrada — mais simples de validar por CI (parse +
//! checagem de unicidade de `name`, responsabilidade do repositório-índice, D2 de `plan.md`) e de
//! buscar (`curl` de uma URL "raw" estática) do que uma API paginada. Sem dependência HTTP nova
//! (mesmo padrão de `install.rs`/D5 e D9 da feature 007): o download em si é feito chamando o
//! binário `curl` já assumido disponível, só o parse usa a crate `toml` já dependência do
//! workspace.
//!
//! **Nota sobre o repositório-índice real (T017 desta feature)**: a criação e publicação do
//! repositório-índice GitHub em si é um checkpoint sensível (`plan.md` § Project Structure) que
//! exige confirmação explícita do usuário sobre nome/owner antes de disparar — fora do escopo
//! desta subtarefa (Foundational, T001-T007). [`default_index_url_base`] documenta o formato de
//! URL esperado como placeholder; nenhum código de produção depende de um repositório-índice já
//! existente até T017 ser executada — todos os testes deste módulo usam um servidor HTTP local de
//! fixture (nunca o GitHub real), mesmo padrão de `install.rs`/`MetricsFixtureServer`.

use std::process::Command;

use serde::Deserialize;

/// Uma entrada resolvida do índice central: nome de instalação + `owner/repo` de origem.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct RegistryIndexEntry {
    pub name: String,
    pub owner: String,
    pub repo: String,
}

/// Erros de [`resolve_name`] — distintos entre si para que o chamador (`install::run_by_name`)
/// consiga produzir uma mensagem específica em vez de um erro genérico (edge case de `spec.md`:
/// índice inacessível MUST ser distinguível de "nome não encontrado").
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RegistryIndexError {
    /// `name` não existe no índice — distinto de qualquer falha de rede/parse.
    NotFound(String),
    /// Falha ao buscar `index.toml` (curl indisponível, status HTTP não-2xx, timeout, etc.).
    FetchFailed(String),
    /// `index.toml` buscado com sucesso, mas não é TOML válido ou não tem o formato esperado.
    MalformedIndex(String),
}

/// Forma bruta desserializada do `index.toml` — tolerante a entrada sem `name`/`owner`/`repo`
/// (ignorada em [`resolve_name`] em vez de abortar a busca inteira; a CI do repositório-índice
/// (T017) é quem garante que nenhuma entrada malformada chega a existir em `main` de verdade).
#[derive(Debug, Default, Deserialize)]
struct RawIndex {
    #[serde(default)]
    plugin: Vec<RawIndexEntry>,
}

#[derive(Debug, Deserialize)]
struct RawIndexEntry {
    name: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
}

/// URL base do repositório-índice central usada quando nenhum override de teste está presente —
/// aponta para o diretório que contém `index.toml` (sem o nome do arquivo, adicionado por
/// [`resolve_name`]). Override só de teste via `FAROL_REGISTRY_INDEX_URL` (mesmo padrão de
/// `FAROL_GITHUB_API_BASE`, D8/D9 de `specs/007-registry-instalacao-plugins-github/research.md`).
///
/// O valor padrão abaixo é um **placeholder**: o repositório-índice real (issue #18, T017 desta
/// feature) ainda não foi criado — nome/owner exatos exigem confirmação explícita do usuário antes
/// da publicação (checkpoint sensível, `plan.md` § Project Structure). Até T017 rodar, uma
/// instalação por nome contra este valor padrão falha com [`RegistryIndexError::FetchFailed`] (o
/// host não existe/não responde) — comportamento correto e visível, nunca uma falha silenciosa.
#[allow(dead_code)]
pub fn default_index_url_base() -> String {
    std::env::var("FAROL_REGISTRY_INDEX_URL")
        .unwrap_or_else(|_| "https://raw.githubusercontent.com/farol-registry/farol-plugin-index/main".to_string())
}

/// Resolve `name` contra o `index.toml` publicado em `{index_url_base}/index.toml`, seguindo o
/// mesmo padrão de busca via `curl` de `install.rs::fetch_latest_release`. `index_url_base` é
/// recebido explicitamente (em vez de lido direto de uma variável de ambiente aqui dentro) para
/// que os testes deste módulo consigam apontar para um servidor HTTP local de fixture sem mutar
/// variáveis de ambiente globais ao processo; [`default_index_url_base`] é quem resolve o valor de
/// produção (com o override de teste `FAROL_REGISTRY_INDEX_URL`) para os chamadores reais
/// (`install::run_by_name`).
#[allow(dead_code)]
pub fn resolve_name(name: &str, index_url_base: &str) -> Result<RegistryIndexEntry, RegistryIndexError> {
    let index_url = format!("{index_url_base}/index.toml");
    let body = fetch_index_toml(&index_url)?;

    let raw: RawIndex =
        toml::from_str(&body).map_err(|err| RegistryIndexError::MalformedIndex(err.to_string()))?;

    for entry in raw.plugin {
        let (Some(entry_name), Some(owner), Some(repo)) = (entry.name, entry.owner, entry.repo) else {
            // Entrada sem os três campos obrigatórios: ignorada, não aborta a busca inteira (a
            // CI do repositório-índice, T017, é a camada que impede isso de chegar a `main`).
            continue;
        };
        if entry_name == name {
            return Ok(RegistryIndexEntry {
                name: entry_name,
                owner,
                repo,
            });
        }
    }

    Err(RegistryIndexError::NotFound(name.to_string()))
}

/// Busca `url` via `curl -sL -w "\n%{http_code}"` (mesmo padrão de
/// `install.rs::fetch_latest_release`) e devolve o corpo da resposta quando o status é `200`.
fn fetch_index_toml(url: &str) -> Result<String, RegistryIndexError> {
    let output = Command::new("curl")
        .args(["-sL", "-w", "\n%{http_code}", url])
        .output()
        .map_err(|err| RegistryIndexError::FetchFailed(format!("falha ao executar curl para {url}: {err}")))?;

    if !output.status.success() {
        return Err(RegistryIndexError::FetchFailed(format!(
            "curl falhou ao consultar {url} (código de saída {:?})",
            output.status.code()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body, status_code) = match stdout.trim_end_matches('\n').rsplit_once('\n') {
        Some((body, status)) => (body.to_string(), status.to_string()),
        None => (String::new(), stdout.trim().to_string()),
    };

    match status_code.trim() {
        "200" => Ok(body),
        other => Err(RegistryIndexError::FetchFailed(format!(
            "{url} devolveu status HTTP inesperado {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    /// Servidor HTTP local de fixture para o índice central — mesmo padrão de
    /// `install::tests::GithubFixtureServer` (`TcpListener` em porta efêmera, thread própria, laço
    /// de `accept` não-bloqueante até `shutdown`), servindo um único corpo fixo para qualquer
    /// caminho requisitado (o índice tem uma única rota, `/index.toml`).
    struct IndexFixtureServer {
        base_url: String,
        shutdown: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for IndexFixtureServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn start_fixture(status: u16, body: &'static str) -> IndexFixtureServer {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("bind da fixture do índice central");
        listener
            .set_nonblocking(true)
            .expect("listener não-bloqueante (para observar o shutdown)");
        let port = listener.local_addr().unwrap().port();

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread = {
            let shutdown = Arc::clone(&shutdown);
            std::thread::spawn(move || accept_until_shutdown(&listener, status, body, &shutdown))
        };

        IndexFixtureServer {
            base_url: format!("http://127.0.0.1:{port}"),
            shutdown,
            thread: Some(thread),
        }
    }

    fn accept_until_shutdown(listener: &TcpListener, status: u16, body: &str, shutdown: &AtomicBool) {
        while !shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => serve_connection(stream, status, body),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    }

    fn serve_connection(mut stream: TcpStream, status: u16, body: &str) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        // Drena a requisição (não precisamos do conteúdo — uma única rota fixa) até a linha em
        // branco que encerra os headers, mesma disciplina de `install::tests::serve_connection`.
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

        let reason = if status == 200 { "OK" } else { "Error" };
        let header = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body.as_bytes());
        let _ = stream.flush();
    }

    #[test]
    fn resolves_a_name_present_in_the_index() {
        let index_toml = "[[plugin]]\nname = \"exemplo\"\nowner = \"dono\"\nrepo = \"exemplo-repo\"\n\n\
                           [[plugin]]\nname = \"outro\"\nowner = \"dono2\"\nrepo = \"outro-repo\"\n";
        let server = start_fixture(200, index_toml);

        let entry = resolve_name("exemplo", &server.base_url).unwrap();
        assert_eq!(
            entry,
            RegistryIndexEntry {
                name: "exemplo".to_string(),
                owner: "dono".to_string(),
                repo: "exemplo-repo".to_string(),
            }
        );

        drop(server);
    }

    #[test]
    fn name_not_present_in_the_index_is_not_found() {
        let index_toml = "[[plugin]]\nname = \"outro\"\nowner = \"dono\"\nrepo = \"outro-repo\"\n";
        let server = start_fixture(200, index_toml);

        let result = resolve_name("nome-inexistente", &server.base_url);
        assert_eq!(
            result,
            Err(RegistryIndexError::NotFound("nome-inexistente".to_string()))
        );

        drop(server);
    }

    #[test]
    fn malformed_index_toml_is_malformed_index() {
        let server = start_fixture(200, "isto não é [ toml válido");

        match resolve_name("exemplo", &server.base_url) {
            Err(RegistryIndexError::MalformedIndex(_)) => {}
            other => panic!("esperava MalformedIndex, obteve {other:?}"),
        }

        drop(server);
    }

    #[test]
    fn index_entry_missing_a_required_field_is_ignored_not_fatal() {
        // Entrada sem `repo` é ignorada silenciosamente (a CI do índice, T017, é quem impede isso
        // de existir em `main` de verdade) — a busca continua e resolve a entrada válida seguinte.
        let index_toml = "[[plugin]]\nname = \"incompleto\"\nowner = \"dono\"\n\n\
                           [[plugin]]\nname = \"exemplo\"\nowner = \"dono\"\nrepo = \"exemplo-repo\"\n";
        let server = start_fixture(200, index_toml);

        let entry = resolve_name("exemplo", &server.base_url).unwrap();
        assert_eq!(entry.repo, "exemplo-repo");

        let missing = resolve_name("incompleto", &server.base_url);
        assert_eq!(
            missing,
            Err(RegistryIndexError::NotFound("incompleto".to_string()))
        );

        drop(server);
    }

    #[test]
    fn http_error_status_is_fetch_failed() {
        let server = start_fixture(500, "internal error");

        match resolve_name("exemplo", &server.base_url) {
            Err(RegistryIndexError::FetchFailed(_)) => {}
            other => panic!("esperava FetchFailed, obteve {other:?}"),
        }

        drop(server);
    }

    #[test]
    fn unreachable_index_url_is_fetch_failed() {
        // Porta que quase certamente não tem nada escutando (mesma disciplina de "servidor
        // inacessível" usada por outras suítes deste crate — sem depender de rede externa).
        match resolve_name("exemplo", "http://127.0.0.1:1") {
            Err(RegistryIndexError::FetchFailed(_)) => {}
            other => panic!("esperava FetchFailed, obteve {other:?}"),
        }
    }
}
