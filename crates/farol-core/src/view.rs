//! Renderização condicionada ao `PluginState` (T027, T028, T029, T032,
//! T036, T039).
//!
//! **T015 (correção C2 parte 2)**: `Farol` deixou de ter uma única conexão
//! implícita e passou a ter `plugins: Vec<PluginSlot>` (main.rs) — `view()`
//! agora empilha uma seção por plugin conhecido, cada uma renderizada
//! exatamente como antes (a lógica de `view_ready`/`unavailable_message` não
//! mudou de comportamento, só passou a operar sobre um `&PluginConnection`
//! recebido por parâmetro em vez de `&self.plugin`).
//!
//! Vocabulário de `kind` de widget suportado nesta feature: só
//! `"status-grid"` (`widget-protocol.md`) — qualquer outro `kind` declarado
//! por um widget (ex.: `"monitor-status-grid"`, novo em v0.2 — D4 de
//! `specs/002-uptime-kuma-plugin/research.md`) é silenciosamente ignorado
//! (não derruba a UI, conforme o contrato); a renderização de
//! `"monitor-status-grid"` chega em T033-T035 (fora do escopo desta
//! subtarefa).

use farol_protocol::RemoteStatus;
// `Capability`/`KnownCapability` (novos em v0.2) ainda não estão na lista de re-exports de
// `crates/farol-protocol/src/lib.rs` — mesmo gap documentado em `plugin_worker.rs`, fora do escopo
// desta subtarefa corrigir (`farol-protocol` é off-limits). Referenciados via
// `farol_protocol::messages::*` (módulo e tipos ambos `pub`).
use farol_protocol::messages::{Capability, KnownCapability};
use iced::widget::{button, column, container, row, text, Column};
use iced::{Element, Length};

use crate::model::{self, PluginState, RepositoryViewModel, UnavailableReason};
use crate::{Farol, Message, PluginSlot};

/// `kind` de widget que esta versão do core sabe desenhar (T028).
const SUPPORTED_WIDGET_KIND: &str = "status-grid";

impl Farol {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut sections: Column<Message> = column![].spacing(24);
        for slot in &self.plugins {
            sections = sections.push(view_plugin_slot(slot));
        }

