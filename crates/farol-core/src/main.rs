//! Ponto de entrada do `farol-core`: aplicação `iced` que abre exatamente
//! uma janela nativa ao iniciar (FR-001), conecta com os processos filhos
//! dos plugins conhecidos (`plugin_worker`), aplica as transições de estado
//! em `update.rs` e renderiza em `view.rs` (T020-T029; T014/T015 da feature
//! 002 generalizam para múltiplas conexões simultâneas).
//!
//! Fio condutor entre os módulos (ver docstring de cada um para detalhe):
//! - `model`: tipos de estado (`PluginState`, `PluginConnection`, ...).
//! - `plugin_worker`: spawn + I/O assíncrona do processo do plugin, isolado
//!   do ciclo `update`/`view` via `iced::Subscription` + canal (D5); T014
//!   generaliza comando/args por plugin conhecido (`PluginSpawnConfig`).
//! - `config_store`/`secrets_store`: leitura/escrita de `config.toml`/
//!   `secrets.toml` (D8 de `specs/002-uptime-kuma-plugin/research.md`) —
//!   consumidos por `plugin_worker` (T018) para injetar `required_config`
//!   como variável de ambiente no spawn de cada plugin.
//! - `update`: transições de `Farol`/`PluginConnection` a partir de `Message`.
//! - `view`: renderização condicionada a `PluginState`.

mod config_store;
#[cfg(test)]
mod e2e_tests;
mod install;
mod model;
mod plugin_worker;
mod plugin_manifest;
mod registry_index;
mod sandbox;
mod sandbox_network;
mod sandbox_seccomp;
mod secrets_store;
mod update;
mod view;
#[cfg(test)]
mod visual_snapshot_tests;

/// Título da janela do app — extraído para constante junto com [`program`]
/// (T002 da feature 003) para que o binário real e o `iced_test::Emulator`
/// construam exatamente o mesmo `Program`.
const WINDOW_TITLE: &str = "Farol";

fn main() -> iced::Result {
    handle_install_subcommand();
    program(Farol::default).run()
}

/// Intercepta `farol install <owner>/<repo>` (T014, feature 007, US2, D5 de
/// `specs/007-registry-instalacao-plugins-github/research.md`) antes de montar a aplicação
/// `iced` — sem a subcommand `install`, `std::env::args().get(1)` não é `Some("install")` e esta
/// função não faz nada, preservando o comportamento de hoje (abre a janela normalmente).
///
/// Formato de argumento inválido (zero ou mais de um `/`) MUST falhar imediatamente, sem tentar
/// rede (contrato "Fluxo de instalação" do contrato de manifesto/instalação).
fn handle_install_subcommand() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("install") {
        return;
    }

    let Some(owner_repo) = args.get(2) else {
        eprintln!("uso: farol install <owner>/<repo>");
        std::process::exit(1);
    };

    let mut parts = owner_repo.splitn(2, '/');
    let (owner, repo) = match (parts.next(), parts.next()) {
        (Some(owner), Some(repo)) if !owner.is_empty() && !repo.is_empty() && !repo.contains('/') => {
            (owner, repo)
        }
        _ => {
            eprintln!(
                "formato inválido: \"{owner_repo}\" — esperado exatamente um \"/\", ex. owner/repo"
            );
            std::process::exit(1);
        }
    };

    let (message_is_success, message, exit_code) = match install::run(owner, repo) {
        install::InstallOutcome::Installed { plugin_name, path } => (
            true,
            format!("plugin \"{plugin_name}\" instalado em {}", path.display()),
            0,
        ),
        install::InstallOutcome::NoRelease => (
            false,
            format!("nenhuma release encontrada em {owner}/{repo}"),
            1,
        ),
        install::InstallOutcome::DownloadFailed(detail) => {
            (false, format!("falha ao instalar {owner}/{repo}: {detail}"), 1)
        }
        install::InstallOutcome::ManifestInvalid(err) => (
            false,
            format!("manifesto de plugin inválido em {owner}/{repo}: {err:?}"),
            1,
        ),
        install::InstallOutcome::NameCollision(plugin_name) => (
            false,
            format!(
                "\"{plugin_name}\" colide com um plugin de referência já existente — instalação cancelada"
            ),
            1,
        ),
    };

    if message_is_success {
        println!("{message}");
    } else {
        eprintln!("{message}");
    }
    std::process::exit(exit_code);
}

