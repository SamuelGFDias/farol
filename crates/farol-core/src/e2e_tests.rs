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

        // Busca o perfil real definido para este plugin em `known_plugins()`
        let sandbox_profile = crate::plugin_worker::known_plugins()
            .into_iter()
            .find(|c| c.plugin_name == plugin_name)
            .unwrap_or_else(|| panic!("known_plugins() não tem entrada para {plugin_name}"))
            .sandbox_profile;

        PluginSpawnConfig {
            plugin_name: plugin_name.to_string(),
            command: "python3".to_string(),
            args: vec![main_py.to_string_lossy().into_owned()],
            sandbox_profile,
            code_root: repo_root().to_path_buf(),
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

/// Todos os processos vivos (não-zumbis) do sistema, como
/// `(pid, ppid, comm, cmdline)` — base para localizar descendentes
/// transitivos (não só filhos diretos) do processo de teste. `comm` é o
/// nome do executável (primeiro campo de `/proc/<pid>/stat`, entre
/// parênteses), útil para distinguir `bwrap` de `python3` quando ambos
/// casam pela mesma `cmdline` (feature 006: `bwrap --unshare-all` insere
/// dois processos `bwrap` — o externo e um reaper interno de PID namespace
/// — entre o processo de teste e o processo real do plugin, e o `cmdline`
/// do `bwrap` externo também contém o caminho do script porque ele é
/// passado como argumento final do sandbox).
fn all_live_processes() -> Vec<(i32, u32, String, String)> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `comm` é o campo entre parênteses; pode conter espaços/parênteses
        // internos, então usamos o primeiro `(` e o último `)` como bordas.
        let Some(comm_start) = str::find(&stat, '(') else {
            continue;
        };
        let Some(comm_end) = str::rfind(&stat, ')') else {
            continue;
        };
        if comm_end <= comm_start {
            continue;
        }
        let comm = stat[comm_start + 1..comm_end].to_string();
        let after_comm = &stat[comm_end + 1..];
        let mut fields = after_comm.split_whitespace();
        let (Some(state), Some(ppid)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(ppid) = ppid.parse::<u32>() else {
            continue;
        };
        if state == "Z" {
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
        processes.push((pid, ppid, comm, cmdline));
    }
    processes
}

/// Localiza o PID do processo `uptime-kuma` real spawnado pelo cenário em
/// andamento (T042). Diferente de [`live_child_processes`] (que só enxerga
/// filhos diretos, e é usado por [`assert_no_lingering_children`] para outro
/// propósito), esta função caminha [`all_live_processes`] pela cadeia de
/// `ppid` para achar todos os descendentes **transitivos** do processo de
/// teste — necessário desde a feature 006, que passou a rodar cada plugin
/// dentro de `bwrap`: a árvore real é processo de teste → `bwrap` externo →
/// `bwrap` interno/reaper de namespace de PID → `python3` do plugin, então o
/// processo real não é mais filho direto. Entre os descendentes cuja
/// `cmdline` contém `"uptime-kuma/main.py"` (o que também casa com os dois
/// processos `bwrap`, que recebem o caminho do script como argumento),
/// filtra adicionalmente por `comm == "python3"` para descartar os `bwrap`
/// intermediários e ficar só com o processo real do plugin. Como
/// [`E2E_LOCK`] serializa os cenários deste módulo e cada um spawna no
/// máximo um plugin, encontrar mais ou menos de um candidato aqui indicaria
/// um bug de isolamento entre testes, não uma condição normal deste cenário
/// — falha alto e claro em vez de escolher um dos candidatos
/// arbitrariamente.
fn find_uptime_kuma_pid() -> i32 {
    let me = std::process::id();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let processes = all_live_processes();

        // Descendentes transitivos de `me`: BFS pela cadeia de `ppid`.
        let mut descendant_pids: std::collections::HashSet<i32> = std::collections::HashSet::new();
        let mut frontier: Vec<u32> = vec![me];
        while let Some(parent) = frontier.pop() {
            for (pid, ppid, _, _) in &processes {
                if *ppid == parent && descendant_pids.insert(*pid) {
                    frontier.push(*pid as u32);
                }
            }
        }

        let matches: Vec<i32> = processes
            .into_iter()
            .filter(|(pid, _, comm, cmdline)| {
                descendant_pids.contains(pid)
                    && comm == "python3"
                    && cmdline.contains("uptime-kuma/main.py")
            })
            .map(|(pid, _, _, _)| pid)
            .collect();
        match matches.as_slice() {
            [pid] => return *pid,
            [] if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => panic!(
                "esperava exatamente 1 processo uptime-kuma vivo (descendente transitivo deste \
                 processo de teste, comm == \"python3\"), encontrou {other:?}"
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
// T026b — fixture `fake-openfortivpn-gui` via `PATH`
// ---------------------------------------------------------------------------

/// Diretório absoluto de `tests/fixtures/fake-openfortivpn-gui/` (T026a,
/// `specs/004-vpn-status-plugin/tasks.md`), resolvido a partir de
/// [`repo_root`] — nunca do `cwd` do processo, mesmo raciocínio de
/// [`HarnessFixture::spawn_config`].
///
/// Confirma que o script `openfortivpn-gui` (o nome do arquivo importa —
/// `plugins/openfortivpn-vpn/vpn_cli.py::find_binary()` resolve
/// `shutil.which("openfortivpn-gui")`) existe ali antes de qualquer cenário
/// depender dele, com a mesma disciplina de `spawn_config` de falhar com
/// causa óbvia em vez de um sintoma indireto mais tarde.
fn fake_openfortivpn_gui_dir() -> PathBuf {
    let dir = repo_root()
        .join("tests")
        .join("fixtures")
        .join("fake-openfortivpn-gui");
    assert!(
        dir.join("openfortivpn-gui").is_file(),
        "a fixture fake-openfortivpn-gui deveria existir em {dir:?} (T026a)"
    );
    dir
}

// ---------------------------------------------------------------------------
// T026a — fixture `fake-docker` via `PATH`
// ---------------------------------------------------------------------------

/// Diretório absoluto de `tests/fixtures/fake-docker/` (T026a,
/// `specs/005-docker-containers-plugin/tasks.md`), resolvido a partir de [`repo_root`] — nunca do
/// `cwd` do processo, mesmo raciocínio de [`fake_openfortivpn_gui_dir`].
///
/// Confirma que o script `docker` (o nome do arquivo importa —
/// `plugins/docker-containers/docker_cli.py::find_binary()` resolve `shutil.which("docker")`)
/// existe ali antes de qualquer cenário depender dele, com a mesma disciplina de falhar com causa
/// óbvia em vez de um sintoma indireto mais tarde.
fn fake_docker_dir() -> PathBuf {
    let dir = repo_root().join("tests").join("fixtures").join("fake-docker");
    assert!(
        dir.join("docker").is_file(),
        "a fixture fake-docker deveria existir em {dir:?} (T026a)"
    );
    dir
}

/// `ID` (64 hex `a`) do container `app` no cenário `FAKE_DOCKER_SCENARIO=action_target` de
/// `tests/fixtures/fake-docker/docker` (T033) — **precisa bater literalmente** com
/// `_ACTION_TARGET_APP_ID` daquele script. Fixo (em vez de derivado por hash, como
/// `_MULTI_STATE_CONTAINERS`) de propósito: os cenários de ação (T033) montam o `ActionTarget` de
/// `Message::ActionInvokeRequested` diretamente, sem primeiro ler o `Farol` real — o `Scenario` do
/// harness só expõe o modelo ao final, via `finish()`, que já encerra o processo do plugin.
const DOCKER_ACTION_TARGET_APP_ID: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// `ID` (64 hex `b`) do container `sidecar` do mesmo cenário `action_target` (T033) — um bystander
/// que nenhum cenário de ação toca, usado para confirmar que uma ação em `app` nunca mexe nas
/// demais linhas (FR-009). Precisa bater literalmente com `_ACTION_TARGET_SIDECAR_ID` da fixture.
const DOCKER_ACTION_TARGET_SIDECAR_ID: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Prepend de um diretório ao `PATH` do processo de teste (T026b), com
/// restauração no `Drop`.
///
/// `plugin_worker.rs::worker` (`Command::new(&config.command)`) resolve
/// `"python3"` herdando o `PATH` do processo pai — não existe hoje nenhum
/// hook de configuração para sobrescrever `PATH` por plugin individualmente.
/// Mutar o `PATH` do processo de teste inteiro é a única forma de fazer
/// `vpn_cli.py::find_binary()` (`shutil.which`, também herdado do processo
/// pai no spawn do plugin) resolver o script da fixture em vez do binário
/// real `openfortivpn-gui` (que pode nem estar instalado na máquina que roda
/// os testes). Só é seguro dentro de [`e2e_guard`] — mesma disciplina que
/// `HarnessFixture::new`/`with_uptime_kuma_base_url` já documentam para
/// `XDG_CONFIG_HOME`: `set_var`/`var`/`remove_var` de `PATH` são globais ao
/// processo, e nenhum outro teste deste módulo lê ou depende do valor de
/// `PATH` além do que o próprio `Command::spawn` do plugin herda.
///
/// **Preserva o restante do `PATH` original** (prepend, nunca substituição
/// total) — `python3` (todo plugin de referência) e `git` (fixture de
/// `git-local`) continuam precisando resolver normalmente pelo `PATH` do
/// sistema; só o diretório da fixture é adicionado à frente, para que
/// `shutil.which` o encontre primeiro caso o binário real também esteja
/// instalado.
struct PathPrefixGuard {
    original: Option<String>,
}

impl PathPrefixGuard {
    fn prepend(dir: &Path) -> Self {
        let original = std::env::var("PATH").ok();
        let new_path = match &original {
            Some(existing) => format!("{}:{existing}", dir.display()),
            None => dir.display().to_string(),
        };
        std::env::set_var("PATH", new_path);
        Self { original }
    }
}

impl Drop for PathPrefixGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
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
        "esperava que uptime-kuma alcançasse Ready pelo Emulator (handshake 0.4 + \
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

/// **T019 (US3, feature 007) — `templates/plugin-template/main.py` alcança `Ready`**.
///
/// Prova que o template mínimo de plugin (`templates/plugin-template/`, T018) é de fato um plugin
/// Farol funcional, e não só um exemplo estático nunca exercitado: mesmo mecanismo do gate T004
/// acima (`Emulator` real, processo Python real, handshake JSON-RPC/NDJSON real), agora contra o
/// `main.py` do template em vez de um plugin de referência sob `plugins/`.
///
/// Não usa `HarnessFixture::spawn_config` porque esse helper resolve `main.py` sob
/// `<repo_root>/plugins/<plugin_name>/` — o template mora em `<repo_root>/templates/
/// plugin-template/`. `PluginSpawnConfig` é construído aqui diretamente, com `plugin_name:
/// "meu-plugin"` (não um nome inventado como `"template-teste"`): o handshake real de
/// `templates/plugin-template/main.py` devolve `PLUGIN_NAME = "meu-plugin"` fixo (T018 optou por não
/// sujar o template com lógica de teste), e `plugin_state` abaixo localiza o slot da conexão pelo
/// `plugin_name` do próprio `PluginSpawnConfig` de spawn — não pelo valor devolvido no handshake —,
/// então usar um nome diferente do que o script realmente devolve não afetaria a busca do slot; a
/// escolha é para o teste refletir com exatidão o que um usuário copiando o template do jeito que o
/// README manda receberia.
#[test]
fn emulator_takes_plugin_template_through_a_real_handshake_to_ready() {
    let _guard = e2e_guard();
    let _fixture = HarnessFixture::new("plugin-template-ready");

    let main_py = repo_root()
        .join("templates")
        .join("plugin-template")
        .join("main.py");
    assert!(
        main_py.is_file(),
        "o `main.py` do template deveria existir em {main_py:?}"
    );

    let spawn_config = PluginSpawnConfig {
        plugin_name: "meu-plugin".to_string(),
        command: "python3".to_string(),
        args: vec![main_py.to_string_lossy().into_owned()],
        sandbox_profile: crate::sandbox::SandboxProfile {
            allow_network: false,
            allow_exec: false,
            extra_binds: vec![],
        },
        code_root: repo_root().to_path_buf(),
    };

    let observed = observe_plugin_state(
        "template de plugin (meu-plugin) alcança Ready", spawn_config,
    );

    assert_eq!(
        observed,
        PluginState::Ready,
        "esperava que o template de plugin alcançasse Ready pelo Emulator (handshake 0.4, sem \
         widgets/ações), obteve {observed:?}"
    );
}

/// **T011 (US1, feature 007) — um plugin *descoberto dinamicamente* alcança `Ready` pela máquina
/// de estados real**.
///
/// Diferença deste teste para os gates acima: em vez de um `PluginSpawnConfig` montado à mão
/// (`HarnessFixture::spawn_config`/literal), o `PluginSpawnConfig` aqui vem de
/// `plugin_worker::discover_installed_plugins()` (T008) — provando que o caminho inteiro (`farol-
/// plugin.toml` real em disco → `parse_manifest` → `discover_installed_plugins` → `Subscription`
/// real → handshake JSON-RPC/NDJSON real → `PluginState::Ready`) funciona de ponta a ponta, não só
/// a função de descoberta isoladamente (isso já é coberto por `plugin_worker::tests`, T010).
///
/// Fixture mínima criada em runtime pelo próprio teste, mesmo espírito do exemplo do Cenário 1 de
/// `quickstart.md`: `<tmp>/xdg-data/farol/plugins/exemplo/{farol-plugin.toml,main.py}`, onde
/// `main.py` responde ao `handshake/hello` com sucesso, protocolo `"0.4"`
/// (`plugin_worker::CORE_PROTOCOL_VERSION`), zero widgets/ações/`required_config` — o contrato
/// mínimo do protocolo, nada além disso (mesmo formato do `templates/plugin-template/main.py`
/// exercitado pelo teste acima, só que escrito à mão aqui em vez de reaproveitado, porque a
/// identidade do plugin — `plugin_name = "exemplo"` — precisa bater com o nome do subdiretório sob
/// `farol_data_base_dir()/plugins/`, ao contrário do template).
///
/// `XDG_DATA_HOME` aponta para essa fixture — hermético, restaurado ao final, protegido por
/// `E2E_LOCK` (mesma disciplina de `HarnessFixture` para `XDG_CONFIG_HOME`: ambas são variáveis
/// globais ao processo, e este módulo é quem as manipula em todo o crate). `XDG_CONFIG_HOME`
/// também aponta para um diretório vazio próprio da fixture — o plugin descoberto não declara
/// `required_config`, então não há nada para o core resolver ali, mas isolar mesmo assim evita
/// depender do `~/.config` real da máquina que roda o teste.
#[test]
fn discovered_plugin_reaches_ready_through_the_real_state_machine() {
    let _guard = e2e_guard();

    let base = std::env::temp_dir().join(format!(
        "farol-e2e-discovered-plugin-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);

    let plugin_dir = base
        .join("xdg-data")
        .join("farol")
        .join("plugins")
        .join("exemplo");
    fs::create_dir_all(&plugin_dir).expect("criar diretório do plugin descoberto da fixture");
    fs::write(
        plugin_dir.join("farol-plugin.toml"),
        "plugin_name = \"exemplo\"\ncommand = \"python3\"\nargs = [\"main.py\"]\n\n\
         [capabilities]\nexec = false\nnetwork = false\n",
    )
    .expect("escrever farol-plugin.toml da fixture");
    // Loop de leitura (não um único `readline`): o `Emulator`/`Farol` real mantém o worker vivo
    // depois do handshake, dentro do mesmo `tokio::select!` que também observa `child.wait()`
    // (`plugin_worker::worker`, T038/D6) — um script que responde ao handshake e sai em seguida
    // termina o processo (`exit status: 0`) e o worker interpreta isso como `WorkerEvent::Crashed`
    // antes mesmo de o `Emulator` conseguir observar `Ready`. Mesmo formato de loop de
    // `templates/plugin-template/main.py` (T018, ver a docstring deste teste), só sem widgets/
    // ações para responder — só o handshake é exercitado aqui.
    fs::write(
        plugin_dir.join("main.py"),
        "import json, sys\n\
         for raw_line in sys.stdin:\n\
         \x20\x20\x20\x20line = raw_line.strip()\n\
         \x20\x20\x20\x20if not line:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20continue\n\
         \x20\x20\x20\x20req = json.loads(line)\n\
         \x20\x20\x20\x20result = {\"protocol_version\": \"0.4\", \"plugin_name\": \"exemplo\",\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\"capabilities\": {\"capabilities\": []}, \
         \"required_config\": [],\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\"widgets\": [], \"actions\": []}\n\
         \x20\x20\x20\x20print(json.dumps({\"jsonrpc\": \"2.0\", \"id\": req[\"id\"], \
         \"result\": result}))\n\
         \x20\x20\x20\x20sys.stdout.flush()\n",
    )
    .expect("escrever main.py da fixture");

    let xdg_config_home = base.join("xdg-config");
    fs::create_dir_all(&xdg_config_home).expect("criar XDG_CONFIG_HOME vazio da fixture");

    // `set_var` é global ao processo — seguro aqui porque este teste segura `E2E_LOCK`, mesma
    // disciplina de `HarnessFixture::new`/`with_uptime_kuma_base_url` para `XDG_CONFIG_HOME`
    // (ver a docstring de `E2E_LOCK`). `XDG_DATA_HOME` não é lido por nenhum outro teste deste
    // módulo (só por `plugin_worker::tests`, que tem seu próprio lock local, `XDG_DATA_HOME_LOCK`,
    // separado de `E2E_LOCK`, e não roda simultaneamente com este por serem módulos distintos
    // dentro do mesmo binário de teste — `cargo test` só paraleliza dentro do orçamento de
    // threads, mas nunca dispensa a serialização que cada lock impõe sobre a variável que protege).
    std::env::set_var("XDG_DATA_HOME", base.join("xdg-data"));
    std::env::set_var("XDG_CONFIG_HOME", &xdg_config_home);

    let discovered = crate::plugin_worker::discover_installed_plugins();
    assert_eq!(
        discovered.len(),
        1,
        "esperava exatamente 1 plugin descoberto pela fixture, obteve {discovered:?}"
    );
    let spawn_config = discovered.into_iter().next().unwrap();
    assert_eq!(spawn_config.plugin_name, "exemplo");
    assert_eq!(spawn_config.code_root, plugin_dir);

    let observed = observe_plugin_state("plugin descoberto dinamicamente alcança Ready", spawn_config);

    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
    let _ = fs::remove_dir_all(&base);

    assert_eq!(
        observed,
        PluginState::Ready,
        "esperava que o plugin descoberto dinamicamente alcançasse Ready pelo Emulator, obteve \
         {observed:?}"
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
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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
        restarted.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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

/// **T039 [US1] / T051 (débito #5, issue #7 — corrigido)** — Cenário 6 de
/// `quickstart.md`: instância Uptime Kuma real, acessível, mas recém
/// instalada e sem nenhum monitor cadastrado. O critério de aceite
/// documentado (`spec.md` Edge Case, `quickstart.md` Cenário 6, `tasks.md`
/// T039) é `widget/get` responder com sucesso e `items: []` — estado válido,
/// análogo ao diretório sem repositórios git da feature 001.
///
/// `plugins/uptime-kuma/metrics_parser.py::parse_metrics` distinguia "zero
/// monitores" de "resposta não reconhecível" incorretamente: as duas
/// condições produziam exatamente a mesma falha, `MetricsParseError`, porque
/// a única checagem era a presença de uma linha `monitor_status{...}` com
/// amostra. Corrigido reconhecendo também a declaração `# HELP`/`# TYPE
/// monitor_status` (que o Prometheus sempre emite para uma família de
/// métrica registrada, mesmo sem amostras) como sinal de instância real —
/// ver `plugins/uptime-kuma/metrics_parser.py` e
/// `plugins/uptime-kuma/test_metrics_parser.py::test_instance_with_no_monitors_returns_empty_items`.
#[test]
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
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"));

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
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"));

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
        harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"));

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
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"));

    let pid = find_uptime_kuma_pid();
    send_signal(pid, "-9");

    wait_until_passive(
        &mut harness,
        "saída de Ready após kill -9",
        |h| !h.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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
    assert!(harness.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"));

    let pid = find_uptime_kuma_pid();
    send_signal(pid, "-STOP");

    harness.dispatch(Message::RefreshTick {
        plugin_name: "uptime-kuma".to_string(),
    });
    wait_until_passive(
        &mut harness,
        "saída de Ready após kill -STOP",
        |h| !h.screen_shows("Plugin: uptime-kuma (protocolo 0.4)"),
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

/// **T026b [US1] — `openfortivpn-vpn` alcança `Ready` e popula o widget `vpn-status`**
/// (`specs/004-vpn-status-plugin/tasks.md` T026), mesmo padrão de
/// [`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`] (T006).
///
/// `openfortivpn-vpn` declara `required_config: []` (`research.md` D6,
/// `plugins/openfortivpn-vpn/main.py::handle_handshake_hello`) — alcançar `Ready` depende só do
/// handshake, então [`HarnessFixture::new`] (sem apontar nenhum `base_url`/`api_key` específico
/// para este plugin — os valores que ela grava são só para `git-local`/`uptime-kuma`, ignorados
/// por este terceiro plugin) já basta.
///
/// O que este cenário precisa controlar é a saída de `openfortivpn-gui status --json` — feito
/// prependando o diretório de [`fake_openfortivpn_gui_dir`] (T026a) ao `PATH` do processo de teste
/// via [`PathPrefixGuard`] e configurando as variáveis `FAKE_OPENFORTIVPN_*` que a fixture
/// reconhece, para simular uma sessão `connected` com dois perfis conhecidos.
///
/// Diferente de `uptime-kuma` (T006), `openfortivpn-vpn` não tem uma `PollerThread` própria: cada
/// `widget/get` invoca `openfortivpn-gui status --json` diretamente e de forma síncrona
/// (`plugins/openfortivpn-vpn/vpn_cli.py::query_status`) — não há corrida contra nenhum relógio
/// próprio do plugin (ao contrário do `suggested_refresh_interval_ms` de 30s de `uptime-kuma`), e
/// o primeiro `widget/get` que o core dispara assim que a conexão fica `Ready` (`update.rs`,
/// "fetch imediato ao ficar Ready") já traz os dados simulados. Ainda assim, o cenário usa
/// [`wait_until`] (em vez de assumir sincronismo perfeito entre uma única chamada a
/// `Scenario::settle` e a resposta assíncrona do worker) — mesma folga que os demais cenários
/// deste módulo.
#[test]
fn openfortivpn_vpn_reaches_ready_and_populates_the_vpn_widget() {
    let _guard = e2e_guard();

    let fixture_dir = fake_openfortivpn_gui_dir();
    let _path_guard = PathPrefixGuard::prepend(&fixture_dir);

    // Variáveis reconhecidas por `tests/fixtures/fake-openfortivpn-gui/openfortivpn-gui` (T026a) —
    // simula uma sessão `connected`, perfil `escritorio` ativo, `casa` como segundo perfil
    // conhecido, sessão com 125s decorridos.
    std::env::set_var("FAKE_OPENFORTIVPN_STATE", "connected");
    std::env::set_var("FAKE_OPENFORTIVPN_PROFILE", "escritorio");
    std::env::set_var("FAKE_OPENFORTIVPN_PROFILES", "escritorio,casa");
    std::env::set_var("FAKE_OPENFORTIVPN_ELAPSED", "125");
    std::env::remove_var("FAKE_OPENFORTIVPN_ERROR");

    let fixture = HarnessFixture::new("openfortivpn-vpn-widget");

    let mut harness = start_scenario(
        "openfortivpn-vpn alcança Ready e popula o vpn-status",
        fixture.spawn_config("openfortivpn-vpn"),
    );
    harness.settle();

    // A conexão saiu de Starting/Handshaking — e chegou a `Ready` (sem tela de setup: D6, sem
    // required_config): só `view_ready` renderiza esta linha.
    assert!(
        harness.screen_shows("Plugin: openfortivpn-vpn (protocolo 0.4)"),
        "openfortivpn-vpn deveria estar Ready assim que o handshake completa (sem required_config, \
         D6)"
    );

    // Espera o primeiro ciclo de dados chegar à tela (ver docstring: sem PollerThread própria,
    // mas ainda assim assíncrono do ponto de vista do Emulator).
    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get do openfortivpn-vpn",
        |h| h.screen_shows("Estado: conectado"),
        STATE_TIMEOUT,
    );

    // A tela mostra os dados *derivados* de `view_vpn_widget` — estado mapeado, perfil ativo,
    // lista de perfis disponíveis.
    for text in [
        "Estado: conectado",
        "Perfil ativo: escritorio",
        "escritorio",
        "casa",
    ] {
        assert!(
            harness.screen_shows(text),
            "o widget vpn-status deveria renderizar {text:?}"
        );
    }
    assert!(
        !harness.screen_shows("Nenhum perfil VPN configurado."),
        "com dois perfis simulados, a tela não pode mostrar o estado \"sem perfis\""
    );
    assert!(
        !harness.screen_shows("Aguardando primeira leitura do estado da VPN..."),
        "com um ciclo de dados já recebido, a tela não deveria mostrar o estado de espera inicial"
    );

    let app = harness.finish();

    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "openfortivpn-vpn")
        .expect("slot de openfortivpn-vpn")
        .connection;

    assert_eq!(connection.state, PluginState::Ready);

    let status = connection
        .vpn_widget
        .status
        .as_ref()
        .expect("vpn_widget.status deveria estar populado após o ciclo de widget/get");
    assert_eq!(
        status.state,
        farol_protocol::messages::VpnConnectionState::Connected
    );
    assert_eq!(status.active_profile.as_deref(), Some("escritorio"));
    assert_eq!(status.elapsed_seconds, Some(125.0));
    assert_eq!(
        status
            .available_profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        vec!["escritorio", "casa"],
        "os perfis decodificados deveriam bater exatamente com FAKE_OPENFORTIVPN_PROFILES, na \
         mesma ordem"
    );
    assert_eq!(
        connection.vpn_widget.last_error, None,
        "um ciclo de widget/get bem-sucedido MUST limpar o erro pontual anterior"
    );

    drop(app);
    assert_no_lingering_children();
}

/// **T026b [US1] — `docker-containers` alcança `Ready` e popula o widget `container-status-grid`**
/// (`specs/005-docker-containers-plugin/tasks.md` T026), mesmo padrão de
/// [`openfortivpn_vpn_reaches_ready_and_populates_the_vpn_widget`] (T026 da feature 004).
///
/// `docker-containers` declara `required_config: []` (`research.md` D9,
/// `plugins/docker-containers/main.py::handle_handshake_hello`) — alcançar `Ready` depende só do
/// handshake, então [`HarnessFixture::new`] já basta (os valores de `base_url`/`api_key` que ela
/// grava são só para `git-local`/`uptime-kuma`, ignorados por este quarto plugin).
///
/// O que este cenário precisa controlar é a saída de `docker ps --all --no-trunc --format
/// '{{json .}}'` — feito prependando o diretório de [`fake_docker_dir`] (T026a) ao `PATH` do
/// processo de teste via [`PathPrefixGuard`] e configurando `FAKE_DOCKER_SCENARIO=multi_state`, que
/// a fixture reconhece para simular três containers (`contracts/docker-cli-mapping.md`): `web`
/// (`running`), `db` (`exited`), e `mystery` (um `State` fora do vocabulário conhecido do Docker,
/// que `docker_cli.py` MUST traduzir para `unknown` sem invalidar as demais linhas, FR-012).
///
/// Sem `PollerThread`/relógio próprio (mesmo raciocínio de `openfortivpn-vpn`, T026b da feature
/// 004): cada `widget/get` invoca `docker ps` diretamente e de forma síncrona
/// (`docker_cli.py::list_containers`) — o primeiro `widget/get` que o core dispara assim que a
/// conexão fica `Ready` já traz os dados simulados, mas o cenário ainda usa [`wait_until`] pela
/// mesma folga que os demais cenários deste módulo, em vez de assumir sincronismo perfeito com uma
/// única `Scenario::settle`.
#[test]
fn docker_containers_reaches_ready_and_populates_the_container_grid() {
    let _guard = e2e_guard();

    let fixture_dir = fake_docker_dir();
    let _path_guard = PathPrefixGuard::prepend(&fixture_dir);
    std::env::set_var("FAKE_DOCKER_SCENARIO", "multi_state");

    let fixture = HarnessFixture::new("docker-containers-widget");

    let mut harness = start_scenario(
        "docker-containers alcança Ready e popula o container-status-grid",
        fixture.spawn_config("docker-containers"),
    );
    harness.settle();

    // A conexão saiu de Starting/Handshaking — e chegou a `Ready` (sem tela de setup: D9, sem
    // required_config): só `view_ready` renderiza esta linha.
    assert!(
        harness.screen_shows("Plugin: docker-containers (protocolo 0.4)"),
        "docker-containers deveria estar Ready assim que o handshake completa (sem \
         required_config, D9)"
    );

    // Espera o primeiro ciclo de dados chegar à tela (mesmo mecanismo de wait_until dos demais
    // cenários — ver docstring acima sobre por que não basta assumir sincronismo perfeito).
    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get do docker-containers",
        |h| h.screen_shows("web") && h.screen_shows("db") && h.screen_shows("mystery"),
        STATE_TIMEOUT,
    );

    // A tela mostra os dados *derivados* de `view_container_grid` — cabeçalho, nomes, imagens e
    // estado mapeado para PT-BR, incluindo o container com `state` desconhecido virando
    // "desconhecido" (FR-012) sem invalidar as demais linhas.
    for text in [
        "Nome",
        "Imagem",
        "Estado",
        "web",
        "nginx:latest",
        "rodando",
        "db",
        "postgres:16",
        "parado",
        "mystery",
        "desconhecido",
    ] {
        assert!(
            harness.screen_shows(text),
            "o widget container-status-grid deveria renderizar {text:?}"
        );
    }
    assert!(
        !harness.screen_shows("Aguardando primeira leitura dos containers..."),
        "com três containers simulados, a tela não pode mostrar o estado de espera inicial"
    );
    assert!(
        !harness.screen_shows("Nenhum container encontrado."),
        "com três containers simulados, a tela não pode mostrar o estado vazio"
    );

    let app = harness.finish();

    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "docker-containers")
        .expect("slot de docker-containers")
        .connection;

    assert_eq!(connection.state, PluginState::Ready);
    assert!(
        connection.docker_widget.loaded,
        "um widget/get bem-sucedido MUST marcar loaded = true (FR-011)"
    );
    assert_eq!(connection.docker_widget.containers.len(), 3);
    assert_eq!(
        connection.docker_widget.last_error, None,
        "um ciclo de widget/get bem-sucedido MUST limpar o erro pontual anterior"
    );

    let names: Vec<&str> = connection
        .docker_widget
        .containers
        .iter()
        .map(|container| container.item.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["db", "mystery", "web"],
        "os containers decodificados deveriam vir ordenados por (name, id) — research.md D10"
    );

    let mystery = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.name == "mystery")
        .expect("container mystery deveria estar presente");
    assert_eq!(
        mystery.item.state,
        farol_protocol::messages::ContainerState::Unknown,
        "um State fora do vocabulário conhecido do Docker (\"borked\", simulado pela fixture) MUST \
         virar unknown (FR-012), sem invalidar as demais linhas"
    );
    assert!(
        mystery.action_in_flight.is_none(),
        "nenhuma ação foi disparada neste cenário (só leitura, US1)"
    );
    assert!(mystery.last_action_error.is_none());

    let web = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.name == "web")
        .expect("container web deveria estar presente");
    assert_eq!(
        web.item.state,
        farol_protocol::messages::ContainerState::Running
    );
    let db = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.name == "db")
        .expect("container db deveria estar presente");
    assert_eq!(
        db.item.state,
        farol_protocol::messages::ContainerState::Exited
    );

    drop(app);
    assert_no_lingering_children();
}

/// **T033 [US2] — `docker.container.start` bem-sucedido leva a linha do container a "rodando", com
/// os botões invertidos corretamente** (`specs/005-docker-containers-plugin/tasks.md` T033), mesmo
/// espírito dos cenários de ação já existentes deste módulo (dispatch de
/// `Message::ActionInvokeRequested` via [`Scenario::dispatch`]), agora fim a fim contra o plugin
/// `docker-containers` real, em vez de só `update()` chamado diretamente (cobertura já existente em
/// `update.rs::action_invoke_requested_for_docker_container_marks_action_in_flight_and_sends_invoke`/
/// `action_success_outcome_for_docker_container_replaces_only_that_containers_item`).
///
/// A fixture usa `FAKE_DOCKER_SCENARIO=action_target` (T033, `tests/fixtures/fake-docker/docker`) —
/// dois containers de IDs **fixos** ([`DOCKER_ACTION_TARGET_APP_ID`]/
/// [`DOCKER_ACTION_TARGET_SIDECAR_ID`], não derivados de hash como `multi_state`), justamente para
/// que este cenário monte o `ActionTarget` da ação sem primeiro precisar ler o `Farol` real (o
/// `Scenario` do harness só expõe o modelo ao final, via `finish()`, que já encerra o processo do
/// plugin).
///
/// Depois de despachar a ação, o cenário usa [`wait_until_passive`] — **não** [`wait_until`] — de
/// propósito: a fixture `action_target` só reporta `app` como `running` na releitura **filtrada**
/// que `docker_cli.py::_reread_container` faz depois de uma ação bem-sucedida (ver a docstring de
/// `_action_target_rows` na fixture); um `RefreshTick` disparado nesse meio tempo pediria uma
/// listagem **cheia**, que a fixture sempre reporta com `app` ainda `exited` (convenção deliberada,
/// documentada ali — a fixture não tem nenhum estado persistido entre processos). A resposta da
/// própria ação já está a caminho assim que o worker a despachou; não há nenhum novo estímulo a
/// dar, só drenar o que já está em voo.
#[test]
fn docker_container_start_action_succeeds_and_flips_the_row_to_running() {
    let _guard = e2e_guard();

    let fixture_dir = fake_docker_dir();
    let _path_guard = PathPrefixGuard::prepend(&fixture_dir);
    std::env::set_var("FAKE_DOCKER_SCENARIO", "action_target");
    std::env::remove_var("FAKE_DOCKER_ACTION_SCENARIO"); // default "success"

    let fixture = HarnessFixture::new("docker-containers-start-success");

    let mut harness = start_scenario(
        "docker.container.start bem-sucedido leva a linha a \"rodando\"",
        fixture.spawn_config("docker-containers"),
    );
    harness.settle();

    assert!(
        harness.screen_shows("Plugin: docker-containers (protocolo 0.4)"),
        "docker-containers deveria estar Ready assim que o handshake completa (sem \
         required_config, D9)"
    );

    // Primeiro ciclo de widget/get: `app` (exited/"parado") e `sidecar` (running/"rodando",
    // nunca tocado por nenhuma ação deste cenário).
    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get do docker-containers (action_target)",
        |h| h.screen_shows("app") && h.screen_shows("sidecar"),
        STATE_TIMEOUT,
    );
    assert!(
        harness.screen_shows("parado"),
        "app deveria começar exited/\"parado\" (convenção da fixture action_target)"
    );

    // Dispara `docker.container.start` para `app` — mesmo shape de `ActionTarget`/
    // `Message::ActionInvokeRequested` que `view_container_action_control` (view.rs) monta a
    // partir do clique real no botão "Iniciar".
    harness.dispatch(Message::ActionInvokeRequested {
        plugin_name: "docker-containers".to_string(),
        action_id: "docker.container.start".to_string(),
        target: farol_protocol::ActionTarget {
            r#type: "docker-container".to_string(),
            id: DOCKER_ACTION_TARGET_APP_ID.to_string(),
        },
        timeout_hint_ms: Some(20_000),
    });

    // Ver docstring da função: passivo de propósito — um `RefreshTick` aqui pediria uma listagem
    // cheia, que a fixture sempre responde com `app` ainda "exited".
    wait_until_passive(
        &mut harness,
        "resposta de docker.container.start para \"app\"",
        |h| !h.screen_shows("parado"),
        STATE_TIMEOUT,
    );
    assert!(
        harness.screen_shows("rodando"),
        "app deveria aparecer \"rodando\" depois de docker.container.start bem-sucedido"
    );

    let app = harness.finish();
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "docker-containers")
        .expect("slot de docker-containers")
        .connection;

    assert_eq!(connection.state, PluginState::Ready);

    let target = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.id == DOCKER_ACTION_TARGET_APP_ID)
        .expect("container app deveria continuar presente");

    assert_eq!(
        target.item.state,
        farol_protocol::messages::ContainerState::Running
    );
    assert!(
        !target.item.start_action.enabled,
        "start deveria ficar desabilitado com o container já rodando (FR-008)"
    );
    assert!(
        target.item.stop_action.enabled,
        "stop deveria ficar habilitado com o container rodando (FR-008)"
    );
    assert!(
        target.item.restart_action.enabled,
        "restart deveria continuar habilitado com o container rodando (FR-008)"
    );
    assert!(
        target.action_in_flight.is_none(),
        "action_in_flight MUST ser limpo ao receber a resposta (sucesso ou erro)"
    );
    assert!(target.last_action_error.is_none());

    // FR-009: a ação em `app` não pode ter mexido em `sidecar`.
    let sidecar = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.id == DOCKER_ACTION_TARGET_SIDECAR_ID)
        .expect("container sidecar deveria continuar presente, intocado");
    assert_eq!(
        sidecar.item.state,
        farol_protocol::messages::ContainerState::Running
    );
    assert!(sidecar.action_in_flight.is_none());
    assert!(sidecar.last_action_error.is_none());

    drop(app);
    assert_no_lingering_children();
}