        container(sections.padding(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Renderiza a seção de um único plugin conhecido (T015) — mesmo `match`
/// por `PluginState` que existia antes desta feature para "a" conexão,
/// agora repetido por slot, com o nome do plugin como cabeçalho para
/// distinguir uma seção da outra na mesma janela.
fn view_plugin_slot(slot: &PluginSlot) -> Element<'_, Message> {
    let plugin_name = slot.spawn_config.plugin_name.as_str();

    let body: Column<Message> = match &slot.connection.state {
        PluginState::Starting => column![text("Iniciando plugin...")],
        PluginState::Handshaking => column![text("Aguardando handshake do plugin...")],
        // T029/T039: mensagem legível de indisponibilidade — UM ÚNICO
        // braço de `match` para TODO `UnavailableReason` (`FailedToStart`,
        // `VersionIncompatible`, `Crashed` de T038, `Unresponsive`,
        // `NotConfigured` de T019), garantindo que todos convergem para a
        // mesma categoria visual básica ("indisponível", distinta de
        // "carregando"/"sem dados"); só o texto interno
        // (`unavailable_message`) varia por motivo. `NotConfigured` é a
        // única variante não-terminal (model.rs) — a tela de setup que
        // substitui esta mensagem por um formulário chega em T029-T035
        // (fora do escopo desta subtarefa); por ora ela é exibida com o
        // mesmo tratamento textual das demais.
        PluginState::Unavailable { reason, detail } => {
            column![text(unavailable_message(reason, detail))]
        }
        PluginState::Ready => view_ready(plugin_name, &slot.connection),
    };

    column![text(plugin_name).size(18), body.spacing(12)]
        .spacing(8)
        .into()
}

/// Conteúdo exibido quando a conexão está `Ready`: manifesto de
/// capacidades (T027) + widget(s) declarado(s) (T028).
fn view_ready<'a>(plugin_name: &'a str, connection: &'a model::PluginConnection) -> Column<'a, Message> {
    let mut content = column![].spacing(8);

    if let Some(identity) = &connection.identity {
        content = content.push(text(format!(
            "Plugin: {} (protocolo {})",
            identity.plugin_name, identity.protocol_version
        )));
        // T027: manifesto de capacidades declarado pelo plugin,
        // consultável na UI — sem enforcement, só exibição (FR-008).
        content = content.push(text(format!(
            "Capacidades declaradas: {}",
            format_capabilities(&identity.capabilities.capabilities)
        )));
    }

    let renders_supported_widget = connection
        .widgets
        .iter()
        .any(|widget| widget.kind == SUPPORTED_WIDGET_KIND);

    if !renders_supported_widget {
        content = content.push(text(
            "Nenhum widget com um `kind` suportado por este core foi declarado.",
        ));
        return content;
    }

    if connection.items.is_empty() {
        content = content.push(text("Nenhum repositório encontrado no diretório varrido."));
    } else {
        content = content.push(view_repo_header());
        for item in &connection.items {
            content = content.push(view_repo_row(plugin_name, item));
        }
    }

    if let Some(error) = &connection.last_widget_error {
        content = content.push(text(format!(
            "Falha na última atualização do widget: {error}"
        )));
    }

    content
}

fn unavailable_message(reason: &UnavailableReason, detail: &str) -> String {
    let headline = match reason {
        UnavailableReason::FailedToStart => "Plugin indisponível — não foi possível iniciar",
        UnavailableReason::VersionIncompatible => {
            "Plugin indisponível — versão de protocolo incompatível"
        }
        UnavailableReason::Crashed => "Plugin indisponível — o processo encerrou inesperadamente",
        UnavailableReason::Unresponsive => "Plugin indisponível — não respondeu a tempo",
        // T019 (D8): não-terminal — mensagem provisória até a tela de setup
        // (T029-T035, fora do escopo desta subtarefa) substituir este ramo
        // por um formulário.
        UnavailableReason::NotConfigured => "Plugin aguardando configuração",
    };
    format!("{headline}\n{detail}")
}

/// T013 (correção H2): `Capability` deixou de ser `String` (agora um objeto
/// discriminado por `kind`, T008 de `farol-protocol`) — `view.rs:60-63`
/// antes desta correção fazia `.join(", ")` direto sobre `Vec<String>`, o
/// que só compilava com o formato antigo. Formata cada capacidade
/// individualmente para uma representação textual razoável (T034, exibição
/// da capacidade `network` do plugin `uptime-kuma`, generaliza este mesmo
/// mecanismo — fora do escopo desta subtarefa).
fn format_capabilities(capabilities: &[Capability]) -> String {
    if capabilities.is_empty() {
        return "(nenhuma)".to_string();
    }
    capabilities
        .iter()
        .map(format_capability)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_capability(capability: &Capability) -> String {
    match capability {
        Capability::Known(KnownCapability::Exec) => "exec".to_string(),
        Capability::Known(KnownCapability::Network { host, port }) => match port {
            Some(port) => format!("network({host}:{port})"),
            None => format!("network({host})"),
        },
        // Vocabulário de `kind` aberto por desenho (`protocol/SPEC.md` §10)
        // — um `kind` desconhecido é aceito e exibido genericamente, sem
        // interpretar seus campos extras.
        Capability::Unknown(unknown) => format!("{} (kind desconhecido)", unknown.kind),
    }
}

fn view_repo_header() -> Element<'static, Message> {
    row![
        text("Repositório").width(Length::FillPortion(2)),
        text("Working tree").width(Length::FillPortion(1)),
        text("Remoto").width(Length::FillPortion(2)),
        text("Ação").width(Length::FillPortion(1)),
    ]
    .spacing(8)
    .into()
}

/// Renderiza uma linha do widget `status-grid`: nome do repositório,
/// working tree suja/limpa (FR-013), estado do remoto — "sem remoto"
/// (FR-014) visualmente distinguível de "0 à frente / 0 atrás" porque vem de
/// uma variante diferente de `RemoteStatus` (nunca inferido por ausência de
/// campo) — e a ação de fetch (T032/T036). Quando a última invocação de
/// fetch para este repositório falhou, uma segunda linha com o erro é
/// exibida logo abaixo, sem substituir os dados do repositório já
/// conhecidos (FR-018).
fn view_repo_row<'a>(plugin_name: &'a str, item: &'a RepositoryViewModel) -> Element<'a, Message> {
    let repo = &item.repo;
    let dirty_label = if repo.dirty {
        "suja (mudanças pendentes)"
    } else {
        "limpa"
    };
    let remote_label = match &repo.remote_status {
        RemoteStatus::Tracked { ahead, behind } => format!("{ahead} à frente / {behind} atrás"),
        RemoteStatus::NoRemote => "sem remoto".to_string(),
    };

    let main_row = row![
        text(repo.name.clone()).width(Length::FillPortion(2)),
        text(dirty_label).width(Length::FillPortion(1)),
        text(remote_label).width(Length::FillPortion(2)),
        view_fetch_control(plugin_name, item),
    ]
    .spacing(8);

    match &item.last_error {
        Some(error) => column![main_row, text(format!("Falha no fetch: {error}"))]
            .spacing(2)
            .into(),
        None => main_row.into(),
    }
}

/// T032: botão de fetch por repositório, habilitado/desabilitado EXATAMENTE
/// como declarado pelo plugin em `fetch_action.enabled` — o core nunca
/// decide isso por conta própria (FR-015). T036: enquanto
/// `fetch_in_flight` é `true`, o botão é substituído por um indicador de
/// "em andamento" (nunca clicável nesse estado, mesmo que a última
/// declaração conhecida tenha `enabled: true`) — evita reentrância pela UI
/// antes da resposta anterior chegar (assíncrona, via `Message::Worker`,
/// sem travar a janela).
///
/// **T015**: `Message::FetchRequested` ganhou o campo `plugin_name` (uma
/// conexão por plugin conhecido agora, não mais implícita) — este `plugin_name`
/// é passado por `view_repo_row`/`view_ready`, propagado da seção que
/// renderizou este item.
fn view_fetch_control<'a>(plugin_name: &'a str, item: &'a RepositoryViewModel) -> Element<'a, Message> {
    if item.fetch_in_flight {
        return text("buscando...")
            .width(Length::FillPortion(1))
            .into();
    }

    let action = &item.fetch_action;
    let on_press = action.enabled.then(|| Message::FetchRequested {
        plugin_name: plugin_name.to_string(),
        action_id: action.id.clone(),
        target: action.target.clone(),
        timeout_hint_ms: action.timeout_hint_ms,
    });

    button(text(action.label.clone()))
        .width(Length::FillPortion(1))
        .on_press_maybe(on_press)
        .into()
}