/// Construção do `iced::Program` do Farol — **ponto único** de montagem da
/// aplicação (T002 da feature 003, `research.md` D5 Camada 1): chamada tanto
/// por [`main`] quanto pelos testes `iced_test` (`e2e_tests.rs`), para nunca
/// existirem dois caminhos de construção divergentes.
///
/// **Parâmetro `boot`**: o `Program` real da feature 002 sempre bootava
/// `Farol::default()`, que deriva os plugins de
/// `plugin_worker::known_plugins()` — caminhos **relativos** à raiz do repo
/// (`plugins/git-local/main.py`, ver docstring de `known_plugins`) e ambos os
/// plugins conhecidos ao mesmo tempo. Sob `cargo test` isso seria
/// não-determinístico duas vezes: o `cwd` do binário de teste é o diretório
/// do crate (não a raiz do repo), e o slot `uptime-kuma` leria o
/// `~/.config/farol` real da máquina. Parametrizar apenas o `boot` mantém
/// `update`/`view`/`subscription`/título idênticos entre binário e teste
/// (que é o ponto de T002) e deixa o teste apontar para uma fixture
/// hermética. `main()` passa `Farol::default` — **nenhuma mudança de
/// comportamento observável de `cargo run --bin farol`**.
///
/// **Migração `iced` 0.14 (achado N2 de `research.md`)**: `iced::application`
/// deixou de aceitar `(title, update, view)` e passou a ser
/// `(boot: impl BootFn, update, view)`, com o título virando o método
/// builder `.title(...)` — daí o `boot` ser hoje o primeiro argumento e não
/// mais um `&str`.
pub(crate) fn program(
    boot: impl Fn() -> Farol + 'static,
) -> iced::Application<impl iced::Program<State = Farol, Message = Message, Theme = iced::Theme>> {
    iced::application(boot, Farol::update, Farol::view)
        .title(WINDOW_TITLE)
        .subscription(Farol::subscription)
}

/// Estado raiz da aplicação iced (o `Model` do padrão Model-Update-View).
///
/// **T015 (correção C2 parte 2)**: antes desta feature, cobria uma única
/// conexão de plugin (`plugin: model::PluginConnection`) — o walking
/// skeleton não lidava com múltiplos plugins simultâneos. Generalizado para
/// uma coleção (`plugins: Vec<PluginSlot>`), uma entrada por plugin
/// conhecido (`plugin_worker::known_plugins()` — `git-local` + `uptime-kuma`,
/// registro fixo, T014), cada uma com sua própria máquina de estados e seu
/// próprio canal de entrada de worker.
pub(crate) struct Farol {
    plugins: Vec<PluginSlot>,
    /// Estado do formulário de instalação in-app por nome (feature 008, US4,
    /// T013) — não pertence a nenhum `PluginSlot` específico (a instalação
    /// ainda não tem um plugin conectado quando começa), por isso vive
    /// diretamente aqui, ao lado de `plugins`.
    install_form: model::InstallForm,
    /// Painel de detalhe genérico de um item de widget (feature 010, US2,
    /// T015) — `Some` enquanto o overlay de detalhe está aberto sobre a view
    /// principal (`view::view_detail_panel`), `None` no estado normal. Mesmo
    /// raciocínio de `install_form` acima para viver diretamente em `Farol`
    /// em vez de dentro de um `PluginSlot`: o painel não pertence a nenhuma
    /// conexão específica (ver `model::DetailPanelState`).
    detail_panel: Option<model::DetailPanelState>,
}

