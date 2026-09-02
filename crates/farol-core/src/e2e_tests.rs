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
use iced_test::instruction::{
    Expectation, Interaction, Keyboard, Mouse, Target as InstructionTarget,
};
use iced_test::program::Program;
use iced_test::selector::{Candidate, Target as SelectorTarget};
use iced_test::{Instruction, Selector};

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
        fs::write(
            repo.join("README.md"),
            "fixture determinística do harness\n",
        )
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
        assert_eq!(
            head.len(),
            40,
            "HEAD da fixture deveria ser um SHA-1: {head:?}"
        );

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

    /// Fixture **sem** `config.toml`/`secrets.toml` de `uptime-kuma` (T036/T037, Cenários 1/2 de
    /// `quickstart.md` — "primeira execução", `rm -f .../config.toml .../secrets.toml` do
    /// quickstart). Ao contrário de `new`/`with_uptime_kuma_base_url`, este construtor
    /// propositalmente não grava nenhum valor de `required_config` para `uptime-kuma`: é essa
    /// ausência que faz o handshake resultar em `all_required_config_present == false` (T019/D8) e
    /// a conexão cair em `PluginState::Unavailable{NotConfigured}` — a tela de setup, não o widget.
    ///
    /// `git-local` não é spawnado por nenhum cenário que usa este construtor, mas `scan_root`
    /// continua sendo criado (vazio, sem repositório) só para manter o mesmo formato de
    /// `HarnessFixture` que `Drop` e `spawn_config` esperam.
    fn uptime_kuma_unconfigured(label: &str) -> Self {
        let base = std::env::temp_dir().join(format!("farol-e2e-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);

        let scan_root = base.join("repos");
        fs::create_dir_all(&scan_root)
            .expect("criar scan_root da fixture (não usado por uptime-kuma)");

        // Mesma observação de hermetismo de `with_uptime_kuma_base_url`: `set_var` é seguro aqui
        // porque `E2E_LOCK` serializa os testes deste módulo.
        std::env::set_var("XDG_CONFIG_HOME", base.join("xdg"));

        Self { base, scan_root }
    }

    /// `PluginSpawnConfig` de um plugin de referência apontando para o
    /// `main.py` real do repositório, por caminho **absoluto** — mesmo
    /// comando/args de `plugin_worker::known_plugins()`, só sem a dependência
    /// de `cwd`.
    fn spawn_config(&self, plugin_name: &str) -> PluginSpawnConfig {
        let main_py = repo_root()
            .join("plugins")
            .join(plugin_name)
            .join("main.py");
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

/// Corpo de `/metrics` de uma instância Uptime Kuma real, recém-instalada, sem nenhum monitor
/// cadastrado ainda (T039, Cenário 6 de `quickstart.md`) — só os comentários `# HELP`/`# TYPE` que o
/// exportador sempre emite, nenhuma amostra `monitor_status{...}`/`monitor_response_time{...}`
/// (o Uptime Kuma só gera uma amostra por monitor configurado; zero monitores ⟹ zero amostras).
/// Confirmado por inspeção direta de `plugins/uptime-kuma/metrics_parser.py::parse_metrics` — ver
/// a docstring do teste que usa esta constante para o porquê disso ser um corpo válido "sem
/// monitores" e não um corpo malformado.
const EMPTY_METRICS_FIXTURE_BODY: &str = concat!(
    "# HELP monitor_cert_days_remaining Monitor Certificate Days Remaining\n",
    "# TYPE monitor_cert_days_remaining gauge\n",
    "# HELP monitor_response_time Monitor Response Time (ms)\n",
    "# TYPE monitor_response_time gauge\n",
    "# HELP monitor_status Monitor Status\n",
    "# TYPE monitor_status gauge\n",
);

/// Corpo de resposta de um servidor HTTP qualquer que **não** é um Uptime Kuma (T041, Cenário 5 de
/// `quickstart.md`) — nenhuma linha `monitor_status{...}` reconhecível, então
/// `metrics_parser.py::parse_metrics` MUST levantar `MetricsParseError` (`-32007
/// metrics_parse_error`) para este corpo, ao contrário de [`EMPTY_METRICS_FIXTURE_BODY`] acima
/// (que documenta um gap real do parser, não uma resposta genuinamente inválida).
const NON_METRICS_FIXTURE_BODY: &str = "<html><body><h1>404 Not Found</h1></body></html>\n";

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
    /// T040 (Cenário 4 de `quickstart.md`): `false` faz a fixture aceitar a conexão TCP e derrubá-la
    /// imediatamente sem responder nada — do ponto de vista do cliente HTTP do plugin
    /// (`urllib.request` em `metrics_client.py`), isso é `http.client.RemoteDisconnected`
    /// (subclasse de `ConnectionResetError`/`OSError`), mapeado para `MetricsUnreachableError` —
    /// equivalente a "a instância Uptime Kuma ficou inacessível/o serviço parou", sem precisar
    /// reiniciar o Farol nem reatribuir a porta efêmera desta fixture. `true` (default) serve
    /// [`MetricsFixtureServer::body`] normalmente.
    available: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MetricsFixtureServer {
    /// Serve [`METRICS_FIXTURE_BODY`] (três monitores conhecidos, T006).
    fn start() -> Self {
        Self::start_with_body(METRICS_FIXTURE_BODY)
    }

    /// Serve `body` para toda requisição autenticada — usado por T039/T041 para simular,
    /// respectivamente, uma instância sem monitores e uma resposta que não é um Uptime Kuma.
    fn start_with_body(body: &'static str) -> Self {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("bind da fixture de /metrics em 127.0.0.1:0");
        let port = listener
            .local_addr()
            .expect("porta efêmera da fixture")
            .port();
        listener
            .set_nonblocking(true)
            .expect("listener não-bloqueante (para o laço poder observar o shutdown)");

        let authorized = Arc::new(AtomicUsize::new(0));
        let unauthorized = Arc::new(AtomicUsize::new(0));
        let available = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread = {
            let authorized = Arc::clone(&authorized);
            let unauthorized = Arc::clone(&unauthorized);
            let available = Arc::clone(&available);
            let shutdown = Arc::clone(&shutdown);

            std::thread::spawn(move || {
                accept_until_shutdown(
                    &listener,
                    body,
                    &authorized,
                    &unauthorized,
                    &available,
                    &shutdown,
                );
            })
        };

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            authorized,
            unauthorized,
            available,
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

    /// Alterna a fixture entre "disponível" (serve `body` normalmente) e "indisponível" (T040 —
    /// ver a docstring do campo `available`).
    fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Relaxed);
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
    body: &str,
    authorized: &AtomicUsize,
    unauthorized: &AtomicUsize,
    available: &AtomicBool,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if available.load(Ordering::Relaxed) {
                    serve_metrics_connection(stream, body, authorized, unauthorized);
                } else {
                    // T040: "instância inacessível" — o SO já completou o handshake TCP antes de
                    // chegarmos aqui (é por isso que não basta parar de `accept()`), então
                    // derrubamos a conexão sem responder nada, em vez de servir `body`. Do lado do
                    // plugin (`urllib.request`), isso vira `http.client.RemoteDisconnected`
                    // (`ConnectionResetError`/`OSError`) — `metrics_client.py` mapeia para
                    // `MetricsUnreachableError`, o mesmo caminho de "porta fechada"/"serviço
                    // parado" que `quickstart.md` Cenário 4 descreve.
                    drop(stream);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
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
    body: &str,
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
            }
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
        http_response(200, "text/plain; version=0.0.4; charset=utf-8", body)
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
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
/// `Selector` (`iced_test`) que casa só um `text_input` — nunca um `text()`
/// estático vizinho — cujo conteúdo atual (valor digitado, ou o placeholder
/// enquanto vazio, mesma regra de `operation::TextInput::text`) é exatamente
/// `self.0`. Ver a docstring de `Scenario::setup_field_point` (T036) para o
/// porquê deste tipo existir: `view_setup_form` (view.rs) renderiza um
/// rótulo estático com o MESMO texto do placeholder logo acima de cada
/// campo, e o `Selector` embutido de `iced_test` para `&str` não distingue
/// os dois — sempre acha o rótulo primeiro.
struct TextInputByText<'a>(&'a str);

impl Selector for TextInputByText<'_> {
    type Output = SelectorTarget;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<Self::Output> {
        match candidate {
            Candidate::TextInput { state, .. } if state.text() == self.0 => {
                Some(SelectorTarget::from(candidate))
            }
            _ => None,
        }
    }

    fn description(&self) -> String {
        format!("text_input com texto == {:?}", self.0)
    }
}

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
    /// Executa uma `Instruction` (interação OU expectativa) e devolve `true`
    /// ⟺ ela reportou sucesso (`Event::Ready`).
    ///
    /// `Emulator::run` responde a QUALQUER `Instruction` com exatamente um
    /// `Event::Ready` (sucesso) ou `Event::Failed` (falha), então este laço
    /// sempre termina. Qualquer `Event::Action` que chegue antes disso (ex.:
    /// uma mensagem recém-produzida pela `Subscription` do worker) é
    /// aplicado no caminho — é aqui que o app de fato progride entre duas
    /// instruções. Extraído de `screen_shows` (T007) para também servir
    /// `click_text`/`click_point`/`type_text` (T036) — mesmo mecanismo de
    /// bombeamento de eventos, só a `Instruction` concreta muda.
    fn run_instruction(&mut self, instruction: Instruction) -> bool {
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
                }
                emulator::Event::Ready => return true,
                emulator::Event::Failed(_) => return false,
            }
        }
    }

    /// `true` ⟺ a tela atual do `Emulator` contém um widget de texto cujo
    /// conteúdo é exatamente `text`.
    fn screen_shows(&mut self, text: &str) -> bool {
        self.run_instruction(Instruction::Expect(Expectation::Text(text.to_string())))
    }

    /// Clica no primeiro widget cujo texto seja exatamente `text` — mesma
    /// resolução de alvo que `Emulator::run` usa para `Target::Text` (T036).
    /// Suficiente para o botão "Confirmar" da tela de setup, único widget da
    /// tela com esse texto. **Não use para os campos de texto** — ver a
    /// docstring de `setup_field_point` para o porquê.
    fn click_text(&mut self, text: &str) -> bool {
        self.run_instruction(Instruction::Interact(Interaction::Mouse(Mouse::Click {
            button: iced::mouse::Button::Left,
            target: Some(InstructionTarget::Text(text.to_string())),
        })))
    }

    /// Clica num ponto absoluto da viewport — usado por `click_setup_field`
    /// (T036) para alcançar um `text_input` cujo texto colide com o rótulo
    /// estático vizinho (ver `setup_field_point`).
    fn click_point(&mut self, point: iced::Point) -> bool {
        self.run_instruction(Instruction::Interact(Interaction::Mouse(Mouse::Click {
            button: iced::mouse::Button::Left,
            target: Some(InstructionTarget::Point(point)),
        })))
    }

    /// Digita `text`, tecla a tecla, no widget atualmente focado. Um
    /// `Interaction::Keyboard` não resolve nenhum alvo — só tem efeito
    /// depois de um `click_setup_field`/`click_text` bem-sucedido ter focado
    /// o campo certo (T036).
    fn type_text(&mut self, text: &str) -> bool {
        self.run_instruction(Instruction::Interact(Interaction::Keyboard(
            Keyboard::Typewrite(text.to_string()),
        )))
    }

    /// Centro visível, em coordenadas de tela, do `text_input` cujo texto
    /// atual (valor digitado, ou o placeholder — `item.description` —
    /// enquanto vazio) é exatamente `description` (T036).
    ///
    /// # Por que não basta `click_text(description)`
    ///
    /// `view_setup_form` (view.rs) renderiza, para cada item de
    /// `required_config`, um `text(item.description.clone())` **estático**
    /// imediatamente acima do `text_input` cujo placeholder é esse MESMO
    /// texto. O `Selector` embutido de `iced_test` para `&str`
    /// (`Target::Text`) para na primeira travessia cujo conteúdo bate — e a
    /// travessia visita o rótulo estático antes do campo
    /// (`column![text(...), field]`, filhos na ordem declarada) — então
    /// `click_text(description)` sempre acerta o rótulo (não focável), nunca
    /// o campo. `TextInputByText` (abaixo) restringe a busca só a
    /// `Candidate::TextInput`, ignorando o rótulo; como `Emulator` não expõe
    /// um `find`/`click` genérico por `Selector` arbitrário (só o mecanismo
    /// fixo de `Target::Text`/`Target::Point` via `Instruction`), a busca é
    /// feita contra uma `iced_test::Simulator` descartável, construída a
    /// partir da MESMA árvore de widgets que o `Emulator` renderiza
    /// (`Emulator::view`) — o layout resultante é determinístico e idêntico
    /// ao que o `Emulator` real usaria no mesmo instante — e só o `Point`
    /// (dado já "achatado", sem nenhum estado emprestado do `Simulator`) é
    /// reaproveitado contra o `Emulator` de verdade via `click_point`.
    fn setup_field_point(&self, description: &str) -> iced::Point {
        let element = self
            .emulator
            .as_ref()
            .expect("o Emulator do cenário só é consumido no encerramento")
            .view(&self.program);

        let mut simulator = iced_test::Simulator::new(element);
        let target = simulator
            .find(TextInputByText(description))
            .unwrap_or_else(|err| {
                panic!("campo de setup {description:?} não encontrado na tela: {err:?}")
            });

        target
            .visible_bounds()
            .unwrap_or_else(|| panic!("campo de setup {description:?} não está visível na tela"))
            .center()
    }

    /// Clica no `text_input` de um campo de setup pela sua descrição (T036)
    /// — ver `setup_field_point` para o porquê de não usar `click_text`.
    fn click_setup_field(&mut self, description: &str) {
        let point = self.setup_field_point(description);
        assert!(
            self.click_point(point),
            "clique no campo de setup {description:?} deveria ter sucesso"
        );
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
            }
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
// T038/T040/T041/T042 — esperas de estado além do handshake inicial
// ---------------------------------------------------------------------------

