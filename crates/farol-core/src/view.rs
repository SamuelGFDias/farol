//! Renderização condicionada ao `PluginState` (T027, T028, T029, T032,
//! T036, T039).
//!
//! Vocabulário de `kind` de widget suportado nesta feature: só
//! `"status-grid"` (`widget-protocol.md`) — qualquer outro `kind` declarado
//! por um widget é silenciosamente ignorado (não derruba a UI), conforme o
//! contrato.

use farol_protocol::RemoteStatus;
use iced::widget::{button, column, container, row, text, Column};
use iced::{Element, Length};

use crate::model::{PluginState, RepositoryViewModel, UnavailableReason};
use crate::{Farol, Message};

/// `kind` de widget que esta versão do core sabe desenhar (T028).
const SUPPORTED_WIDGET_KIND: &str = "status-grid";

impl Farol {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let content: Column<Message> = match &self.plugin.state {
            PluginState::Starting => column![text("Iniciando plugin...")],
            PluginState::Handshaking => column![text("Aguardando handshake do plugin...")],
            // T029/T039: mensagem legível de indisponibilidade — UM ÚNICO
            // braço de `match` para TODO `UnavailableReason` (`FailedToStart`,
            // `VersionIncompatible`, `Crashed` de T038, `Unresponsive`),
            // garantindo que todos convergem para a mesma categoria visual
            // básica ("indisponível", distinta de "carregando"/"sem dados");
            // só o texto interno (`unavailable_message`) varia por motivo.
            // Nenhum widget é renderizado neste ramo, só a mensagem — mas
            // (T040) isso não impede o resto da janela/`update` de
            // continuar respondendo, já que nada aqui bloqueia ou espera o
            // plugin.
            PluginState::Unavailable { reason, detail } => {
                column![text(unavailable_message(reason, detail))]
            }
            PluginState::Ready => self.view_ready(),
        };

        container(content.spacing(12).padding(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Conteúdo exibido quando a conexão está `Ready`: manifesto de
    /// capacidades (T027) + widget(s) declarado(s) (T028).
    fn view_ready(&self) -> Column<'_, Message> {
        let mut content = column![text("Farol").size(24)].spacing(8);

        if let Some(identity) = &self.plugin.identity {
            content = content.push(text(format!(
                "Plugin: {} (protocolo {})",
                identity.plugin_name, identity.protocol_version
            )));
            // T027: manifesto de capacidades declarado pelo plugin,
            // consultável na UI — sem enforcement, só exibição (FR-008).
            content = content.push(text(format!(
                "Capacidades declaradas: {}",
                if identity.capabilities.capabilities.is_empty() {
                    "(nenhuma)".to_string()
                } else {
                    identity.capabilities.capabilities.join(", ")
                }
            )));
        }

        let renders_supported_widget = self
            .plugin
            .widgets
            .iter()
            .any(|widget| widget.kind == SUPPORTED_WIDGET_KIND);

        if !renders_supported_widget {
            content = content.push(text(
                "Nenhum widget com um `kind` suportado por este core foi declarado.",
            ));
            return content;
        }

        if self.plugin.items.is_empty() {
            content = content.push(text("Nenhum repositório encontrado no diretório varrido."));
        } else {
            content = content.push(view_repo_header());
            for item in &self.plugin.items {
                content = content.push(view_repo_row(item));
            }
        }

        if let Some(error) = &self.plugin.last_widget_error {
            content = content.push(text(format!(
                "Falha na última atualização do widget: {error}"
            )));
        }

        content
    }
}

fn unavailable_message(reason: &UnavailableReason, detail: &str) -> String {
    let headline = match reason {
        UnavailableReason::FailedToStart => "Plugin indisponível — não foi possível iniciar",
        UnavailableReason::VersionIncompatible => {
            "Plugin indisponível — versão de protocolo incompatível"
        }
        UnavailableReason::Crashed => "Plugin indisponível — o processo encerrou inesperadamente",
        UnavailableReason::Unresponsive => "Plugin indisponível — não respondeu a tempo",
    };
    format!("{headline}\n{detail}")
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
fn view_repo_row(item: &RepositoryViewModel) -> Element<'_, Message> {
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
        view_fetch_control(item),
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
fn view_fetch_control(item: &RepositoryViewModel) -> Element<'_, Message> {
    if item.fetch_in_flight {
        return text("buscando...")
            .width(Length::FillPortion(1))
            .into();
    }

    let action = &item.fetch_action;
    let on_press = action.enabled.then(|| Message::FetchRequested {
        action_id: action.id.clone(),
        target: action.target.clone(),
        timeout_hint_ms: action.timeout_hint_ms,
    });

    button(text(action.label.clone()))
        .width(Length::FillPortion(1))
        .on_press_maybe(on_press)
        .into()
}