impl Default for Farol {
    /// Uma entrada por plugin conhecido mais plugin instalado descoberto
    /// (T014/T015; T009 da feature 007 — `plugin_worker::all_plugins()`
    /// soma `known_plugins()` com `discover_installed_plugins()`, filtrando
    /// colisão de nome), cada uma começando em `PluginState::Starting`
    /// (default de `model::PluginConnection`) e sem canal de worker ainda
    /// (`None` — só existe depois que a `Subscription` daquele plugin emitir
    /// `WorkerEvent::Ready`).
    fn default() -> Self {
        Self::with_plugins(plugin_worker::all_plugins())
    }
}

impl Farol {
    /// Estado inicial com um slot por `PluginSpawnConfig` informado, cada um
    /// em `PluginState::Starting` e sem canal de worker.
    ///
    /// Extraído de [`Farol::default`] (que passou a ser
    /// `Self::with_plugins(plugin_worker::known_plugins())`) em T002 da
    /// feature 003: os testes `iced_test` (`e2e_tests.rs`) precisam de um
    /// `Farol` com um único plugin, apontado para uma fixture hermética e por
    /// caminho absoluto, sem depender do registro fixo nem do `cwd` do
    /// processo de teste. O binário real continua passando exatamente
    /// `known_plugins()`.
    pub(crate) fn with_plugins(spawn_configs: Vec<plugin_worker::PluginSpawnConfig>) -> Self {
        Self {
            plugins: spawn_configs
                .into_iter()
                .map(|spawn_config| PluginSlot {
                    spawn_config,
                    connection: model::PluginConnection::default(),
                    worker_sender: None,
                })
                .collect(),
            install_form: model::InstallForm::default(),
            detail_panel: None,
        }
    }
}

/// Agrupa, para um único plugin conhecido (T014, `plugin_worker::PluginSpawnConfig`):
/// a configuração usada para spawná-lo, o estado de sua conexão
/// (`model::PluginConnection`) e o canal de entrada do seu worker — T015,
/// correção C2 parte 2. `spawn_config.plugin_name` é a chave usada por
/// `update.rs`/`view.rs` para encontrar a entrada correspondente a um
/// `Message` (todo `Message` que se refere a uma conexão específica carrega
/// o `plugin_name` dela, ver `Message` abaixo).
pub(crate) struct PluginSlot {
    spawn_config: plugin_worker::PluginSpawnConfig,
    connection: model::PluginConnection,
    /// `Sender` do canal de entrada do worker deste plugin, recebido via
    /// `plugin_worker::WorkerEvent::Ready`. `None` até o worker emitir esse
    /// evento (processo ainda não subiu, ou spawn falhou e nunca vai subir).
    worker_sender: Option<iced::futures::channel::mpsc::Sender<plugin_worker::WorkerInput>>,
}