/// **T033 [US2] — falha simulada de `docker.container.start` (`no_such_container`) resulta em
/// mensagem traduzida visível na linha daquele container, sem derrubar o core nem apagar a lista
/// dos demais containers** (`specs/005-docker-containers-plugin/tasks.md` T033), mesmo espírito de
/// [`docker_container_start_action_succeeds_and_flips_the_row_to_running`] acima, agora com
/// `FAKE_DOCKER_ACTION_SCENARIO=no_such_container` (T033, `tests/fixtures/fake-docker/docker`) — a
/// formulação de stderr observada em Docker 29.6.2 (`contracts/docker-cli-mapping.md` § action/
/// invoke), classificada por `docker_cli.py::_classify_action_stderr` e traduzida para PT-BR
/// (`docker_cli.py::_ACTION_ERROR_MESSAGES`) antes de chegar ao core — a mesma mensagem que
/// `update.rs::action_plugin_error_outcome_for_docker_container_sets_last_action_error_only_for_that_container`
/// já cobre no nível de `update()`, agora fim a fim contra o plugin real.
///
/// Diferente do cenário de sucesso, aqui `docker start <id>` falha **antes** de qualquer releitura
/// (`docker_cli.py::_run_action` só releitura em sucesso) — o estado de `app` permanece `exited` (a
/// fixture nem chega a ser consultada de novo para aquele `id`), então este cenário também prova
/// que uma falha não inventa uma transição de estado que não aconteceu.
#[test]
fn docker_container_start_action_failure_shows_translated_error_without_dropping_other_containers()
{
    let _guard = e2e_guard();

    let fixture_dir = fake_docker_dir();
    let _path_guard = PathPrefixGuard::prepend(&fixture_dir);
    std::env::set_var("FAKE_DOCKER_SCENARIO", "action_target");
    std::env::set_var("FAKE_DOCKER_ACTION_SCENARIO", "no_such_container");

    let fixture = HarnessFixture::new("docker-containers-start-failure");

    let mut harness = start_scenario(
        "falha de docker.container.start mostra mensagem traduzida sem derrubar a lista",
        fixture.spawn_config("docker-containers"),
    );
    harness.settle();

    assert!(harness.screen_shows("Plugin: docker-containers (protocolo 0.4)"));

    wait_until(
        &mut harness,
        "primeiro ciclo de widget/get do docker-containers (action_target)",
        |h| h.screen_shows("app") && h.screen_shows("sidecar"),
        STATE_TIMEOUT,
    );

    harness.dispatch(Message::ActionInvokeRequested {
        plugin_name: "docker-containers".to_string(),
        action_id: "docker.container.start".to_string(),
        target: farol_protocol::ActionTarget {
            r#type: "docker-container".to_string(),
            id: DOCKER_ACTION_TARGET_APP_ID.to_string(),
        },
        timeout_hint_ms: Some(20_000),
    });

    // Tradução PT-BR exata de `docker_cli.py::_ACTION_ERROR_MESSAGES["no_such_container"]`
    // (`contracts/docker-cli-mapping.md` § action/invoke) — `view.rs::view_container_row` a
    // renderiza como `"Falha: {error}"`.
    const TRANSLATED_ERROR: &str =
        "O container não existe mais — ele pode ter sido removido enquanto a lista estava aberta.";
    let expected_line = format!("Falha: {TRANSLATED_ERROR}");

    // Passivo pelo mesmo motivo do cenário de sucesso acima — não há nenhum `widget/get` novo a
    // pedir, só a resposta (de erro) da ação já em voo a drenar.
    wait_until_passive(
        &mut harness,
        "resposta de erro de docker.container.start para \"app\"",
        |h| h.screen_shows(&expected_line),
        STATE_TIMEOUT,
    );

    // FR-009: o core continua respondendo, com a lista inteira ainda visível — nem a linha de
    // `app` nem a de `sidecar` desapareceram por causa do erro.
    assert!(harness.screen_shows("app"));
    assert!(harness.screen_shows("sidecar"));
    assert!(
        harness.screen_shows("parado"),
        "uma falha na própria ação (sem releitura) MUST deixar o estado do container como estava"
    );

    let app = harness.finish();
    let connection = &app
        .plugins
        .iter()
        .find(|slot| slot.spawn_config.plugin_name == "docker-containers")
        .expect("slot de docker-containers")
        .connection;

    assert_eq!(
        connection.state,
        PluginState::Ready,
        "um erro de ação MUST NOT derrubar o core"
    );
    assert_eq!(
        connection.docker_widget.containers.len(),
        2,
        "nenhum container foi removido por causa do erro"
    );

    let target = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.id == DOCKER_ACTION_TARGET_APP_ID)
        .expect("container app deveria continuar presente");
    assert_eq!(
        target.item.state,
        farol_protocol::messages::ContainerState::Exited,
        "sem releitura (a própria ação falhou antes dela), o estado do container não muda"
    );
    assert_eq!(target.last_action_error.as_deref(), Some(TRANSLATED_ERROR));
    assert!(
        target.action_in_flight.is_none(),
        "action_in_flight MUST ser limpo mesmo em erro"
    );

    let sidecar = connection
        .docker_widget
        .containers
        .iter()
        .find(|container| container.item.id == DOCKER_ACTION_TARGET_SIDECAR_ID)
        .expect("container sidecar deveria continuar presente, intocado");
    assert!(sidecar.last_action_error.is_none());
    assert!(sidecar.action_in_flight.is_none());

    drop(app);
    assert_no_lingering_children();
}