/// Bombeia o cenário — disparando um `Message::RefreshTick` a cada iteração,
/// já que os erros pontuais/recuperação de T038/T040/T041 só se manifestam
/// no PRÓXIMO ciclo de `widget/get` — até `condition` ser verdadeira, ou
/// falha nomeando o `PluginState` real observado (mesmo padrão de
/// `Scenario::settle`, generalizado para uma condição arbitrária em vez de
/// só "saiu de Starting/Handshaking").
///
/// `timeout` é um parâmetro (em vez de sempre [`STATE_TIMEOUT`]) porque
/// T040 precisa esperar o ciclo de polling **do próprio plugin** — fixo em
/// 30s (`plugins/uptime-kuma/poller.py::DEFAULT_POLL_INTERVAL_MS`,
/// hardcoded, fora do escopo desta subtarefa alterar `plugins/`) — que corre
/// independente de quantas vezes o core pede um `widget/get`: um
/// `RefreshTick` só lê o cache do poller, nunca força uma nova leitura de
/// rede. Um teto de 30s ficaria justo demais contra esse relógio fixo do
/// plugin; os cenários que não dependem dele (T038/T041, cujo erro já está
/// pronto desde a primeira leitura do poller) continuam usando
/// [`STATE_TIMEOUT`] normalmente.
fn wait_until<P>(
    harness: &mut Scenario<P>,
    stage: &str,
    mut condition: impl FnMut(&mut Scenario<P>) -> bool,
    timeout: Duration,
) where
    P: Program<State = Farol, Message = Message> + 'static,
{
    let deadline = Instant::now() + timeout;
    loop {
        harness.budget.check(stage);
        if condition(harness) {
            return;
        }
        if Instant::now() >= deadline {
            harness.fail_with_observed_state(format!("{stage} não aconteceu em {timeout:?}"));
        }
        harness.dispatch(Message::RefreshTick {
            plugin_name: harness.plugin_name.clone(),
        });
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Como [`wait_until`], mas **sem** disparar `Message::RefreshTick` a cada
/// iteração — só bombeia os `Event::Action` já pendentes na fila do
/// `Emulator` (via `Scenario::screen_shows`, chamada dentro de `condition`).
///
/// Usado por T042 (kill -9): o worker observa `child.wait()`
/// concorrentemente (`plugin_worker.rs`), então a transição para
/// `Unavailable{Crashed}` chega sozinha, de forma assíncrona — injetar
/// `RefreshTick`s aqui só correria contra essa detecção (uma requisição de
/// `widget/get` mandada para um processo que acabou de morrer poderia, em
/// tese, terminar decodificada como uma falha de leitura comum antes do
/// `child.wait()` resolver, mascarando o caminho que este cenário
/// especificamente quer provar).
fn wait_until_passive<P>(
    harness: &mut Scenario<P>,
    stage: &str,
    mut condition: impl FnMut(&mut Scenario<P>) -> bool,
    timeout: Duration,
) where
    P: Program<State = Farol, Message = Message> + 'static,
{
    let deadline = Instant::now() + timeout;
    loop {
        harness.budget.check(stage);
        if condition(harness) {
            return;
        }
        if Instant::now() >= deadline {
            harness.fail_with_observed_state(format!("{stage} não aconteceu em {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------
// T042 — sinalização Unix contra o processo real do plugin
// ---------------------------------------------------------------------------

/// Localiza o PID do processo `uptime-kuma` real spawnado pelo cenário em
/// andamento (T042), filtrando [`live_child_processes`] (já usado por
/// [`assert_no_lingering_children`]) pela `cmdline`. Como [`E2E_LOCK`]
/// serializa os cenários deste módulo e cada um spawna no máximo um plugin,
/// encontrar mais ou menos de um candidato aqui indicaria um bug de
/// isolamento entre testes, não uma condição normal deste cenário — falha
/// alto e claro em vez de escolher um dos candidatos arbitrariamente.
fn find_uptime_kuma_pid() -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let matches: Vec<i32> = live_child_processes()
            .into_iter()
            .filter(|(_, cmdline)| cmdline.contains("uptime-kuma/main.py"))
            .map(|(pid, _)| pid)
            .collect();
        match matches.as_slice() {
            [pid] => return *pid,
            [] if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => panic!(
                "esperava exatamente 1 processo uptime-kuma vivo (filho direto deste processo de \
                 teste), encontrou {other:?}"
            ),
        }
    }
}

/// Envia um sinal Unix a um PID via o utilitário `kill` do sistema — sem
/// dependência nova no `Cargo.toml` só para isto (mesmo espírito de
/// `base64_encode` acima). Usado por T042 para reproduzir literalmente
/// `kill -9`/`kill -STOP` (`quickstart.md` Cenário 7) contra o processo real
/// de um plugin já `Ready`.
fn send_signal(pid: i32, signal: &str) {
    let status = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .unwrap_or_else(|err| panic!("falha ao executar `kill {signal} {pid}`: {err}"));
    assert!(status.success(), "`kill {signal} {pid}` falhou: {status}");
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
/// **Por que `uptime-kuma` e não `git-local`**: histórico — até o commit
/// `9d2fe77` ("fix: migra git-local para protocolo 0.2..."), `git-local`
/// falava protocolo `"0.1"` e não conseguia alcançar `Ready` contra este
/// core (débito técnico #4, issue #4, resolvida naquele commit). `git-local`
/// hoje também alcança `Ready` (ver
/// [`emulator_takes_git_local_through_a_real_handshake_to_ready`]) — este
/// teste continua usando `uptime-kuma` porque
/// [`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`] (T006) já
/// aprofunda o mesmo caminho até um ciclo de dados real de `widget/get`, o
/// que `git-local` (sem fixture de `/metrics`) não exercitaria aqui.
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
/// # Nota histórica (débito técnico #4/T050, resolvido em `9d2fe77`)
///
/// Até o commit `9d2fe77` ("fix: migra git-local para protocolo 0.2 e
/// distingue remote sem tracking"), `plugins/git-local/main.py` respondia
/// `protocol_version: "0.1"` contra um core que fala `"0.2"`
/// (`plugin_worker::CORE_PROTOCOL_VERSION`) — sob o regime `MAJOR == 0` de
/// `ProtocolVersion::is_compatible_with` (D7 da feature 001), isso é
/// **incompatível**, e este teste (então chamado
/// `emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state`)
/// afirmava `Unavailable{VersionIncompatible}` como o "estado terminal" do
/// handshake — deliberadamente, para **congelar** o débito técnico #4 como
/// comportamento observado e automatizado (issue #4 do tracker do projeto).
///
/// `9d2fe77` fechou a issue #4: `git-local` agora declara `protocol_version:
/// "0.2"`, `capabilities.capabilities: [{"kind": "exec"}]` e
/// `required_config: []` — alcançando `Ready` contra este core, exatamente
/// como esta função passou a afirmar (renomeada de acordo). O caminho
/// percorrido até o veredito continua sendo o mesmo do cenário `Ready` de
/// `uptime-kuma` acima — `Subscription` real, processo Python real,
/// `handshake/hello` real, resposta real decodificada — só que agora contra
/// o segundo plugin de referência do projeto, provando que o mecanismo do
/// harness não é específico de `uptime-kuma`.
///
/// **T005 (dropada como redundante, decisão do arquiteto de 2026-09-01;
/// nota mantida por precisão histórica)**: quando este teste ainda afirmava
/// `VersionIncompatible`, o cenário "`git-local` alcança `Ready`" que T005
/// prevé era estruturalmente impossível (achado N4 de `research.md` D1) — a
/// migração de `9d2fe77` tornou T005 alcançável depois de tudo, e este
/// mesmo teste (atualizado) passou a cobri-lo.
#[test]
fn emulator_takes_git_local_through_a_real_handshake_to_ready() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("git-local-terminal");

    assert!(
        fixture.scan_root.join("exemplo").join(".git").is_dir(),
        "a fixture deveria conter um repositório git real"
    );

    let observed = observe_plugin_state(
        "git-local alcança Ready (protocolo 0.2, débito técnico #4 resolvido em 9d2fe77)",
        fixture.spawn_config("git-local"),
    );

    assert_eq!(
        observed,
        PluginState::Ready,
        "esperava que git-local alcançasse Ready pelo Emulator (protocolo 0.2 desde 9d2fe77), \
         obteve {observed:?}"
    );
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
        harness
            .budget
            .check("espera pelo primeiro ciclo de widget/get com dados");

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
    for text in [
        "Monitor",
        "Status",
        "Tempo de resposta",
        "up",
        "down",
        "42 ms",
        "7 ms",
        "—",
    ] {
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

/// **T036 [US1]** — Cenário 1 de `quickstart.md`: primeira execução, sem
/// `config.toml`/`secrets.toml` para `uptime-kuma`. A tela de setup aparece
/// (`Unavailable{NotConfigured}`); os dois campos são preenchidos e
/// confirmados **pela UI de verdade** — clique/digitação via
/// `Instruction`/`Selector` do `iced_test`, a mesma DSL de interação que o
/// `Emulator` já usa nos demais cenários deste módulo, não uma escrita
/// direta em `config.toml`/`secrets.toml` — e o widget populado aparece em
/// seguida, sem editar nenhum arquivo manualmente. Por fim, reabre o Farol
/// (mesmo `XDG_CONFIG_HOME`, um `Scenario` novo) e confirma que a
/// configuração persiste: a tela de setup não aparece de novo.
///
/// Prova, no mesmo cenário, o mecanismo inteiro de D8 ponta a ponta: o
/// widget/formulário (T035), a persistência em `config.toml`/`secrets.toml`
/// (T016/T017/T032) e a reconexão do worker via `setup_attempt`
/// (`plugin_worker::subscription`) — um NOVO processo spawnado que lê as
/// variáveis de ambiente recém-persistidas, provado pela capacidade
/// `network` exibida citar o `base_url` digitado, não um valor antigo.
#[test]
fn setup_form_filled_via_ui_reaches_ready_with_real_data_and_persists_across_restart() {
    let _guard = e2e_guard();

    let metrics = MetricsFixtureServer::start();
    let fixture = HarnessFixture::uptime_kuma_unconfigured("uptime-kuma-setup-ui");

    // --- primeira execução: tela de setup, preenchida via Selector/DSL de interação do iced_test ---
    let mut harness = start_scenario(
        "T036: tela de setup preenchida via UI leva a Ready com dados reais",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();

    for text in [
        "URL base da instância Uptime Kuma",
        "API Key de métricas do Uptime Kuma",
        "Confirmar",
    ] {
        assert!(
            harness.screen_shows(text),
            "tela de setup deveria mostrar {text:?}"
        );
    }
    assert!(
        !harness.screen_shows("Monitor"),
        "sem configuração ainda, o cabeçalho do widget monitor-status-grid não deveria aparecer"
    );

    harness.click_setup_field("URL base da instância Uptime Kuma");
    assert!(
        harness.type_text(&metrics.base_url),
        "digitar a URL base deveria ter efeito (campo deveria estar focado)"
    );

    harness.click_setup_field("API Key de métricas do Uptime Kuma");
    assert!(
        harness.type_text(UPTIME_KUMA_FIXTURE_API_KEY),
        "digitar a API Key deveria ter efeito (campo deveria estar focado)"
    );

    assert!(
        harness.click_text("Confirmar"),
        "confirmar a tela de setup deveria ter efeito"
    );

    // Depois de confirmar, o core persiste config/secrets e reconecta o worker
    // (`setup_attempt` incrementado, T032) — a mesma `settle()` que já sabe esperar
    // Starting/Handshaking até um estado terminal funciona igual aqui, só que desta vez o
    // handshake real do NOVO processo (que já enxerga as variáveis recém-persistidas) leva a
    // Ready, não a Unavailable de novo.
    harness.settle();

    assert!(
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        "depois de confirmar a tela de setup, uptime-kuma deveria estar Ready"
    );

    // Widget populado com dados reais — mesmo mecanismo/corpo de T006 (cutucando o refresh em vez
    // de esperar os 30s reais do plugin).
    let expected = expected_monitors();
    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get pós-setup",
        |h| expected.iter().all(|monitor| h.screen_shows(&monitor.name)),
        STATE_TIMEOUT,
    );

    // Capacidade `network` exibida citando host/porta derivados do `base_url` recém-digitado
    // (T034) — prova que o NOVO processo leu a variável de ambiente recém-persistida, não um
    // valor antigo/vazio.
    let expected_capability_line = format!(
        "Capacidades declaradas: network({})",
        metrics.base_url.trim_start_matches("http://")
    );
    assert!(
        harness.screen_shows(&expected_capability_line),
        "capacidades declaradas deveriam citar a capacidade `network` recém-configurada, esperava \
         {expected_capability_line:?}"
    );

    let app = harness.finish();
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("slot de uptime-kuma")
        .connection;
    assert_eq!(connection.state, PluginState::Ready);
    assert_eq!(connection.monitor_widget.monitors, expected);
    drop(app);
    assert_no_lingering_children();

    // Persistência: `config.toml`/`secrets.toml` foram gravados pela própria UI, nunca editados à
    // mão (lidos aqui pelo mesmo `config_store`/`secrets_store` que o core usa em produção, contra
    // o mesmo `XDG_CONFIG_HOME` hermético desta fixture).
    let persisted_config = crate::config_store::load_plugin_config("uptime-kuma");
    assert_eq!(
        persisted_config.get("base_url").map(String::as_str),
        Some(metrics.base_url.as_str())
    );
    let persisted_secrets = crate::secrets_store::load_plugin_secrets("uptime-kuma");
    assert_eq!(
        persisted_secrets.get("api_key").map(String::as_str),
        Some(UPTIME_KUMA_FIXTURE_API_KEY)
    );

    // --- reabrir o Farol (mesmo XDG_CONFIG_HOME) não deve mostrar a tela de setup de novo ---
    let mut restarted = start_scenario(
        "T036: reabertura reaproveita a configuração persistida",
        fixture.spawn_config("uptime-kuma"),
    );
    restarted.settle();
    assert!(
        restarted.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        "reabrir o Farol com config/secrets já persistidos deveria ir direto a Ready, sem a tela \
         de setup"
    );
    assert!(
        !restarted.screen_shows("Confirmar"),
        "a tela de setup não deveria reaparecer numa reabertura com configuração já persistida"
    );
    let restarted_app = restarted.finish();
    assert_eq!(
        plugin_state(&restarted_app, "uptime-kuma"),
        PluginState::Ready
    );
    drop(restarted_app);
    assert_no_lingering_children();
}

/// **T037 [US1]** — Cenário 2 de `quickstart.md`: primeira execução, a tela
/// de setup aparece mas **não** é preenchida. Confirma
/// `PluginState::Unavailable{NotConfigured}` como caminho **primário**
/// (T019) — estável (um `RefreshTick` não tem efeito nenhum, já que o core
/// nunca chama `widget/get` neste estado) e visivelmente distinto da tela de
/// "0 monitores" (Ready com `items: []`, T039): a tela mostrada é sempre o
/// formulário de setup, nunca o grid nem sua mensagem de lista vazia.
///
/// A salvaguarda descrita em `error-model-delta.md` (uma chamada direta de
/// `widget/get` devolveria `error(-32005, not_configured)`, T028 do lado do
/// plugin) não é exercitada aqui: o próprio mecanismo que este teste prova
/// garante que o core nunca chega a enviar `widget/get` enquanto
/// `NotConfigured` — não há como provocar essa chamada pela API pública do
/// `Program`/`Message` sem contornar o core, que é exatamente o que a
/// salvaguarda protege contra (um plugin/cliente de protocolo diferente
/// deste core, não este cenário). Coberta do lado do plugin pela suíte
/// `pytest` de `plugins/uptime-kuma` (T046, fora do escopo desta subtarefa).
#[test]
fn setup_form_left_unfilled_stays_not_configured() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::uptime_kuma_unconfigured("uptime-kuma-not-configured");

    let mut harness = start_scenario(
        "T037: tela de setup não preenchida permanece NotConfigured",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();

    for text in [
        "URL base da instância Uptime Kuma",
        "API Key de métricas do Uptime Kuma",
        "Confirmar",
    ] {
        assert!(
            harness.screen_shows(text),
            "tela de setup deveria mostrar {text:?}"
        );
    }
    assert!(
        !harness.screen_shows("Nenhum monitor cadastrado nesta instância."),
        "NotConfigured não deveria nunca se parecer com \"0 monitores\" (Ready sem itens)"
    );
    assert!(
        !harness.screen_shows("Monitor"),
        "o cabeçalho do grid de monitores não deveria aparecer sem configuração"
    );

    // Um tick de refresh não deveria ter efeito nenhum enquanto NotConfigured (T019: o core nunca
    // chama widget/get neste estado) — a tela de setup continua exatamente igual depois.
    harness.dispatch(Message::RefreshTick {
        plugin_name: "uptime-kuma".to_string(),
    });
    assert!(
        harness.screen_shows("Confirmar"),
        "NotConfigured deveria ser estável — sem transição espontânea causada por um RefreshTick"
    );

    let app = harness.finish();
    match plugin_state(&app, "uptime-kuma") {
        PluginState::Unavailable {
            reason: UnavailableReason::NotConfigured,
            ..
        } => {}
        other => panic!("esperava Unavailable{{NotConfigured}}, obteve {other:?}"),
    }
    drop(app);
    assert_no_lingering_children();
}

/// **T038 [US1]** — Cenário 3 de `quickstart.md`: `required_config` presente
/// (config/secrets já persistidos), mas `base_url` aponta para um host/porta
/// inacessível — confirma `metrics_unreachable`, **distinto de
/// `not_configured`**: a distinção observável, do lado do core, é a
/// própria `PluginState` — `not_configured` nunca chega a `Ready` (T037),
/// enquanto um `base_url` inválido continua `Ready` o tempo todo, só
/// sinalizando o erro pontual daquele widget (FR-019, mesmo mecanismo
/// genérico de US2 herdado da feature 001).
///
/// Usa a fixture default (`HarnessFixture::new`,
/// [`UPTIME_KUMA_UNREACHABLE_BASE_URL`] = porta fechada em `127.0.0.1`) —
/// dispensa `MetricsFixtureServer`, já que nenhuma conexão chega a
/// completar.
#[test]
fn uptime_kuma_reports_metrics_unreachable_for_an_invalid_base_url_but_stays_ready() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("uptime-kuma-metrics-unreachable");

    let mut harness = start_scenario(
        "T038: base_url inválido gera metrics_unreachable, não not_configured",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        "required_config presente (mesmo com base_url inacessível) deveria levar a Ready, nunca a \
         NotConfigured"
    );

    let error_text =
        "Falha ao consultar monitores: falha ao consultar /metrics da instância Uptime Kuma configurada";
    wait_until(
        &mut harness,
        "erro pontual de metrics_unreachable",
        |h| h.screen_shows(error_text),
        STATE_TIMEOUT,
    );

    let app = harness.finish();
    assert_eq!(
        plugin_state(&app, "uptime-kuma"),
        PluginState::Ready,
        "um erro pontual de leitura NÃO muda PluginState — continua Ready, distinto de qualquer \
         Unavailable (incluindo NotConfigured)"
    );
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("slot de uptime-kuma")
        .connection;
    assert!(connection.monitor_widget.last_error.is_some());
    assert!(
        connection.monitor_widget.monitors.is_empty(),
        "nunca houve nenhuma leitura bem-sucedida nesta conexão"
    );
    drop(app);
    assert_no_lingering_children();
}

/// **T039 [US1] (achado nesta sessão — gap real do plugin, não corrigido:
/// `plugins/` está fora do escopo autorizado desta subtarefa)** — Cenário 6
/// de `quickstart.md`: instância Uptime Kuma real, acessível, mas recém
/// instalada e sem nenhum monitor cadastrado. O critério de aceite
/// documentado (`spec.md` Edge Case, `quickstart.md` Cenário 6, `tasks.md`
/// T039) é `widget/get` responder com sucesso e `items: []` — estado válido,
/// análogo ao diretório sem repositórios git da feature 001.
///
/// A implementação atual de `plugins/uptime-kuma/metrics_parser.py::parse_metrics`
/// (linhas 94-95) não distingue "zero monitores" de "resposta não
/// reconhecível": as duas condições produzem exatamente a mesma falha,
/// `MetricsParseError` (`found_monitor_status_line == False` sempre que não
/// há NENHUMA linha `monitor_status{...}` no corpo — que é justamente o que
/// uma instância sem monitores emite, já que o Uptime Kuma só gera uma
/// amostra Prometheus por monitor configurado). Confirmado interativamente
/// antes de escrever este teste:
///
/// ```text
/// $ python3 -c "from metrics_parser import parse_metrics, MetricsParseError; \
///     parse_metrics('# HELP monitor_status ...\n# TYPE monitor_status gauge\n')"
/// MetricsParseError: nenhuma linha monitor_status{...} encontrada no corpo de /metrics
/// ```
///
/// Ou seja: hoje, uma instância real sem monitores devolve `error(-32007,
/// metrics_parse_error)` pelo `widget/get`, não `items: []`. Fora do escopo
/// autorizado desta subtarefa corrigir (`plugins/` está off-limits) —
/// reportado como bloqueio, não corrigido por conta própria. Precisa virar
/// issue própria (constitution v1.0.0, Governance: débito técnico é issue
/// obrigatória) antes deste teste poder deixar de ser `#[ignore]`d — mesmo
/// padrão já usado por
/// `crates/farol-protocol/tests/schema_boundaries.rs::widget_monitor_status_item_response_time_ms_negative_value_is_a_known_protocol_gap`
/// (ver `AGENTS.md` § Testes).
///
/// Este teste fica `#[ignore]`d de propósito: exercita o critério de aceite
/// DOCUMENTADO (sucesso, `items: []`), então falha contra o código real como
/// está hoje — rodar com `cargo test --package farol-core e2e_tests --
/// --ignored` reproduz o gap sob demanda; deixá-lo habilitado por padrão
/// quebraria `cargo test` para todo mundo por um comportamento pré-existente
/// do plugin, não uma regressão desta subtarefa.
#[test]
#[ignore = "gap real em plugins/uptime-kuma/metrics_parser.py (zero monitores vira \
            metrics_parse_error, não items: []) — plugins/ está fora do escopo autorizado desta \
            subtarefa; precisa virar issue antes de habilitar"]
fn uptime_kuma_widget_reports_empty_items_when_instance_has_no_monitors() {
    let _guard = e2e_guard();
    let metrics = MetricsFixtureServer::start_with_body(EMPTY_METRICS_FIXTURE_BODY);
    let fixture =
        HarnessFixture::with_uptime_kuma_base_url("uptime-kuma-no-monitors", &metrics.base_url);

    let mut harness = start_scenario(
        "T039: instância acessível sem nenhum monitor cadastrado",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"));

    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get após a instância subir",
        |h| h.screen_shows("Nenhum monitor cadastrado nesta instância."),
        STATE_TIMEOUT,
    );

    let app = harness.finish();
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("slot de uptime-kuma")
        .connection;
    assert_eq!(connection.state, PluginState::Ready);
    assert_eq!(connection.monitor_widget.monitors, Vec::new());
    assert_eq!(connection.monitor_widget.last_error, None);
    drop(app);
    assert_no_lingering_children();
}

/// **T040 [US2]** — Cenário 4 de `quickstart.md`: com o widget já populado
/// (dados reais, mesmo mecanismo de T006), a instância Uptime Kuma fica
/// inacessível **sem reiniciar o Farol** (aqui: `MetricsFixtureServer`
/// passa a derrubar toda conexão nova, `set_available(false)` — equivalente
/// a "parar o serviço"/"porta fechada" do ponto de vista do cliente HTTP do
/// plugin). Confirma: (a) a janela permanece respondendo (o cenário inteiro
/// continua rodando dentro do `Emulator`), (b) o próximo ciclo sinaliza o
/// erro pontual **mantendo os últimos monitores conhecidos** na tela, sem
/// mudar `PluginState`, e (c) restaurar o acesso — sem reiniciar o Farol —
/// faz os dados reais voltarem no ciclo seguinte.
///
/// **Sobre a duração real deste teste**: `plugins/uptime-kuma/poller.py`
/// tem cadência de polling fixa em 30s
/// (`DEFAULT_POLL_INTERVAL_MS`, hardcoded, fora do escopo desta subtarefa
/// tocar `plugins/`) — nem o core nem este teste conseguem acelerá-la
/// (`Message::RefreshTick` só lê o cache do poller, nunca força uma
/// requisição HTTP nova). As duas esperas abaixo (queda e recuperação)
/// dependem cada uma de UM ciclo real do poller — por isso usam
/// `wait_until` com um teto próprio (45s, folga sobre os 30s do plugin) em
/// vez do [`STATE_TIMEOUT`] padrão de 30s, ainda dentro do
/// [`SCENARIO_TIMEOUT`] de 120s do cenário inteiro.
#[test]
fn uptime_kuma_recovers_after_instance_becomes_unreachable_without_restarting_farol() {
    const POLL_CYCLE_TIMEOUT: Duration = Duration::from_secs(45);

    let _guard = e2e_guard();
    let metrics = MetricsFixtureServer::start();
    let fixture =
        HarnessFixture::with_uptime_kuma_base_url("uptime-kuma-recovers", &metrics.base_url);

    let mut harness = start_scenario(
        "T040: Uptime Kuma fica inacessível e depois volta, sem reiniciar o Farol",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"));

    // (a) widget populado antes de derrubar a instância — mesmo padrão de T006.
    let expected = expected_monitors();
    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get bem-sucedido",
        |h| expected.iter().all(|monitor| h.screen_shows(&monitor.name)),
        STATE_TIMEOUT,
    );

    // (b) "parar o serviço Uptime Kuma" sem reiniciar o Farol.
    metrics.set_available(false);

    let error_text =
        "Falha ao consultar monitores: falha ao consultar /metrics da instância Uptime Kuma configurada";
    wait_until(
        &mut harness,
        "erro pontual após a instância ficar inacessível",
        |h| h.screen_shows(error_text),
        POLL_CYCLE_TIMEOUT,
    );
    for monitor in &expected {
        assert!(
            harness.screen_shows(&monitor.name),
            "os últimos monitores conhecidos deveriam continuar visíveis ao lado do erro pontual"
        );
    }
    assert!(
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        "a janela/conexão continua Ready — um erro pontual de leitura nunca vira Unavailable"
    );

    // (c) restaura o acesso — sem reiniciar o Farol nem o processo do plugin.
    metrics.set_available(true);
    wait_until(
        &mut harness,
        "dados reais voltam após restaurar o acesso",
        |h| !h.screen_shows(error_text),
        POLL_CYCLE_TIMEOUT,
    );

    let app = harness.finish();
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("slot de uptime-kuma")
        .connection;
    assert_eq!(connection.state, PluginState::Ready);
    assert_eq!(connection.monitor_widget.monitors, expected);
    assert_eq!(
        connection.monitor_widget.last_error, None,
        "um ciclo de widget/get bem-sucedido MUST limpar o erro pontual anterior"
    );
    drop(app);
    assert_no_lingering_children();
}

/// **T041 [US2]** — Cenário 5 de `quickstart.md`: `base_url` aponta para um
/// servidor HTTP que responde, mas não é um Uptime Kuma (`/metrics` não
/// reconhecível — [`NON_METRICS_FIXTURE_BODY`]). Confirma o mesmo
/// tratamento de erro pontual do Cenário 4/T040 (FR-016) — a conexão
/// continua `Ready`, sem crash do plugin.
#[test]
fn uptime_kuma_reports_metrics_parse_error_for_a_non_metrics_response_without_crashing() {
    let _guard = e2e_guard();
    let metrics = MetricsFixtureServer::start_with_body(NON_METRICS_FIXTURE_BODY);
    let fixture =
        HarnessFixture::with_uptime_kuma_base_url("uptime-kuma-parse-error", &metrics.base_url);

    let mut harness = start_scenario(
        "T041: resposta de /metrics não reconhecível como Uptime Kuma",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"));

    let error_text =
        "Falha ao consultar monitores: falha ao consultar /metrics da instância Uptime Kuma configurada";
    wait_until(
        &mut harness,
        "erro pontual de metrics_parse_error",
        |h| h.screen_shows(error_text),
        STATE_TIMEOUT,
    );

    let app = harness.finish();
    assert_eq!(
        plugin_state(&app, "uptime-kuma"),
        PluginState::Ready,
        "resposta não reconhecível é erro pontual (mesmo tratamento de metrics_unreachable) — não \
         derruba o plugin nem muda PluginState"
    );
    drop(app);
    assert_no_lingering_children();
}

/// **T042 [P] (parte 1/2 — `kill -9`)** — Cenário 7 de `quickstart.md`,
/// herdado sem modificação da feature 001 (FR-018), agora provado contra o
/// processo real de `uptime-kuma`: mata o processo já `Ready` com `SIGKILL`
/// e confirma `Unavailable{Crashed}` — `plugin_worker.rs` observa
/// `child.wait()` concorrentemente e emite o evento sozinho, sem precisar
/// de nenhuma requisição em voo (`wait_until_passive` — ver sua docstring
/// para o porquê de não injetar `RefreshTick` aqui).
#[test]
fn uptime_kuma_process_killed_becomes_crashed_without_taking_down_the_core() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("uptime-kuma-crash");

    let mut harness = start_scenario(
        "T042a: kill -9 no processo já Ready vira Unavailable{Crashed}",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"));

    let pid = find_uptime_kuma_pid();
    send_signal(pid, "-9");

    wait_until_passive(
        &mut harness,
        "saída de Ready após kill -9",
        |h| !h.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        STATE_TIMEOUT,
    );

    let app = harness.finish();
    match plugin_state(&app, "uptime-kuma") {
        PluginState::Unavailable {
            reason: UnavailableReason::Crashed,
            ..
        } => {}
        other => panic!("esperava Unavailable{{Crashed}} após kill -9, obteve {other:?}"),
    }
    drop(app);
    // A janela do Farol (o Emulator/Program real) continuou aberta e respondendo a instruções
    // durante todo o cenário acima — é isso que prova "o core não trava"/"nenhum crash do core",
    // não apenas o `PluginState` final.
    assert_no_lingering_children();
}

/// **T042 [P] (parte 2/2 — `kill -STOP`)** — mesmo Cenário 7, agora travando
/// o processo (`SIGSTOP`) em vez de matá-lo: um ciclo de refresh contra um
/// processo travado estoura `RPC_TIMEOUT_CONTROL` (5s,
/// `plugin_worker.rs`), convergindo para `Unavailable{Unresponsive}` — bem
/// dentro do [`STATE_TIMEOUT`] de 30s. Ao contrário de T042a, aqui
/// PRECISAMOS de um `Message::RefreshTick` explícito: o processo continua
/// vivo (só parado), então não há nenhum `child.wait()` para disparar a
/// transição sozinho — é a RPC que precisa estourar o timeout.
#[test]
fn uptime_kuma_process_frozen_becomes_unresponsive_without_taking_down_the_core() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("uptime-kuma-unresponsive");

    let mut harness = start_scenario(
        "T042b: kill -STOP no processo já Ready vira Unavailable{Unresponsive}",
        fixture.spawn_config("uptime-kuma"),
    );
    harness.settle();
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"));

    let pid = find_uptime_kuma_pid();
    send_signal(pid, "-STOP");

    harness.dispatch(Message::RefreshTick {
        plugin_name: "uptime-kuma".to_string(),
    });
    wait_until_passive(
        &mut harness,
        "saída de Ready após kill -STOP",
        |h| !h.screen_shows("Plugin: uptime-kuma (protocolo 0.2)"),
        STATE_TIMEOUT,
    );

    let app = harness.finish();
    match plugin_state(&app, "uptime-kuma") {
        PluginState::Unavailable {
            reason: UnavailableReason::Unresponsive,
            ..
        } => {}
        other => panic!("esperava Unavailable{{Unresponsive}} após kill -STOP, obteve {other:?}"),
    }
    drop(app);

    // Limpeza extra: `kill_on_drop` já deveria ter encerrado o processo travado ao consumir o
    // Emulator acima (SIGKILL termina até um processo parado, no Linux) — confirmado por
    // `assert_no_lingering_children()` logo abaixo; um `-CONT`+`-9` de defesa (caso o processo,
    // por algum motivo, ainda esteja vivo e parado) não tem custo nenhum.
    let _ = Command::new("kill")
        .arg("-CONT")
        .arg(pid.to_string())
        .status();
    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    assert_no_lingering_children();
}
