//! Camada 1 do harness de execução real (`research.md` D5, US1) — testes que
//! rodam o `Program` real do Farol (`crate::program`, T002) dentro do
//! `iced_test::Emulator`, headless, sem GPU/X11/Wayland.
//!
//! # Por que este módulo mora em `src/`, e não em `crates/farol-core/tests/`
//!
//! `plan.md`/`tasks.md` da feature 003 previam `crates/farol-core/tests/
//! e2e_harness.rs`. Isso é **estruturalmente impossível** neste crate: ele é
//! só-`bin` (`[[bin]] name = "farol"`, sem `src/lib.rs` nem target `lib`), e
//! um teste de integração em `tests/` compila como crate separado, que só
//! consegue importar de um target `lib` (achado N3 da "Nota de execução
//! (2026-09-01)" em `research.md`). Em vez de criar um `src/lib.rs` só para
//! isso — o que obrigaria a alargar visibilidades (`pub(crate)` → `pub`) em
//! `model.rs`/`update.rs`/`plugin_worker.rs`/`view.rs` —, este módulo segue
//! **o padrão que os 32 testes já existentes deste crate usam**:
//! `#[cfg(test)] mod` dentro do próprio bin target (ver `update.rs`,
//! `config_store.rs`, `secrets_store.rs`).
//!
//! # O que estes testes provam (e o que não provam)
//!
//! O `Emulator` roda o `Program` de verdade: `boot` → `subscription()` →
//! recipes montadas e polladas pelo executor tokio real → `update()` a cada
//! mensagem. Para o Farol isso significa processo filho de plugin **real**
//! (`tokio::process`, `kill_on_drop`), handshake JSON-RPC/NDJSON **real** por
//! stdin/stdout, e transição de `PluginState` **real** em `update.rs`. Não é
//! simulação: é o mesmo código que `cargo run --bin farol` executa, menos o
//! backend de janela (winit) — coberto pela Camada 2 (`tests/integration/
//! harness.sh`, T008).
//!
//! # Orçamento de tempo (T007, `## Clarifications` de `spec.md`)
//!
//! Dois tetos distintos, ambos aplicados de verdade (não só documentados):
//!
//! - [`STATE_TIMEOUT`] (30s) — por **verificação individual**: cada espera por
//!   um estado/uma tela específica. Excedê-lo falha nomeando o `plugin_name` e
//!   o `PluginState` realmente observado (FR-003/FR-004).
//! - [`SCENARIO_TIMEOUT`] (120s) — por **cenário inteiro**, via
//!   [`ScenarioBudget`], checado dentro de cada laço de espera e reafirmado ao
//!   final. Um cenário que estourasse o teto falha explicitamente como
//!   estouro de orçamento, categoria distinta de uma falha de asserção.
//!
//! Cada cenário também confirma, ao final, que **nenhum processo filho
//! remanesce** ([`assert_no_lingering_children`]) — a contraparte executável da
//! garantia `kill_on_drop` de `plugin_worker.rs`
//! (`contracts/e2e-harness-contract.md` Camada 1).

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc;
use iced::futures::executor;
use iced::futures::StreamExt;
use iced::Size;
use iced_test::emulator::{self, Emulator, Mode};
use iced_test::instruction::Expectation;
use iced_test::program::Program;
use iced_test::Instruction;

use crate::model::{PluginState, UnavailableReason};
use crate::plugin_worker::PluginSpawnConfig;
use crate::{Farol, Message};

/// Orçamento por verificação de estado (`## Clarifications` de `spec.md`:
/// 30s). Excedê-lo falha o teste com o `PluginState` realmente observado, não
/// com um "timeout" opaco (FR-003/FR-004).
const STATE_TIMEOUT: Duration = Duration::from_secs(30);

/// Orçamento do **cenário inteiro** (`## Clarifications` de `spec.md`: 120s),
/// aplicado por [`ScenarioBudget`]. Cobre tudo entre o início do teste e a
/// última asserção — spawn, handshake, ciclos de `widget/get` e a confirmação
/// de que nenhum processo filho remanesceu (T007).
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(120);

/// Viewport headless do `Emulator` — o mesmo default de `iced` (1024x768).
const VIEWPORT: Size = Size::new(1024.0, 768.0);

/// `base_url` da fixture de `uptime-kuma` **quando o cenário não precisa de
/// dados de widget**: porta 1 de `127.0.0.1`, que nunca tem serviço
/// escutando — a `PollerThread` do plugin recebe "connection refused"
/// imediatamente, sem latência e **sem nenhum tráfego de rede para fora da
/// máquina**. Suficiente para o cenário de `Ready` do gate T004, que depende
/// só do handshake + `required_config` resolvido. O cenário com dados de
/// verdade (T006) usa [`MetricsFixtureServer`] no lugar disto.
const UPTIME_KUMA_UNREACHABLE_BASE_URL: &str = "http://127.0.0.1:1";

/// API key sintética da fixture (`research.md` D2) — nunca usada contra
/// nenhuma instância real de Uptime Kuma.
const UPTIME_KUMA_FIXTURE_API_KEY: &str = "farol-e2e-fixture-key";

/// Serializa os testes E2E deste módulo.
///
/// Eles são os únicos testes do crate que mexem em `$XDG_CONFIG_HOME` (uma
/// variável de ambiente é global ao processo, e `cargo test` roda os testes
/// em threads do mesmo processo). Serializá-los é o que permite cada um ter
/// sua própria fixture hermética, criada e **removida** no escopo do teste,
/// em vez de um diretório temporário compartilhado que sobreviveria ao fim do
/// processo. Também é o que torna [`assert_no_lingering_children`] uma
/// asserção honesta: nenhum outro cenário pode ter um processo filho legítimo
/// em voo enquanto ela roda.
static E2E_LOCK: Mutex<()> = Mutex::new(());

fn e2e_guard() -> MutexGuard<'static, ()> {
    // Um teste E2E que falhe envenena o mutex; o próximo não deve falhar por
    // tabela — o dado protegido é `()`, não há estado inconsistente possível.
    E2E_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// T007 — orçamento de tempo do cenário inteiro