/// Mensagens do ciclo `update` do iced — cobre os eventos do worker de
/// plugin, o tick do refresh periódico (T026) e o clique de "Fetch" numa
/// linha de repositório (T032/T033).
///
/// **T015**: cada variante que se refere a uma conexão específica ganhou um
/// campo `plugin_name` — antes desta feature havia só uma conexão possível
/// (implícita); agora `update.rs` precisa saber a qual das entradas de
/// `Farol::plugins` a mensagem se refere.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Um evento emitido pela `Subscription` do worker de um plugin.
    Worker {
        plugin_name: String,
        event: plugin_worker::WorkerEvent,
    },
    /// Tick do timer de refresh periódico (FR-011) de um plugin — só produz
    /// efeito (envio de `widget/get`) quando aquela conexão já está `Ready`.
    RefreshTick { plugin_name: String },
    /// Usuário clicou o botão de uma `ActionDeclaration` habilitada de um
    /// plugin (T032 originalmente só para "Fetch" de `git-local`) — só é
    /// produzida pela `view` quando `ActionDeclaration.enabled == true`
    /// (view.rs); o core nunca dispara isto por conta própria. Dispara
    /// `action/invoke` pelo worker daquele plugin (T033), sem bloquear a UI
    /// — a resposta chega depois, assíncrona, como `Message::Worker` (D5).
    ///
    /// **T019 (feature 004)**: renomeada do nome anterior, específico de
    /// `git.fetch` — o *shape* já era genérico (`ActionTarget` livre, sem
    /// nada específico de `git-local`), só o nome não era. Generalizado
    /// para não precisar de uma segunda variante quase idêntica quando
    /// `vpn.connect`/`vpn.disconnect` (feature 004, T031/T032) passarem a
    /// disparar `action/invoke` pelo mesmo mecanismo — puramente uma
    /// renomeação, sem mudança de comportamento para `git-local`.
    ActionInvokeRequested {
        plugin_name: String,
        action_id: String,
        target: farol_protocol::ActionTarget,
        timeout_hint_ms: Option<u64>,
    },
    /// T032 (D8): usuário editou um campo do formulário de setup de um
    /// plugin (`view_setup_form`, view.rs) — atualiza `SetupForm.fields`
    /// daquela conexão (`Farol::handle_setup_field_changed`, update.rs).
    SetupFieldChanged {
        plugin_name: String,
        field_name: String,
        value: String,
    },
    /// T032 (D8): usuário confirmou o formulário de setup de um plugin —
    /// persiste os valores em `config.toml`/`secrets.toml` e dispara a
    /// reconexão do worker (`Farol::handle_setup_submitted`, update.rs).
    SetupSubmitted { plugin_name: String },
    /// Feature 008, US4 (T014): usuário editou o campo de nome do formulário
    /// de instalação in-app (`view_install_form`, view.rs) — atualiza
    /// `Farol::install_form.name_input` (`Farol::handle_install_form_name_changed`,
    /// update.rs). Mesmo padrão de `SetupFieldChanged`, sem `plugin_name`
    /// porque este formulário não pertence a nenhuma conexão já existente.
    InstallFormNameChanged(String),
    /// Feature 008, US4 (T014): usuário confirmou o formulário de instalação
    /// in-app — dispara `install::run_by_name` em segundo plano
    /// (`tokio::task::spawn_blocking` dentro de um `iced::Task::perform`,
    /// `Farol::handle_install_by_name_submitted`, update.rs), sem bloquear a
    /// UI enquanto o download/build roda.
    InstallByNameSubmitted,
    /// Feature 008, US4 (T014): resultado (sucesso ou falha, já traduzido
    /// para texto legível — `InstallByNameOutcome`/`InstallOutcome` não são
    /// `Clone`, então não podem viajar direto num `Message`, que precisa
    /// ser) de uma instalação in-app disparada por `InstallByNameSubmitted`.
    /// `success == true` também dispara a descoberta de plugins recém-
    /// instalados (`Farol::add_newly_installed_plugin_slots`, update.rs),
    /// fazendo o novo plugin aparecer na lista sem reiniciar o app.
    InstallOutcomeReceived { success: bool, message: String },
    /// Feature 010, US2 (T016): usuário pediu para ver o detalhe de um item
    /// específico de um widget (ex.: o botão "Detalhe" de uma linha de
    /// container, `view_item_detail_control`, view.rs) — abre
    /// `Farol::detail_panel` (`Farol::handle_item_detail_requested`,
    /// update.rs). Mensagem genérica, reutilizável por qualquer `kind` de
    /// widget existente ou futuro (D4/D5 de `plan.md`): não é uma ação que
    /// muda estado do plugin, é puramente uma interação de UI local — nunca
    /// dispara `action/invoke`.
    ///
    /// `items` reaproveita `farol_protocol::messages::WidgetItems` como um
    /// vetor de exatamente um elemento (o item clicado) — ver a docstring de
    /// `model::DetailPanelState` para o raciocínio completo.
    ItemDetailRequested {
        plugin_name: String,
        items: farol_protocol::messages::WidgetItems,
    },
    /// Feature 010, US2 (T016): usuário fechou o painel de detalhe (botão
    /// "Fechar", `view_detail_panel`) — limpa `Farol::detail_panel`
    /// (`Farol::handle_item_detail_closed`, update.rs), sem afetar nenhum
    /// outro estado (FR-007: o painel principal volta ao estado anterior sem
    /// perda de estado dos demais widgets).
    ItemDetailClosed,
}
