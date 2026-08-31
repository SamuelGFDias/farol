//! Ponto de entrada do `farol-core`: aplicação `iced` que abre exatamente
//! uma janela nativa ao iniciar (FR-001), conecta com o processo filho de
//! um plugin (`plugin_worker`), aplica as transições de estado em
//! `update.rs` e renderiza em `view.rs` (T020-T029).
//!
//! Fio condutor entre os módulos (ver docstring de cada um para detalhe):
//! - `model`: tipos de estado (`PluginState`, `PluginConnection`, ...).
//! - `plugin_worker`: spawn + I/O assíncrona do processo do plugin, isolado
//!   do ciclo `update`/`view` via `iced::Subscription` + canal (D5).
//! - `update`: transições de `Farol`/`PluginConnection` a partir de `Message`.
//! - `view`: renderização condicionada a `PluginState`.

mod model;
mod plugin_worker;
mod update;
mod view;

fn main() -> iced::Result {
    iced::application("Farol", Farol::update, Farol::view)
        .subscription(Farol::subscription)
        .run()
}

/// Estado raiz da aplicação iced (o `Model` do padrão Model-Update-View).
///
/// Nesta subtarefa cobre uma única conexão de plugin (`plugin`) — o
/// walking skeleton não lida com múltiplos plugins simultâneos.
#[derive(Default)]
pub(crate) struct Farol {
    /// Estado da conexão com o (único, nesta feature) plugin.
    plugin: model::PluginConnection,
    /// `Sender` do canal de entrada do worker, recebido via
    /// `plugin_worker::WorkerEvent::Ready`. `None` até o worker emitir esse
    /// evento (processo ainda não subiu, ou spawn falhou e nunca vai subir).
    worker_sender: Option<iced::futures::channel::mpsc::Sender<plugin_worker::WorkerInput>>,
}

/// Mensagens do ciclo `update` do iced — cobre os eventos do worker de
/// plugin, o tick do refresh periódico (T026) e o clique de "Fetch" numa
/// linha de repositório (T032/T033).
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Um evento emitido pela `Subscription` do worker do plugin.
    Worker(plugin_worker::WorkerEvent),
    /// Tick do timer de refresh periódico (FR-011) — só produz efeito
    /// (envio de `widget/get`) quando a conexão já está `Ready`.
    RefreshTick,
    /// Usuário clicou "Fetch" para um repositório (T032) — só é produzida
    /// pela `view` quando `ActionDeclaration.enabled == true` (view.rs); o
    /// core nunca dispara isto por conta própria. Dispara `action/invoke`
    /// pelo worker (T033), sem bloquear a UI — a resposta chega depois,
    /// assíncrona, como `Message::Worker` (D5).
    FetchRequested {
        action_id: String,
        target: farol_protocol::ActionTarget,
        timeout_hint_ms: Option<u64>,
    },
}
