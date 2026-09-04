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
mod sandbox;
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
    program(Farol::default).run()
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
}