// ---------------------------------------------------------------------------

/// Teto de [`SCENARIO_TIMEOUT`] para um cenário inteiro (T007).
///
/// Diferente de [`STATE_TIMEOUT`], que limita **uma** espera, este orçamento
/// atravessa o cenário todo: é consultado dentro de cada laço de espera
/// ([`ScenarioBudget::check`]) e reafirmado no encerramento
/// ([`ScenarioBudget::finish`]), então nem uma sequência de esperas
/// individualmente dentro do limite pode fazer um cenário passar do teto sem
/// que isso apareça como falha explícita.
struct ScenarioBudget {
    scenario: &'static str,
    started: Instant,
}

impl ScenarioBudget {
    fn start(scenario: &'static str) -> Self {
        Self {
            scenario,
            started: Instant::now(),
        }
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Falha o cenário se o orçamento já estourou. `stage` nomeia o ponto em
    /// que o estouro foi detectado, para a mensagem não exigir leitura de log
    /// bruto (FR-004).
    fn check(&self, stage: &str) {
        let elapsed = self.elapsed();
        assert!(
            elapsed < SCENARIO_TIMEOUT,
            "cenário {:?} estourou o orçamento de {SCENARIO_TIMEOUT:?} em {stage:?} (decorrido: \
             {elapsed:?})",
            self.scenario
        );
    }

    /// Reafirma o orçamento no fim do cenário e devolve o tempo decorrido.
    fn finish(self) -> Duration {
        self.check("encerramento do cenário");
        self.elapsed()
    }
}

// ---------------------------------------------------------------------------
// T003 — fixture determinística dos plugins de referência
// ---------------------------------------------------------------------------

/// Raiz do repositório, derivada de `CARGO_MANIFEST_DIR`
/// (`<raiz>/crates/farol-core`) — **nunca** do `cwd` do processo.
///
/// `plugin_worker::known_plugins()` usa caminhos relativos
/// (`plugins/git-local/main.py`) e só funciona com `cwd` na raiz do repo; sob
/// `cargo test` o `cwd` é o diretório do crate. Resolver o caminho absoluto
/// aqui é o que dispensa qualquer mutação de `cwd` global no binário de teste.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR deve ser <raiz>/crates/farol-core")
}

