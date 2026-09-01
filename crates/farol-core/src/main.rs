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
mod model;
mod plugin_worker;
mod secrets_store;
mod update;
mod view;

fn main() -> iced::Result {
    iced::application("Farol", Farol::update, Farol::view)
        .subscription(Farol::subscription)
        .run()
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
    /// Uma entrada por plugin conhecido (T014/T015), cada uma começando em
    /// `PluginState::Starting` (default de `model::PluginConnection`) e sem
    /// canal de worker ainda (`None` — só existe depois que a `Subscription`
    /// daquele plugin emitir `WorkerEvent::Ready`).
    fn default() -> Self {
        Self {
            plugins: plugin_worker::known_plugins()
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
    /// Usuário clicou "Fetch" para um repositório de um plugin (T032) — só é
    /// produzida pela `view` quando `ActionDeclaration.enabled == true`
    /// (view.rs); o core nunca dispara isto por conta própria. Dispara
    /// `action/invoke` pelo worker daquele plugin (T033), sem bloquear a UI
    /// — a resposta chega depois, assíncrona, como `Message::Worker` (D5).
    FetchRequested {
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
