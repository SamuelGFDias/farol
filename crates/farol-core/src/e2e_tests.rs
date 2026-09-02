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
//! harness.sh`, T008, ainda não escrita).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
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

/// Viewport headless do `Emulator` — o mesmo default de `iced` (1024x768).
const VIEWPORT: Size = Size::new(1024.0, 768.0);

/// `base_url` da fixture de `uptime-kuma`: porta 1 de `127.0.0.1`, que nunca
/// tem serviço escutando — a `PollerThread` do plugin recebe "connection
/// refused" imediatamente, sem latência e **sem nenhum tráfego de rede para
/// fora da máquina**. Suficiente para o cenário de `Ready` deste módulo, que
/// depende só do handshake + `required_config` resolvido; o duplo HTTP
/// determinístico de `/metrics` previsto em `research.md` D2 continua sendo
/// tarefa de T006 (assertivas sobre os *dados* do widget), não deste gate.
const UPTIME_KUMA_FIXTURE_BASE_URL: &str = "http://127.0.0.1:1";

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
/// processo.
static E2E_LOCK: Mutex<()> = Mutex::new(());

fn e2e_guard() -> MutexGuard<'static, ()> {
    // Um teste E2E que falhe envenena o mutex; o próximo não deve falhar por
    // tabela — o dado protegido é `()`, não há estado inconsistente possível.
    E2E_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    fn new(label: &str) -> Self {
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
            &format!("base_url = {UPTIME_KUMA_FIXTURE_BASE_URL:?}\n"),
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
// T004 — o `Emulator` roda a `Subscription` real de ponta a ponta
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

/// Bombeia o loop do `Emulator` até a tela do Farol deixar de mostrar
/// qualquer um de [`TRANSIENT_SCREEN_TEXTS`] — ou seja, até a conexão sair de
/// `Starting`/`Handshaking` — e então devolve o estado real da aplicação.
///
/// Mesmo padrão de laço de `iced_test::run`/`iced_test::screenshot`: todo
/// `Event::Action` recebido precisa voltar para `Emulator::perform`, senão o
/// runtime não progride. A condição de parada é consultada via
/// `Instruction::Expect`, que reaproveita o renderer/`UserInterface` do
/// próprio `Emulator` (construir um `Simulator` a cada iteração recriaria um
/// renderer headless inteiro por consulta).
fn run_until_settled<P>(
    program: &P,
    mut emulator: Emulator<P>,
    mut receiver: Receiver<P>,
    plugin_name: &str,
) -> Farol
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    // `Emulator::new` termina o boot emitindo exatamente um `Event::Ready`
    // (`Mode::Immediate`: `wait_for` com `Task::none()` e nenhuma task
    // pendente). Ele precisa ser consumido **antes** do laço, senão a primeira
    // consulta de [`screen_shows`] o interpretaria como a resposta do seu
    // próprio `Expect` e todas as respostas seguintes ficariam deslocadas em
    // um — um teste que "passa" pelo motivo errado, exatamente a classe de
    // diagnóstico enganoso que esta feature existe para eliminar.
    consume_ready(program, &mut emulator, &mut receiver);

    let deadline = Instant::now() + STATE_TIMEOUT;

    loop {
        let still_transient = TRANSIENT_SCREEN_TEXTS
            .iter()
            .any(|text| screen_shows(program, &mut emulator, &mut receiver, text));

        if !still_transient {
            return emulator.into_state().0;
        }

        if Instant::now() >= deadline {
            // FR-003/FR-004: a mensagem nomeia o plugin e o estado realmente
            // observado, sem exigir leitura de log bruto.
            let observed = plugin_state(&emulator.into_state().0, plugin_name);
            panic!(
                "plugin {plugin_name:?} não saiu de Starting/Handshaking em {STATE_TIMEOUT:?} \
                 — estado observado: {observed:?}"
            );
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Monta o `Program` real com um único slot de plugin (apontado para a
/// fixture, por caminho absoluto), roda-o no `Emulator` até a conexão sair de
/// `Starting`/`Handshaking` e devolve o `PluginState` alcançado.
///
/// Um slot só, e não `Farol::default()`, porque `known_plugins()` traria os
/// dois plugins de referência: cada cenário afirma sobre um deles, e spawnar
/// o outro junto só adicionaria variabilidade sem cobrir nada a mais.
fn observe_plugin_state(spawn_config: PluginSpawnConfig) -> PluginState {
    let plugin_name = spawn_config.plugin_name.clone();
    let program = crate::program(move || Farol::with_plugins(vec![spawn_config.clone()]));

    let (sender, receiver) = mpsc::channel(100);
    let emulator = Emulator::new(sender, &program, Mode::Immediate, VIEWPORT);

    let app = run_until_settled(&program, emulator, receiver, &plugin_name);
    plugin_state(&app, &plugin_name)
}

/// `true` ⟺ a tela atual do `Emulator` contém um widget de texto cujo
/// conteúdo é exatamente `text`.
///
/// `Emulator::run` responde a um `Instruction::Expect` com exatamente um
/// `Event::Ready` (encontrou) ou `Event::Failed` (não encontrou), então este
/// laço sempre termina. Qualquer `Event::Action` que chegue antes disso (ex.:
/// uma mensagem recém-produzida pela `Subscription` do worker) é aplicado no
/// caminho — é aqui que o app de fato progride entre duas consultas.
fn screen_shows<P>(
    program: &P,
    emulator: &mut Emulator<P>,
    receiver: &mut Receiver<P>,
    text: &str,
) -> bool
where
    P: Program<State = Farol, Message = Message> + 'static,
{
    emulator.run(
        program,
        Instruction::Expect(Expectation::Text(text.to_string())),
    );

    loop {
        match next_event(receiver) {
            emulator::Event::Action(action) => emulator.perform(program, action),
            emulator::Event::Ready => return true,
            emulator::Event::Failed(_) => return false,
        }
    }
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

type Receiver<P> = mpsc::Receiver<emulator::Event<P>>;

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

    let observed = observe_plugin_state(fixture.spawn_config("uptime-kuma"));

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
#[test]
fn emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state() {
    let _guard = e2e_guard();
    let fixture = HarnessFixture::new("git-local-terminal");

    assert!(
        fixture.scan_root.join("exemplo").join(".git").is_dir(),
        "a fixture deveria conter um repositório git real"
    );

    match observe_plugin_state(fixture.spawn_config("git-local")) {
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