/// Fixture determinística dos plugins de referência (T003, `research.md` D2),
/// sob um diretório temporário próprio que é removido no `Drop`:
///
/// - **`git-local`**: um repositório git **real** criado do zero, mais um
///   `config.toml` apontando `scan_root` para ele.
/// - **`uptime-kuma`**: `config.toml` (`base_url`) + `secrets.toml`
///   (`api_key`) sintéticos — o suficiente para o `required_config` do
///   handshake resolver inteiro pelo mesmo mecanismo de produção
///   (`config_store`/`secrets_store` → variável de ambiente injetada no
///   spawn, D8 da feature 002).
///
/// Hermetismo: o diretório temporário também vira `$XDG_CONFIG_HOME` do
/// processo de teste (ver [`HarnessFixture::new`]), então **os dois lados**
/// leem dali — o core (`crate::config_store::farol_config_base_dir`) e cada
/// processo de plugin (que aplica a mesma convenção XDG e herda o ambiente do
/// pai no spawn). Nenhum teste toca o `~/.config/farol` real da máquina.
///
/// Nenhuma credencial real, nenhuma rede para fora de `127.0.0.1`: um
/// `git init` seguido de um commit local, com identidade e datas fixas e
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` neutralizados (para o resultado não
/// depender do `~/.gitconfig` de quem roda), e uma API key sintética
/// ([`UPTIME_KUMA_FIXTURE_API_KEY`]).
struct HarnessFixture {
    base: PathBuf,
    scan_root: PathBuf,
}

impl HarnessFixture {
    /// Fixture com `uptime-kuma` apontado para
    /// [`UPTIME_KUMA_UNREACHABLE_BASE_URL`] — cenários que só precisam do
    /// handshake/`required_config` (T004).
    fn new(label: &str) -> Self {
        Self::with_uptime_kuma_base_url(label, UPTIME_KUMA_UNREACHABLE_BASE_URL)
    }

    /// Fixture com `base_url` de `uptime-kuma` explícito — usada por T006 para
    /// apontar o plugin ao [`MetricsFixtureServer`] em sua porta efêmera.
    fn with_uptime_kuma_base_url(label: &str, uptime_kuma_base_url: &str) -> Self {
        let base = std::env::temp_dir().join(format!("farol-e2e-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);

        let scan_root = base.join("repos");
        let repo = scan_root.join("exemplo");
        fs::create_dir_all(&repo).expect("criar scan_root da fixture");

        git(&repo, &["init", "--quiet", "--initial-branch=main"]);
        fs::write(repo.join("README.md"), "fixture determinística do harness\n")
            .expect("escrever arquivo da fixture");
        git(&repo, &["add", "README.md"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=Farol Harness",
                "-c",
                "user.email=harness@farol.invalid",
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "--message=commit inicial da fixture",
            ],
        );

        // Confirma que a fixture é de fato um repositório git válido — se o
        // `git` local se comportar de forma inesperada, o teste falha aqui,
        // com causa óbvia, em vez de mais tarde por um sintoma indireto.
        let head = git(&repo, &["rev-parse", "--verify", "HEAD"]);
        assert_eq!(head.len(), 40, "HEAD da fixture deveria ser um SHA-1: {head:?}");

        let farol_config = base.join("xdg").join("farol");
        write_plugin_config(
            &farol_config,
            "git-local",
            &format!("scan_root = {:?}\n", scan_root.to_string_lossy()),
        );
        write_plugin_config(
            &farol_config,
            "uptime-kuma",
            &format!("base_url = {uptime_kuma_base_url:?}\n"),
        );
        fs::write(
            farol_config.join("secrets.toml"),
            format!("[uptime-kuma]\napi_key = {UPTIME_KUMA_FIXTURE_API_KEY:?}\n"),
        )
        .expect("escrever secrets.toml da fixture");

        // `set_var` é global ao processo — seguro aqui porque todo teste que
        // depende disso segura `E2E_LOCK`, e nenhum outro teste deste crate lê
        // `XDG_CONFIG_HOME`/`HOME` (os de `config_store`/`secrets_store`
        // recebem o diretório-base por parâmetro justamente para não depender
        // do ambiente real).
        std::env::set_var("XDG_CONFIG_HOME", base.join("xdg"));

        Self { base, scan_root }
    }

    /// `PluginSpawnConfig` de um plugin de referência apontando para o
    /// `main.py` real do repositório, por caminho **absoluto** — mesmo
    /// comando/args de `plugin_worker::known_plugins()`, só sem a dependência
    /// de `cwd`.
    fn spawn_config(&self, plugin_name: &str) -> PluginSpawnConfig {
        let main_py = repo_root().join("plugins").join(plugin_name).join("main.py");
        assert!(
            main_py.is_file(),
            "o `main.py` de {plugin_name} deveria existir em {main_py:?}"
        );

        PluginSpawnConfig {
            plugin_name: plugin_name.to_string(),
            command: "python3".to_string(),
            args: vec![main_py.to_string_lossy().into_owned()],
        }
    }
}

impl Drop for HarnessFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Escreve o `config.toml` de um plugin sob a base de configuração da fixture
/// (`<base>/xdg/farol/plugins/<plugin>/config.toml` — o mesmo layout que
/// `config_store::plugin_config_path` resolve).
fn write_plugin_config(farol_config: &Path, plugin_name: &str, contents: &str) {
    let dir = farol_config.join("plugins").join(plugin_name);
    fs::create_dir_all(&dir).expect("criar diretório de config da fixture");
    fs::write(dir.join("config.toml"), contents).expect("escrever config.toml da fixture");
}

/// Roda `git` dentro de `repo` com o ambiente neutralizado e devolve o
/// `stdout` sem espaços nas pontas. Falha o teste com a saída de erro real do
/// `git` quando o comando não sucede.
fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("falha ao executar `git {}`: {err}", args.join(" ")));

    assert!(
        output.status.success(),
        "`git {}` falhou ({}): {}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ---------------------------------------------------------------------------
// T006 — duplo HTTP determinístico do endpoint `/metrics` (`research.md` D2)
// ---------------------------------------------------------------------------

/// Corpo Prometheus servido pelo [`MetricsFixtureServer`] — três monitores com
/// status e tempo de resposta **conhecidos** (`research.md` D2: "2–3 monitores
/// com status/tempo de resposta conhecidos").
///
/// O formato é o mesmo exercitado por `plugins/uptime-kuma/
/// test_metrics_parser.py` (linhas reais capturadas de uma instância Uptime
/// Kuma), incluindo deliberadamente os três casos que o parser trata de forma
/// distinta:
///
/// 1. `monitor_response_time` fracionário (`42.4`) — arredondado para `42`;
/// 2. `monitor_status 0` — mapeado para `down`;
/// 3. `monitor_response_time -1` — o **sentinela** do Uptime Kuma para "não
///    aplicável" (monitores `docker`), que MUST virar `response_time_ms: null`
///    e nunca `-1`. É o caso do commit `2608c03`: um `-1` que chegasse ao core
///    derrubaria o decode do widget inteiro (`Option<u32>` não representa
///    negativo — `research.md` D3). Servi-lo aqui faz deste cenário também um
///    teste de regressão de ponta a ponta daquele defeito, não só do parser.
///
/// Linhas de comentário (`# HELP`/`# TYPE`) e uma família de métrica que o
/// parser ignora (`monitor_cert_days_remaining`) estão presentes de propósito:
/// um `/metrics` real as traz, e a fixture não deve ser mais fácil de parsear
/// que a realidade.
const METRICS_FIXTURE_BODY: &str = concat!(
    "# HELP monitor_cert_days_remaining Monitor Certificate Days Remaining\n",
    "# TYPE monitor_cert_days_remaining gauge\n",
    "monitor_cert_days_remaining{monitor_name=\"farol-api\"} 89\n",
    "# HELP monitor_response_time Monitor Response Time (ms)\n",
    "# TYPE monitor_response_time gauge\n",
    "monitor_response_time{monitor_id=\"1\",monitor_name=\"farol-api\",monitor_type=\"http\",",
    "monitor_url=\"https://api.invalid\",monitor_hostname=\"null\",monitor_port=\"null\"} 42.4\n",
    "monitor_response_time{monitor_id=\"2\",monitor_name=\"farol-db\",monitor_type=\"port\",",
    "monitor_url=\"null\",monitor_hostname=\"db.invalid\",monitor_port=\"5432\"} 7\n",
    "monitor_response_time{monitor_id=\"3\",monitor_name=\"farol-container\",",
    "monitor_type=\"docker\",monitor_url=\"https://\",monitor_hostname=\"null\",",
    "monitor_port=\"null\"} -1\n",
    "# HELP monitor_status Monitor Status\n",
    "# TYPE monitor_status gauge\n",
    "monitor_status{monitor_id=\"1\",monitor_name=\"farol-api\",monitor_type=\"http\",",
    "monitor_url=\"https://api.invalid\",monitor_hostname=\"null\",monitor_port=\"null\"} 1\n",
    "monitor_status{monitor_id=\"2\",monitor_name=\"farol-db\",monitor_type=\"port\",",
    "monitor_url=\"null\",monitor_hostname=\"db.invalid\",monitor_port=\"5432\"} 0\n",
    "monitor_status{monitor_id=\"3\",monitor_name=\"farol-container\",monitor_type=\"docker\",",
    "monitor_url=\"https://\",monitor_hostname=\"null\",monitor_port=\"null\"} 1\n",
);

/// Os monitores que [`METRICS_FIXTURE_BODY`] MUST produzir, na ordem em que
/// `plugins/uptime-kuma/metrics_parser.py` os emite (ordem de aparição das
/// linhas `monitor_status`).
fn expected_monitors() -> Vec<farol_protocol::messages::MonitorStatusItem> {
    use farol_protocol::messages::{MonitorStatus, MonitorStatusItem};

    vec![
        MonitorStatusItem {
            name: "farol-api".to_string(),
            status: MonitorStatus::Up,
            response_time_ms: Some(42),
        },
        MonitorStatusItem {
            name: "farol-db".to_string(),
            status: MonitorStatus::Down,
            response_time_ms: Some(7),
        },
        MonitorStatusItem {
            name: "farol-container".to_string(),
            status: MonitorStatus::Up,
            response_time_ms: None,
        },
    ]
}

/// Duplo determinístico do endpoint `/metrics` de uma instância Uptime Kuma
/// (T006, `research.md` D2), servido em `127.0.0.1` numa **porta efêmera**
/// (`:0`) por uma thread própria.
///
/// # Por que `std::net::TcpListener` e não `tokio`/`hyper`
///
/// `research.md` D2 deixou o mecanismo como decisão de implementação. Uma
/// thread com a `TcpListener` da stdlib (a) não acrescenta nenhuma dependência
/// nova ao crate — `hyper`/`axum` seriam dependências novas só para teste —,
/// e (b) fica **fora** do runtime tokio que o `Emulator` cria e destrói, então
/// o servidor não compete com nem depende do ciclo de vida do runtime sob
/// teste. O protocolo exercitado é trivialmente pequeno: uma requisição
/// `GET /metrics` por poll, sem keep-alive (`urllib` da stdlib do Python manda
/// `Connection: close`).
///
/// Autenticação: HTTP Basic com usuário vazio e [`UPTIME_KUMA_FIXTURE_API_KEY`]
/// como senha — exatamente o que `plugins/uptime-kuma/metrics_client.py`
/// monta. Uma requisição sem o header correto recebe `401` e é contada em
/// [`MetricsFixtureServer::unauthorized_requests`]: se o core deixasse de
/// injetar o segredo pelo caminho de produção, o cenário falharia apontando
/// isso, em vez de silenciosamente não ter dados.
struct MetricsFixtureServer {
    base_url: String,
    authorized: Arc<AtomicUsize>,
    unauthorized: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MetricsFixtureServer {
    fn start() -> Self {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("bind da fixture de /metrics em 127.0.0.1:0");
        let port = listener.local_addr().expect("porta efêmera da fixture").port();
        listener
            .set_nonblocking(true)
            .expect("listener não-bloqueante (para o laço poder observar o shutdown)");

        let authorized = Arc::new(AtomicUsize::new(0));
        let unauthorized = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread = {
            let authorized = Arc::clone(&authorized);
            let unauthorized = Arc::clone(&unauthorized);
            let shutdown = Arc::clone(&shutdown);

            std::thread::spawn(move || {
                accept_until_shutdown(&listener, &authorized, &unauthorized, &shutdown);
            })
        };

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            authorized,
            unauthorized,
            shutdown,
            thread: Some(thread),
        }
    }

    fn authorized_requests(&self) -> usize {
        self.authorized.load(Ordering::Relaxed)
    }

    fn unauthorized_requests(&self) -> usize {
        self.unauthorized.load(Ordering::Relaxed)
    }
}

impl Drop for MetricsFixtureServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Laço de `accept` da fixture, até `shutdown` ser sinalizado no `Drop`.
///
/// O `listener` está em modo não-bloqueante para que o laço consiga observar o
/// `shutdown` mesmo sem nenhuma conexão chegando — daí o `WouldBlock` ser um
/// caso normal, não um erro.
fn accept_until_shutdown(
    listener: &TcpListener,
    authorized: &AtomicUsize,
    unauthorized: &AtomicUsize,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => serve_metrics_connection(stream, authorized, unauthorized),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            },
            Err(_) => break,
        }
    }
}

/// Atende uma única conexão do duplo de `/metrics`: lê os headers, decide
/// entre `200`/`401`/`404` e fecha. Erros de I/O são ignorados de propósito —
/// um cliente que desiste no meio (ex.: o plugin sendo morto por
/// `kill_on_drop`) não é falha da fixture, e o cenário já falha por outro
/// caminho se os dados não chegarem.
fn serve_metrics_connection(
    mut stream: TcpStream,
    authorized: &AtomicUsize,
    unauthorized: &AtomicUsize,
) {
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
            },
            Err(_) => return,
        }
    }

    let request = String::from_utf8_lossy(&request);
    let request_line = request.lines().next().unwrap_or_default();
    let expected_credentials = base64_encode(format!(":{UPTIME_KUMA_FIXTURE_API_KEY}").as_bytes());
    let authenticated = request.lines().any(|line| {
        let (name, value) = match line.split_once(':') {
            Some(parts) => parts,
            None => return false,
        };
        name.eq_ignore_ascii_case("authorization")
            && value.trim() == format!("Basic {expected_credentials}")
    });

    let response = if !request_line.starts_with("GET /metrics ") {
        http_response(404, "text/plain; charset=utf-8", "not found\n")
    } else if !authenticated {
        unauthorized.fetch_add(1, Ordering::Relaxed);
        http_response(401, "text/plain; charset=utf-8", "unauthorized\n")
    } else {
        authorized.fetch_add(1, Ordering::Relaxed);
        http_response(200, "text/plain; version=0.0.4; charset=utf-8", METRICS_FIXTURE_BODY)
    };

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn http_response(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        _ => "Not Found",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Base64 padrão (RFC 4648), só para montar/conferir o header `Authorization`
/// da fixture. Escrito à mão em vez de puxar uma dependência nova para o
/// crate por causa de uma única string de 25 bytes conhecida em tempo de
/// compilação.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let byte1 = u32::from(chunk[0]);
        let byte2 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let byte3 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let group = (byte1 << 16) | (byte2 << 8) | byte3;

        encoded.push(ALPHABET[((group >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((group >> 12) & 0x3f) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((group >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(group & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

// ---------------------------------------------------------------------------
// T004/T006 — dirigir o `Program` real dentro do `Emulator`
// ---------------------------------------------------------------------------

/// Estado observável do slot de um plugin depois de o `Emulator` ter rodado.
fn plugin_state(app: &Farol, plugin_name: &str) -> PluginState {
    app.plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == plugin_name)
        .unwrap_or_else(|| panic!("nenhum slot para o plugin {plugin_name:?}"))
        .connection
        .state
        .clone()
}

/// Textos que `view.rs` renderiza enquanto a conexão ainda **não** alcançou um
/// estado terminal (`PluginState::Starting`/`Handshaking`). Duplicados aqui de
/// propósito: são o único sinal que o `Emulator` expõe sem consumir seu estado
/// interno (`Emulator::into_state` recebe `self` por valor), e é sobre a tela
/// de verdade — não sobre o modelo — que a espera se baseia.
const TRANSIENT_SCREEN_TEXTS: [&str; 2] =
    ["Iniciando plugin...", "Aguardando handshake do plugin..."];

type Receiver<P> = mpsc::Receiver<emulator::Event<P>>;

/// Um cenário do harness de Camada 1: o `Program` real do Farol, um
/// `Emulator` dirigindo-o, e o orçamento de tempo de T007.
///
/// O `Emulator` fica dentro de um `Option` porque `Emulator::into_state`
/// recebe `self` por valor: guardá-lo assim permite que um método `&mut self`
/// (o caminho de falha por timeout) ainda consiga extrair o `Farol` real para
/// nomear o `PluginState` observado, em vez de reportar um "timeout" opaco
/// (FR-003/FR-004).
struct Scenario<P>
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    program: P,
    emulator: Option<Emulator<P>>,
    receiver: Receiver<P>,
    plugin_name: String,
    budget: ScenarioBudget,
}

impl<P> Scenario<P>
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    /// `true` ⟺ a tela atual do `Emulator` contém um widget de texto cujo
    /// conteúdo é exatamente `text`.
    ///
    /// `Emulator::run` responde a um `Instruction::Expect` com exatamente um
    /// `Event::Ready` (encontrou) ou `Event::Failed` (não encontrou), então
    /// este laço sempre termina. Qualquer `Event::Action` que chegue antes
    /// disso (ex.: uma mensagem recém-produzida pela `Subscription` do worker)
    /// é aplicado no caminho — é aqui que o app de fato progride entre duas
    /// consultas.
    fn screen_shows(&mut self, text: &str) -> bool {
        let instruction = Instruction::Expect(Expectation::Text(text.to_string()));
        let program = &self.program;
        self.emulator
            .as_mut()
            .expect("o Emulator do cenário só é consumido no encerramento")
            .run(program, instruction);

        loop {
            match next_event(&mut self.receiver) {
                emulator::Event::Action(action) => {
                    let program = &self.program;
                    self.emulator
                        .as_mut()
                        .expect("o Emulator do cenário só é consumido no encerramento")
                        .perform(program, action);
                },
                emulator::Event::Ready => return true,
                emulator::Event::Failed(_) => return false,
            }
        }
    }

    /// Bombeia o loop do `Emulator` até a tela do Farol deixar de mostrar
    /// qualquer um de [`TRANSIENT_SCREEN_TEXTS`] — ou seja, até a conexão sair
    /// de `Starting`/`Handshaking`.
    fn settle(&mut self) {
        let deadline = Instant::now() + STATE_TIMEOUT;

        loop {
            self.budget.check("espera pelo estado terminal do plugin");

            let still_transient = TRANSIENT_SCREEN_TEXTS
                .iter()
                .any(|text| self.screen_shows(text));
            if !still_transient {
                return;
            }

            if Instant::now() >= deadline {
                // FR-003/FR-004: a mensagem nomeia o plugin e o estado
                // realmente observado, sem exigir leitura de log bruto.
                self.fail_with_observed_state(format!(
                    "plugin {:?} não saiu de Starting/Handshaking em {STATE_TIMEOUT:?}",
                    self.plugin_name
                ));
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Falha o cenário nomeando o `PluginState` real observado no modelo.
    fn fail_with_observed_state(&mut self, headline: String) -> ! {
        let app = self
            .emulator
            .take()
            .expect("o Emulator do cenário só é consumido no encerramento")
            .into_state()
            .0;
        let observed = plugin_state(&app, &self.plugin_name);
        panic!("{headline} — estado observado: {observed:?}");
    }

    /// Aplica uma `Message` do próprio app pelo `update()` real.
    ///
    /// Usado por T006 só para `Message::RefreshTick` — ver a nota em
    /// [`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`] sobre por
    /// que a espera pelo primeiro ciclo de dados não pode depender do timer de
    /// 30s do plugin.
    fn dispatch(&mut self, message: Message) {
        let program = &self.program;
        self.emulator
            .as_mut()
            .expect("o Emulator do cenário só é consumido no encerramento")
            .update(program, message);
    }

    /// Encerra o cenário: consome o `Emulator` (o que derruba o runtime tokio
    /// e, com ele, o processo filho do plugin via `kill_on_drop`), reafirma o
    /// orçamento de T007 e devolve o `Farol` real para asserções sobre o
    /// modelo.
    fn finish(mut self) -> Farol {
        let app = self
            .emulator
            .take()
            .expect("o Emulator do cenário só é consumido no encerramento")
            .into_state()
            .0;
        let elapsed = self.budget.finish();
        // Visível com `cargo test -- --nocapture`; é o dado que sustenta a
        // afirmação de T007 de que os cenários ficam com folga larga dentro do
        // teto de 120s.
        eprintln!("[e2e] cenário concluído em {elapsed:?} (teto {SCENARIO_TIMEOUT:?})");
        app
    }
}

/// Monta o `Program` real com um único slot de plugin (apontado para a
/// fixture, por caminho absoluto) e o coloca sob um `Emulator` pronto para ser
/// bombeado.
///
/// Um slot só, e não `Farol::default()`, porque `known_plugins()` traria os
/// dois plugins de referência: cada cenário afirma sobre um deles, e spawnar
/// o outro junto só adicionaria variabilidade sem cobrir nada a mais.
fn start_scenario(
    scenario: &'static str,
    spawn_config: PluginSpawnConfig,
) -> Scenario<impl Program<State = Farol, Message = Message>> {
    let budget = ScenarioBudget::start(scenario);
    let plugin_name = spawn_config.plugin_name.clone();
    let program = crate::program(move || Farol::with_plugins(vec![spawn_config.clone()]));

    let (sender, mut receiver) = mpsc::channel(100);
    let mut emulator = Emulator::new(sender, &program, Mode::Immediate, VIEWPORT);

    // `Emulator::new` termina o boot emitindo exatamente um `Event::Ready`
    // (`Mode::Immediate`: `wait_for` com `Task::none()` e nenhuma task
    // pendente). Ele precisa ser consumido **antes** de qualquer consulta de
    // `screen_shows`, senão a primeira delas o interpretaria como a resposta
    // do seu próprio `Expect` e todas as respostas seguintes ficariam
    // deslocadas em um — um teste que "passa" pelo motivo errado, exatamente a
    // classe de diagnóstico enganoso que esta feature existe para eliminar.
    consume_ready(&program, &mut emulator, &mut receiver);

    Scenario {
        program,
        emulator: Some(emulator),
        receiver,
        plugin_name,
        budget,
    }
}

/// Roda um cenário até a conexão sair de `Starting`/`Handshaking` e devolve o
/// `PluginState` alcançado, já com o orçamento de T007 reafirmado e sem
/// processo filho remanescente.
fn observe_plugin_state(scenario: &'static str, spawn_config: PluginSpawnConfig) -> PluginState {
    let mut harness = start_scenario(scenario, spawn_config);
    harness.settle();

    let plugin_name = harness.plugin_name.clone();
    let app = harness.finish();
    let state = plugin_state(&app, &plugin_name);
    drop(app);

    assert_no_lingering_children();
    state
}

/// Consome eventos até um `Event::Ready`, aplicando no caminho todo
/// `Event::Action` que chegar antes dele — é assim que a aplicação progride.
fn consume_ready<P>(program: &P, emulator: &mut Emulator<P>, receiver: &mut Receiver<P>)
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    loop {
        match next_event(receiver) {
            emulator::Event::Action(action) => emulator.perform(program, action),
            emulator::Event::Ready => return,
            emulator::Event::Failed(instruction) => {
                panic!("nenhuma instrução deveria estar em voo aqui, mas {instruction} falhou")
            },
        }
    }
}

/// Próximo evento do `Emulator`, bloqueando a thread do teste.
///
/// Nunca bloqueia indefinidamente nos usos deste módulo: só é chamada quando
/// há uma resposta garantida a caminho (o `Event::Ready` do boot, ou o
/// `Ready`/`Failed` de um `Instruction::Expect` recém-submetido).
fn next_event<P>(receiver: &mut Receiver<P>) -> emulator::Event<P>
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    executor::block_on(receiver.next()).expect("o runtime do Emulator não deveria encerrar sozinho")
}

// ---------------------------------------------------------------------------
// T007 — nenhum processo filho remanescente ao fim de um cenário
// ---------------------------------------------------------------------------

/// Espera curta (bem dentro de [`SCENARIO_TIMEOUT`]) para os filhos diretos
/// deste processo desaparecerem depois que o `Emulator` — e com ele o runtime
/// tokio e o `Child` com `kill_on_drop` — é derrubado.
const CHILD_REAP_TIMEOUT: Duration = Duration::from_secs(10);

/// Confirma que nenhum processo filho **vivo** deste processo de teste
/// remanesce (`contracts/e2e-harness-contract.md` Camada 1, saída esperada de
/// sucesso).
///
/// Zumbis (estado `Z` em `/proc/<pid>/stat`) são deliberadamente ignorados:
/// são entradas de tabela de processo à espera de `wait()`, sem nenhum
/// recurso de execução — o `kill_on_drop` do `tokio` mata o processo de fato,
/// mas o reaping fica a cargo do driver de processos do runtime, que está
/// sendo desmontado exatamente nesse instante. O que o contrato exige (e o que
/// custa caro na prática) é não deixar processo **executando**; é isso que
/// esta função afirma.
///
/// Linux-only por construção (`/proc`), consistente com o Princípio I da
/// constitution e com `## Out of Scope` de `spec.md` (nenhuma outra plataforma
/// no escopo desta infraestrutura).
fn assert_no_lingering_children() {
    let deadline = Instant::now() + CHILD_REAP_TIMEOUT;

    loop {
        let alive = live_child_processes();
        if alive.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "processos filhos ainda vivos {CHILD_REAP_TIMEOUT:?} depois do fim do cenário \
                 (kill_on_drop deveria tê-los encerrado): {alive:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Filhos diretos deste processo que não estão em estado `Z` (zumbi), como
/// pares `(pid, cmdline)` — a `cmdline` entra na mensagem de falha para o
/// diagnóstico não exigir um `ps` manual depois (FR-004).
fn live_child_processes() -> Vec<(i32, String)> {
    let me = std::process::id();
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut children = Vec::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // O campo `comm` é o único que pode conter espaços/parênteses; tudo
        // que interessa aqui vem depois do último `)`.
        let Some(after_comm) = stat.rsplit_once(')').map(|(_, rest)| rest) else {
            continue;
        };
        let mut fields = after_comm.split_whitespace();
        let (Some(state), Some(ppid)) = (fields.next(), fields.next()) else {
            continue;
        };
        if state == "Z" || ppid.parse::<u32>().ok() != Some(me) {
            continue;
        }

        let cmdline = fs::read(entry.path().join("cmdline"))
            .map(|raw| {
                String::from_utf8_lossy(&raw)
                    .split('\0')
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        children.push((pid, cmdline));
    }
    children
}

// ---------------------------------------------------------------------------
// Cenários
// ---------------------------------------------------------------------------

/// **T004 (gate da feature 003, redefinido — cenário `Ready`)** — o
/// `Emulator` do `iced_test` roda `Farol::subscription()` de verdade e leva
/// uma conexão de plugin **real** até `PluginState::Ready`.
///
/// É este o teste que sustenta US1 daqui em diante: chegar a `Ready` exige que
/// **todo** o mecanismo real tenha rodado dentro do `Emulator`, sem simulação
/// em nenhum ponto —
///
/// 1. `Farol::subscription()` montada de verdade e suas recipes polladas pelo
///    executor tokio real (`iced_test::emulator::Emulator::resubscribe` chama
///    `program.subscription(&state)`);
/// 2. `plugin_worker::worker` spawnando um processo filho **real**
///    (`tokio::process`, `python3 plugins/uptime-kuma/main.py`), com os
///    valores da fixture injetados como variável de ambiente pelo mesmo
///    caminho de produção (`config_store`/`secrets_store` → `FAROL_PLUGIN_*`);
/// 3. `handshake/hello` JSON-RPC/NDJSON **real** escrito no stdin do processo
///    e a resposta **real** lida do stdout dele;
/// 4. `required_config` (`base_url` + `api_key`) resolvido de verdade contra
///    o storage do core, que é o que decide `Ready` em vez de
///    `Unavailable{NotConfigured}` (T019/D8 da feature 002);
/// 5. a transição real aplicada por `update.rs::handle_handshake_outcome`.
///
/// **Por que `uptime-kuma` e não `git-local`**: ver
/// [`emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state`]
/// — `git-local` ainda fala protocolo `"0.1"` e **não consegue** alcançar
/// `Ready` contra este core (débito técnico #4), o que é uma limitação do
/// plugin, não do harness.
#[test]
fn emulator_runs_the_real_subscription_until_a_plugin_reaches_ready() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("uptime-kuma-ready");

    let observed = observe_plugin_state(
        "uptime-kuma alcança Ready",
        fixture.spawn_config("uptime-kuma"),
    );

    assert_eq!(
        observed,
        PluginState::Ready,
        "esperava que uptime-kuma alcançasse Ready pelo Emulator (handshake 0.2 + \
         required_config resolvido pela fixture), obteve {observed:?}"
    );
}

/// **T004 (gate da feature 003, redefinido — cenário `git-local`)** — o mesmo
/// mecanismo do teste acima, agora contra a fixture determinística de
/// `git-local` (T003): repositório git real, `scan_root` apontado para ele.
///
/// # Por que o estado esperado aqui não é `PluginState::Ready`
///
/// `plugins/git-local/main.py` responde `protocol_version: "0.1"`
/// (`PROTOCOL_VERSION = "0.1"`) e este core fala `"0.2"`
/// (`plugin_worker::CORE_PROTOCOL_VERSION`). Sob o regime `MAJOR == 0` de
/// `ProtocolVersion::is_compatible_with` (D7 da feature 001), MINOR diferente
/// é **incompatível** — logo `git-local` termina, deterministicamente, em
/// `Unavailable{VersionIncompatible}`, nunca em `Ready`. Isso é o débito
/// técnico #4 já registrado em `AGENTS.md` ("protocolo v0.2 ... quebra
/// `git-local` de propósito"): a migração do plugin para v0.2 é
/// deliberadamente Fora de Escopo desta feature.
///
/// O teste continua valendo a pena porque o caminho percorrido até o veredito
/// é o mesmo do cenário `Ready` — `Subscription` real, processo Python real,
/// `handshake/hello` real, resposta real decodificada — e porque **congela**
/// o débito #4 como comportamento observado e automatizado: no dia em que
/// `git-local` migrar para v0.2, este teste falha e obriga a atualizá-lo, em
/// vez de o débito seguir invisível.
///
/// **T005 (dropada como redundante, decisão do arquiteto de 2026-09-01)**: o
/// cenário "git-local alcança `Ready`" que T005 previa é estruturalmente
/// impossível enquanto o débito #4 existir (achado N4 de `research.md` D1), e
/// o mecanismo que ele provaria — o `Emulator` levando um plugin real a
/// `Ready` — já está provado por
/// [`emulator_runs_the_real_subscription_until_a_plugin_reaches_ready`] e por
/// [`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`]. Este teste é
/// o que resta de T005: `git-local` percorrendo o mecanismo real até seu
/// estado terminal de verdade.
#[test]
fn emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("git-local-terminal");

    assert!(
        fixture.scan_root.join("exemplo").join(".git").is_dir(),
        "a fixture deveria conter um repositório git real"
    );

    match observe_plugin_state(
        "git-local alcança estado terminal",
        fixture.spawn_config("git-local"),
    ) {
        PluginState::Unavailable {
            reason: UnavailableReason::VersionIncompatible,
            detail,
        } => {
            // O detalhe é montado por `update.rs` a partir da versão que o
            // processo do plugin *de fato* respondeu — se o handshake não
            // tivesse acontecido de verdade, não haveria "0.1" aqui.
            assert!(
                detail.contains("0.1") && detail.contains("0.2"),
                "o detalhe deveria nomear as duas versões do handshake real, obteve: {detail:?}"
            );
        },
        other => panic!(
            "esperava Unavailable{{VersionIncompatible}} (git-local fala 0.1, core fala 0.2 — \
             débito técnico #4), obteve {other:?}"
        ),
    }
}

/// **T006 (US1) — `uptime-kuma` alcança `Ready` *com dados de verdade***.
///
/// O gate T004 prova que a conexão chega a `Ready`; para isso basta o
/// handshake e o `required_config` resolvido — o plugin nem precisa conseguir
/// falar com nada (lá o `base_url` aponta para uma porta fechada). Este
/// cenário vai além e fecha o ciclo completo de dados:
///
/// 1. um duplo HTTP determinístico de `/metrics` ([`MetricsFixtureServer`],
///    `research.md` D2) sobe numa porta efêmera de `127.0.0.1`;
/// 2. a fixture aponta o `base_url` do plugin para ele e guarda a API key
///    sintética em `secrets.toml` — o core injeta os dois no spawn pelo
///    caminho de produção (`FAROL_PLUGIN_UPTIME_KUMA_*`);
/// 3. a `PollerThread` do plugin faz a requisição HTTP **real** com Basic Auth
///    e parseia o corpo Prometheus **real** ([`METRICS_FIXTURE_BODY`]);
/// 4. o core, ao ficar `Ready`, dispara `widget/get` de verdade e decodifica
///    a resposta em `MonitorStatusItem`s;
/// 5. `view.rs` renderiza o widget `monitor-status-grid`, e é **na tela** que
///    a asserção principal acontece (`Selector`/`Instruction::Expect`), não só
///    no modelo.
///
/// # Por que este cenário exercita o segundo bug histórico
///
/// O timer de refresh (`update.rs::subscription`, ramo
/// `if slot.connection.state == PluginState::Ready`) só é **construído**
/// quando algum plugin de fato chega a `Ready` — foi exatamente por isso que o
/// segundo panic de closure capturante da feature 002 passou por todo
/// `cargo test` e por várias execuções manuais antes de aparecer
/// (`AGENTS.md` § Armadilha, ponto 2). Aqui esse ramo é construído de verdade,
/// e **repetidamente**: `Emulator::update` chama `resubscribe` a cada
/// mensagem, então cada ciclo abaixo remonta `Farol::subscription()` com uma
/// conexão `Ready` no estado.
///
/// # Por que o cenário injeta `Message::RefreshTick`
///
/// O plugin declara `suggested_refresh_interval_ms = 30000`, então o timer
/// real só produziria um segundo `widget/get` 30s depois do primeiro — no
/// limite exato de [`STATE_TIMEOUT`]. E o primeiro `widget/get` (disparado
/// pelo core assim que fica `Ready`) corre contra a primeira volta da
/// `PollerThread` do plugin: se o `widget/get` chega antes de a thread ter
/// concluído a requisição HTTP, o plugin responde, corretamente,
/// `-32006 metrics_unreachable` ("aguardando primeira leitura"). Esperar 30s
/// pelo tick seguinte tornaria o teste lento e ainda assim frágil.
///
/// Injetar `Message::RefreshTick` pelo `update()` real resolve isso **sem
/// simular nada**: é literalmente a mesma mensagem que o timer de produção
/// emite, processada pelo mesmo `handle_refresh_tick`, provocando o mesmo
/// `widget/get` pelo mesmo canal de worker. O que o teste substitui é só a
/// *cadência* do relógio — não o mecanismo.
#[test]
fn uptime_kuma_reaches_ready_and_populates_the_monitor_grid() {
    let _guard = e2e_guard();

    let metrics = MetricsFixtureServer::start();
    let fixture =
        HarnessFixture::with_uptime_kuma_base_url("uptime-kuma-widget", &metrics.base_url);

    let mut harness = start_scenario(
        "uptime-kuma popula o monitor-status-grid",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();

    // A conexão saiu de Starting/Handshaking — e chegou a `Ready`, não a um
    // `Unavailable`: só `view_ready` renderiza esta linha.
    assert!(
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        "uptime-kuma deveria estar Ready antes de qualquer ciclo de widget/get"
    );

    // Espera o primeiro ciclo de dados chegar à tela, cutucando o refresh na
    // cadência do teste (ver docstring) em vez de esperar o timer de 30s.
    let expected = expected_monitors();
    let deadline = Instant::now() + STATE_TIMEOUT;
    loop {
        harness.budget.check("espera pelo primeiro ciclo de widget/get com dados");

        if expected
            .iter()
            .all(|monitor| harness.screen_shows(&monitor.name))
        {
            break;
        }

        if Instant::now() >= deadline {
            harness.fail_with_observed_state(format!(
                "o widget monitor-status-grid não foi populado em {STATE_TIMEOUT:?} (requisições \
                 autenticadas servidas pela fixture: {}, não autenticadas: {})",
                metrics.authorized_requests(),
                metrics.unauthorized_requests()
            ));
        }

        harness.dispatch(Message::RefreshTick {
            plugin_name: "uptime-kuma".to_string(),
        });
        std::thread::sleep(Duration::from_millis(100));
    }

    // A tela mostra os dados *derivados*, não só os nomes: status mapeado e
    // tempo de resposta formatado, incluindo o "—" do sentinela `-1`.
    for text in ["Monitor", "Status", "Tempo de resposta", "up", "down", "42 ms", "7 ms", "—"] {
        assert!(
            harness.screen_shows(text),
            "o widget monitor-status-grid deveria renderizar {text:?}"
        );
    }
    assert!(
        !harness.screen_shows("Nenhum monitor cadastrado nesta instância."),
        "com três monitores servidos, a tela não pode mostrar o estado vazio"
    );

    let app = harness.finish();

    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("slot de uptime-kuma")
        .connection;

    assert_eq!(connection.state, PluginState::Ready);
    assert_eq!(
        connection.monitor_widget.monitors, expected,
        "os monitores decodificados deveriam bater exatamente com o corpo servido pela fixture — \
         incluindo o sentinela -1 de farol-container virando response_time_ms: None (commit \
         2608c03 / research.md D3)"
    );
    assert_eq!(
        connection.monitor_widget.last_error, None,
        "um ciclo de widget/get bem-sucedido MUST limpar o erro pontual anterior"
    );

    assert!(
        metrics.authorized_requests() >= 1,
        "a fixture de /metrics deveria ter servido ao menos uma requisição autenticada — o plugin \
         só consegue autenticar se o core injetou a API key pelo caminho de produção"
    );
    assert_eq!(
        metrics.unauthorized_requests(),
        0,
        "nenhuma requisição deveria chegar sem o Basic Auth correto"
    );

    drop(app);
    assert_no_lingering_children();
}
