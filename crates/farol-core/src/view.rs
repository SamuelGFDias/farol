//! Renderização condicionada ao `PluginState` (T027, T028, T029, T032,
//! T033, T034, T035, T036, T039).
//!
//! **T015 (correção C2 parte 2)**: `Farol` deixou de ter uma única conexão
//! implícita e passou a ter `plugins: Vec<PluginSlot>` (main.rs) — `view()`
//! agora empilha uma seção por plugin conhecido, cada uma renderizada
//! exatamente como antes (a lógica de `view_ready`/`unavailable_message` não
//! mudou de comportamento, só passou a operar sobre um `&PluginConnection`
//! recebido por parâmetro em vez de `&self.plugin`).
//!
//! Vocabulário de `kind` de widget suportado por este core: `"status-grid"`
//! (`widget-protocol.md`, `git-local`) e, desde T033, `"monitor-status-grid"`
//! (novo em v0.2 — D4 de `specs/002-uptime-kuma-plugin/research.md`,
//! `uptime-kuma`). Qualquer outro `kind` declarado por um widget continua
//! sendo silenciosamente ignorado (não derruba a UI, conforme o contrato).

use farol_protocol::RemoteStatus;
// `Capability`/`KnownCapability`/`MonitorStatus` (novos em v0.2) ainda não estão na lista de
// re-exports de `crates/farol-protocol/src/lib.rs` — mesmo gap documentado em `plugin_worker.rs`,
// fora do escopo desta subtarefa corrigir (`farol-protocol` é off-limits). Referenciados via
// `farol_protocol::messages::*` (módulo e tipos ambos `pub`).
use farol_protocol::messages::{
    Capability, ContainerState, KnownCapability, MonitorStatus, VpnConnectionState,
};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{Element, Length};

use crate::model::{self, PluginState, RepositoryViewModel, UnavailableReason};
use crate::{Farol, Message, PluginSlot};

/// `kind` de widget `status-grid` (`git-local`, T028).
const SUPPORTED_WIDGET_KIND: &str = "status-grid";
/// `kind` de widget `monitor-status-grid` (`uptime-kuma`, T033, D4 de
/// `specs/002-uptime-kuma-plugin/research.md`).
const MONITOR_WIDGET_KIND: &str = "monitor-status-grid";
/// `kind` de widget `vpn-status` (`openfortivpn-vpn`, T025, novo em v0.3 —
/// `specs/004-vpn-status-plugin/data-model.md` §1.7/`research.md` D3).
const VPN_WIDGET_KIND: &str = "vpn-status";
/// `kind` de widget `container-status-grid` (`docker-containers`, T025, novo em v0.4 —
/// `specs/005-docker-containers-plugin/data-model.md` §1.3). Mesmo literal de
/// `update::CONTAINER_WIDGET_KIND` (`update.rs` não é reutilizável aqui — cada módulo declara sua
/// própria constante para o mesmo `kind`, mesmo padrão já usado por `SUPPORTED_WIDGET_KIND`/
/// `MONITOR_WIDGET_KIND`/`VPN_WIDGET_KIND` acima).
const CONTAINER_WIDGET_KIND: &str = "container-status-grid";

impl Farol {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut sections: Column<Message> = column![].spacing(24);
        // Feature 008, US4 (T015): formulário de instalação in-app sempre
        // visível, acima das seções por plugin — não pertence a nenhum
        // `PluginSlot` (`view_install_form` não recebe um), mesmo raciocínio
        // de `Farol::install_form` viver diretamente em `Farol` (main.rs).
        sections = sections.push(view_install_form(&self.install_form));
        for slot in &self.plugins {
            sections = sections.push(view_plugin_slot(slot));
        }

        container(sections.padding(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Feature 008, US4 (T015): formulário de instalação in-app por nome de
/// plugin do índice central — reaproveita o padrão visual de
/// `view_setup_form` (campo de texto + botão de confirmar, coluna com
/// espaçamento igual), mas sem `plugin_name`/loop de itens (um único campo
/// fixo, "nome", em vez de um por `RequiredConfigItem`).
///
/// Botão desabilitado (`on_press_maybe(None)`) enquanto
/// `InstallFormStatus::InProgress` — mesma disciplina de
/// `view_vpn_widget`/`view_container_grid` para não deixar a UI disparar uma
/// segunda instalação sobreposta à primeira antes da resposta do
/// `Task::perform` em voo chegar.
fn view_install_form(form: &model::InstallForm) -> Column<'_, Message> {
    let mut content: Column<Message> = column![text("Instalar plugin").size(18)].spacing(8);

    let in_progress = matches!(form.status, model::InstallFormStatus::InProgress);

    let field = text_input("nome do plugin (índice)", &form.name_input)
        .width(Length::Fill)
        .on_input(Message::InstallFormNameChanged);
    content = content.push(field);

    let on_press = (!in_progress).then_some(Message::InstallByNameSubmitted);
    content = content.push(button(text("Instalar")).on_press_maybe(on_press));

    match &form.status {
        model::InstallFormStatus::Idle => {}
        model::InstallFormStatus::InProgress => content = content.push(text("Instalando...")),
        model::InstallFormStatus::Success(message) => content = content.push(text(message.clone())),
        model::InstallFormStatus::Error(message) => content = content.push(text(message.clone())),
    }

    content
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
        // T035: `NotConfigured` é a única variante não-terminal de
        // `UnavailableReason` (model.rs) — em vez da mensagem textual
        // genérica das demais, renderiza o formulário de setup construído
        // por `update::handle_handshake_outcome` (T030) a partir do
        // `required_config` deste handshake.
        PluginState::Unavailable {
            reason: UnavailableReason::NotConfigured,
            detail,
        } => match &slot.connection.setup_form {
            Some(form) => view_setup_form(plugin_name, form),
            // Defensivo: `NotConfigured` sem `setup_form` já montado não
            // deveria ocorrer em uso normal (`handle_handshake_outcome`
            // sempre constrói os dois juntos) — cai de volta na mensagem
            // genérica em vez de uma tela vazia.
            None => column![text(unavailable_message(
                &UnavailableReason::NotConfigured,
                detail
            ))],
        },
        // T029/T039: mensagem legível de indisponibilidade — UM ÚNICO
        // braço de `match` para as quatro variantes TERMINAIS de
        // `UnavailableReason` (`FailedToStart`, `VersionIncompatible`,
        // `Crashed` de T038, `Unresponsive`), garantindo que todas
        // convergem para a mesma categoria visual básica ("indisponível",
        // distinta de "carregando"/"sem dados"); só o texto interno
        // (`unavailable_message`) varia por motivo. `NotConfigured` tem seu
        // próprio braço acima (T035), por ser a única variante não-terminal.
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
/// capacidades (T027/T034) + widget(s) declarado(s) (T028/T033).
///
/// **T034**: a capacidade `network` (host/porta) do plugin `uptime-kuma` já
/// é exibida pelo bloco de capacidades abaixo, sem nenhuma lógica dedicada
/// — `format_capability` (T013) já trata `KnownCapability::Network` do
/// mesmo jeito declarativo que já tratava `KnownCapability::Exec` na
/// feature 001, e este bloco roda para qualquer plugin `Ready`,
/// independente do `kind` de widget que ele declara.
fn view_ready<'a>(
    plugin_name: &'a str,
    connection: &'a model::PluginConnection,
) -> Column<'a, Message> {
    let mut content = column![].spacing(8);

    if let Some(identity) = &connection.identity {
        content = content.push(text(format!(
            "Plugin: {} (protocolo {})",
            identity.plugin_name, identity.protocol_version
        )));
        // T027/T034: manifesto de capacidades declarado pelo plugin,
        // consultável na UI — sem enforcement, só exibição (FR-005/FR-006/
        // FR-008).
        content = content.push(text(format!(
            "Capacidades declaradas: {}",
            format_capabilities(&identity.capabilities.capabilities)
        )));
    }

    let renders_status_grid = connection
        .widgets
        .iter()
        .any(|widget| widget.kind == SUPPORTED_WIDGET_KIND);
    let renders_monitor_grid = connection
        .widgets
        .iter()
        .any(|widget| widget.kind == MONITOR_WIDGET_KIND);
    let renders_vpn_widget = connection
        .widgets
        .iter()
        .any(|widget| widget.kind == VPN_WIDGET_KIND);
    let renders_container_grid = connection
        .widgets
        .iter()
        .any(|widget| widget.kind == CONTAINER_WIDGET_KIND);

    if renders_status_grid {
        content = view_status_grid(plugin_name, connection, content);
    }
    if renders_monitor_grid {
        content = view_monitor_grid(&connection.monitor_widget, content);
    }
    if renders_vpn_widget {
        content = view_vpn_widget(plugin_name, &connection.vpn_widget, content);
    }
    if renders_container_grid {
        content = view_container_grid(plugin_name, &connection.docker_widget, content);
    }
    if !renders_status_grid
        && !renders_monitor_grid
        && !renders_vpn_widget
        && !renders_container_grid
    {
        content = content.push(text(
            "Nenhum widget com um `kind` suportado por este core foi declarado.",
        ));
    }

    content
}

/// Widget `status-grid` (`git-local`, T028) — extraído de `view_ready` em
/// T033 para dar lugar, no mesmo `match` conceitual, ao widget
/// `monitor-status-grid` (`view_monitor_grid`, T033).
fn view_status_grid<'a>(
    plugin_name: &'a str,
    connection: &'a model::PluginConnection,
    mut content: Column<'a, Message>,
) -> Column<'a, Message> {
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

/// T033: widget `monitor-status-grid` (`uptime-kuma`) — lista de monitores
/// (nome, status `up`/`down`/`pending`/`maintenance`, tempo de resposta
/// quando aplicável) a partir de `MonitorWidgetViewModel` (T029/T031).
///
/// `last_error` (incluindo o safeguard `not_configured` de `widget/get`,
/// `contracts/error-model-delta.md`) MUST renderizar um estado explícito,
/// visivelmente distinto de "0 monitores" (FR-008, FR-013, FR-014, SC-001,
/// SC-005): por isso a mensagem "nenhum monitor cadastrado" só aparece
/// quando NÃO há erro pendente — um erro com lista vazia mostra só o erro
/// (nunca os dois textos juntos, que pareceriam contraditórios); um erro
/// com monitores preservados de uma leitura anterior boa (`data-model.md`
/// §3.1: erro pontual não apaga `monitors`, FR-017) mostra o erro em cima
/// da última lista boa conhecida, em vez de escondê-la.
fn view_monitor_grid<'a>(
    widget: &'a model::MonitorWidgetViewModel,
    mut content: Column<'a, Message>,
) -> Column<'a, Message> {
    if let Some(error) = &widget.last_error {
        content = content.push(text(format!("Falha ao consultar monitores: {error}")));
    }

    if !widget.monitors.is_empty() {
        content = content.push(view_monitor_header());
        for monitor in &widget.monitors {
            content = content.push(view_monitor_row(monitor));
        }
    } else if widget.last_error.is_none() {
        content = content.push(text("Nenhum monitor cadastrado nesta instância."));
    }

    content
}

fn view_monitor_header() -> Element<'static, Message> {
    row![
        text("Monitor").width(Length::FillPortion(2)),
        text("Status").width(Length::FillPortion(1)),
        text("Tempo de resposta").width(Length::FillPortion(1)),
    ]
    .spacing(8)
    .into()
}

/// Renderiza uma linha do widget `monitor-status-grid`: nome do monitor,
/// status mapeado (T033, FR-012) e tempo de resposta quando aplicável —
/// `response_time_ms: None` (`data-model.md` §1.3: nullable explícito,
/// nunca ausente) é exibido como "—", nunca inferido como "0 ms".
fn view_monitor_row(monitor: &farol_protocol::messages::MonitorStatusItem) -> Element<'_, Message> {
    let status_label = match monitor.status {
        MonitorStatus::Up => "up",
        MonitorStatus::Down => "down",
        MonitorStatus::Pending => "pending",
        MonitorStatus::Maintenance => "maintenance",
    };
    let response_label = match monitor.response_time_ms {
        Some(ms) => format!("{ms} ms"),
        None => "—".to_string(),
    };

    row![
        text(monitor.name.clone()).width(Length::FillPortion(2)),
        text(status_label).width(Length::FillPortion(1)),
        text(response_label).width(Length::FillPortion(1)),
    ]
    .spacing(8)
    .into()
}

/// T025/T032: widget `vpn-status` (`openfortivpn-vpn`). T032 (US2) estende a
/// renderização somente-leitura de T025 com controles de ação: um botão de
/// "conectar" por perfil disponível (`VpnProfile::connect_action`) e um
/// botão de "desconectar" quando `state == Connected`
/// (`VpnStatusItem::disconnect_action`) — mesmo padrão de
/// `view_fetch_control`/`Message::ActionInvokeRequested` já usado por
/// `git.fetch`: o core nunca decide `enabled` por conta própria, só
/// desabilita adicionalmente enquanto uma invocação já está em andamento
/// (`connect_in_flight`/`disconnect_in_flight`, D7 de `research.md`) para
/// evitar reentrância pela UI antes da resposta anterior chegar.
///
/// Mesmo espírito de `view_monitor_grid` para `last_error` (`FR-017`): um
/// erro pontual do último `widget/get` não apaga o `status` de uma leitura
/// anterior boa — o erro é exibido em cima do que já se sabe, nunca no
/// lugar. `last_action_error` (erro da última `vpn.connect`/
/// `vpn.disconnect`) segue a mesma regra, exibido sem esconder `status`/
/// lista de perfis já mostrados.
///
/// `elapsed_seconds` deliberadamente NÃO é exibido aqui — escopo de T034
/// (User Story 3); o campo já está populado no `Model` desde T021, só a
/// exibição fica para depois.
fn view_vpn_widget<'a>(
    plugin_name: &'a str,
    widget: &'a model::VpnWidgetViewModel,
    mut content: Column<'a, Message>,
) -> Column<'a, Message> {
    if let Some(error) = &widget.last_error {
        content = content.push(text(format!("Falha ao consultar VPN: {error}")));
    }

    let action_in_flight = widget.connect_in_flight || widget.disconnect_in_flight;

    match &widget.status {
        Some(item) => {
            let state_label = match item.state {
                VpnConnectionState::Disconnected => "desconectado",
                VpnConnectionState::Connecting => "conectando",
                VpnConnectionState::Connected => "conectado",
            };
            content = content.push(text(format!("Estado: {state_label}")));

            if item.state == VpnConnectionState::Connected {
                content = content.push(text(format!(
                    "Perfil ativo: {}",
                    item.active_profile.as_deref().unwrap_or("—")
                )));
                // T034 (US3): tempo de sessão decorrido, formatado de forma
                // legível. `data-model.md` §1.3 garante `elapsed_seconds`
                // presente quando `state == Connected`, mas tratamos `None`
                // defensivamente sem quebrar a renderização.
                content = content.push(text(format!(
                    "Conectado há: {}",
                    item.elapsed_seconds
                        .map(format_elapsed)
                        .unwrap_or_else(|| "—".to_string())
                )));
                content = content.push(view_vpn_disconnect_control(
                    plugin_name,
                    item,
                    action_in_flight,
                ));
            }

            if item.available_profiles.is_empty() {
                content = content.push(text("Nenhum perfil VPN configurado."));
            } else {
                for profile in &item.available_profiles {
                    content =
                        content.push(view_vpn_profile_row(plugin_name, profile, action_in_flight));
                }
            }
        }
        None => {
            if widget.last_error.is_none() {
                content = content.push(text("Aguardando primeira leitura do estado da VPN..."));
            }
        }
    }

    // D7 de `research.md`: feedback textual de "em andamento" enquanto a
    // resposta de `action/invoke` ainda não chegou — os botões já ficam
    // desabilitados (`action_in_flight` acima), mas um texto explícito
    // deixa claro qual ação está pendente.
    if widget.connect_in_flight {
        content = content.push(text("Conectando..."));
    }
    if widget.disconnect_in_flight {
        content = content.push(text("Desconectando..."));
    }

    if let Some(error) = &widget.last_action_error {
        content = content.push(text(format!("Falha na última ação: {error}")));
    }

    content
}

/// T034: formata uma duração em segundos como texto legível de
/// horas/minutos/segundos (ex.: `3725.0` -> `"1h 2min"`, `125.0` ->
/// `"2min 5s"`, `45.0` -> `"45s"`). Sem formato exato exigido pela spec —
/// só precisa ser claramente legível como duração.
fn format_elapsed(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0).round() as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}min")
    } else if minutes > 0 {
        format!("{minutes}min {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// T032: uma linha por `VpnProfile` — nome + botão de conectar
/// (`connect_action.label`), habilitado quando `connect_action.enabled ==
/// true` e nenhuma ação (`connect`/`disconnect`) já está em andamento.
/// Mesmo padrão exato de `view_fetch_control` para construir
/// `Message::ActionInvokeRequested`.
fn view_vpn_profile_row<'a>(
    plugin_name: &'a str,
    profile: &'a farol_protocol::messages::VpnProfile,
    action_in_flight: bool,
) -> Element<'a, Message> {
    let action = &profile.connect_action;
    let on_press = (action.enabled && !action_in_flight).then(|| Message::ActionInvokeRequested {
        plugin_name: plugin_name.to_string(),
        action_id: action.id.clone(),
        target: action.target.clone(),
        timeout_hint_ms: action.timeout_hint_ms,
    });

    row![
        text(profile.name.clone()).width(Length::FillPortion(2)),
        button(text(action.label.clone()))
            .width(Length::FillPortion(1))
            .on_press_maybe(on_press),
    ]
    .spacing(8)
    .into()
}

/// T032: botão de desconectar (`VpnStatusItem::disconnect_action`),
/// exibido só quando `state == Connected` (chamador já garante isso) —
/// habilitado quando `disconnect_action.enabled == true` e nenhuma ação já
/// está em andamento.
fn view_vpn_disconnect_control<'a>(
    plugin_name: &'a str,
    item: &'a farol_protocol::messages::VpnStatusItem,
    action_in_flight: bool,
) -> Element<'a, Message> {
    let action = &item.disconnect_action;
    let on_press = (action.enabled && !action_in_flight).then(|| Message::ActionInvokeRequested {
        plugin_name: plugin_name.to_string(),
        action_id: action.id.clone(),
        target: action.target.clone(),
        timeout_hint_ms: action.timeout_hint_ms,
    });

    button(text(action.label.clone()))
        .on_press_maybe(on_press)
        .into()
}

/// T025: widget `container-status-grid` (`docker-containers`) — renderização SOMENTE LEITURA
/// (`specs/005-docker-containers-plugin/tasks.md` T025): sem botões de iniciar/parar/reiniciar,
/// que são escopo de US2/T032. Mesmo espírito de `view_monitor_grid` para `last_error` (FR-006):
/// um erro pontual do último `widget/get` não apaga `containers` de uma leitura anterior boa — o
/// erro é exibido em cima do que já se sabe, nunca no lugar. `widget.loaded` distingue "ainda não
/// li nada" de "li com sucesso e não há nenhum container" (FR-011) — sem ele, `containers.is_empty()`
/// seria ambíguo entre os dois casos.
fn view_container_grid<'a>(
    plugin_name: &'a str,
    widget: &'a model::DockerWidgetViewModel,
    mut content: Column<'a, Message>,
) -> Column<'a, Message> {
    if let Some(error) = &widget.last_error {
        content = content.push(text(format!("Falha ao consultar containers: {error}")));
    }

    if !widget.containers.is_empty() {
        content = content.push(view_container_header());
        for container in &widget.containers {
            content = content.push(view_container_row(plugin_name, container));
        }
    } else if !widget.loaded {
        content = content.push(text("Aguardando primeira leitura dos containers..."));
    } else if widget.last_error.is_none() {
        content = content.push(text("Nenhum container encontrado."));
    }

    content
}

fn view_container_header() -> Element<'static, Message> {
    row![
        text("Nome").width(Length::FillPortion(2)),
        text("Imagem").width(Length::FillPortion(2)),
        text("Estado").width(Length::FillPortion(1)),
    ]
    .spacing(8)
    .into()
}

/// Renderiza uma linha do widget `container-status-grid`: nome, imagem, estado mapeado para
/// PT-BR (FR-003, `spec.md` §Requisitos Funcionais — vocabulário de estado usado na matriz de
/// ações) e, desde T032 (US2), os três botões de ação (`start_action`/`stop_action`/
/// `restart_action`) — mesmo padrão exato de `view_vpn_profile_row`/`view_vpn_disconnect_control`
/// para construir `Message::ActionInvokeRequested`: `on_press` só quando `action.enabled == true`
/// **e** `container.action_in_flight == None` — o core pode recusar o que o plugin habilitou
/// (`ActionDeclaration::enabled`), mas nunca habilitar o que o plugin desabilitou (`research.md`
/// D7, Princípio III). Enquanto uma ação está em andamento (`action_in_flight.is_some()`), um
/// texto curto indica qual ("Iniciando..."/"Parando..."/"Reiniciando...") sem esconder o resto da
/// linha; `last_action_error`, quando presente, aparece como texto adicional pela mesma razão —
/// nome/imagem/estado continuam visíveis (mesmo espírito de `last_error` do widget inteiro, FR-006).
fn view_container_row<'a>(
    plugin_name: &'a str,
    container: &'a model::ContainerViewModel,
) -> Element<'a, Message> {
    let item = &container.item;
    let state_label = match item.state {
        ContainerState::Created => "criado",
        ContainerState::Running => "rodando",
        ContainerState::Restarting => "reiniciando",
        ContainerState::Paused => "pausado",
        ContainerState::Removing => "em remoção",
        ContainerState::Exited => "parado",
        ContainerState::Dead => "morto",
        ContainerState::Unknown => "desconhecido",
    };

    let action_in_flight = container.action_in_flight.is_some();

    let mut line: Column<Message> = column![row![
        text(item.name.clone()).width(Length::FillPortion(2)),
        text(item.image.clone()).width(Length::FillPortion(2)),
        text(state_label).width(Length::FillPortion(1)),
        view_container_action_control(plugin_name, &item.start_action, action_in_flight),
        view_container_action_control(plugin_name, &item.stop_action, action_in_flight),
        view_container_action_control(plugin_name, &item.restart_action, action_in_flight),
    ]
    .spacing(8)]
    .spacing(4);

    if let Some(kind) = container.action_in_flight {
        let in_flight_label = match kind {
            model::ContainerActionKind::Start => "Iniciando...",
            model::ContainerActionKind::Stop => "Parando...",
            model::ContainerActionKind::Restart => "Reiniciando...",
        };
        line = line.push(text(in_flight_label));
    }

    if let Some(error) = &container.last_action_error {
        line = line.push(text(format!("Falha: {error}")));
    }

    line.into()
}

/// T032: um botão de ação de container (`start_action`/`stop_action`/`restart_action`) —
/// habilitado quando `action.enabled == true` e nenhuma ação já está em andamento **para este
/// container** (`action_in_flight`, restrição adicional do core, nunca uma habilitação por conta
/// própria). Mesmo padrão exato de `view_vpn_disconnect_control`.
fn view_container_action_control<'a>(
    plugin_name: &'a str,
    action: &'a farol_protocol::messages::ActionDeclaration,
    action_in_flight: bool,
) -> Element<'a, Message> {
    let on_press = (action.enabled && !action_in_flight).then(|| Message::ActionInvokeRequested {
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

/// T035 (D8): renderiza o formulário de setup (`SetupForm`, T030) — em vez
/// do widget normal do plugin — quando
/// `PluginState::Unavailable{reason: NotConfigured, ..}`. Um campo de texto
/// por item de `required_config` (mascarado quando `secret: true`, via
/// `TextInput::secure`), rótulo = `description`, botão de confirmar.
fn view_setup_form<'a>(plugin_name: &'a str, form: &'a model::SetupForm) -> Column<'a, Message> {
    let mut content: Column<Message> = column![text(format!(
        "Plugin \"{plugin_name}\" aguardando configuração — preencha os campos abaixo."
    ))]
    .spacing(8);

    for (item, current_value) in &form.fields {
        let field_name = item.name.clone();
        let owned_plugin_name = plugin_name.to_string();
        let field = text_input(&item.description, current_value)
            .secure(item.secret)
            .width(Length::Fill)
            .on_input(move |value| Message::SetupFieldChanged {
                plugin_name: owned_plugin_name.clone(),
                field_name: field_name.clone(),
                value,
            });
        content = content.push(column![text(item.description.clone()), field].spacing(2));
    }

    let confirm = Some(Message::SetupSubmitted {
        plugin_name: plugin_name.to_string(),
    });
    content = content.push(button(text("Confirmar")).on_press_maybe(confirm));

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
/// (FR-014) e "sem tracking configurado" (débito técnico #3 / issue #3)
/// visualmente distinguíveis entre si e de "0 à frente / 0 atrás" porque vêm
/// de variantes diferentes de `RemoteStatus` (nunca inferido por ausência de
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
        // Distinto tanto de "sem remoto" quanto de "0 à frente / 0 atrás": remote configurado,
        // mas sem branch de tracking (`@{u}`) — ahead/behind não são computáveis, não "zero"
        // (débito técnico #3 / issue #3, `tasks.md` T049).
        RemoteStatus::NoUpstreamTracking => {
            "sem tracking configurado (ahead/behind desconhecido)".to_string()
        }
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
/// **T015**: `Message::ActionInvokeRequested` (renomeada em T019, feature
/// 004, do nome anterior específico de `git.fetch` — mesmos campos,
/// identificador genérico) ganhou o campo `plugin_name` (uma conexão por
/// plugin conhecido agora, não mais implícita) — este `plugin_name` é
/// passado por `view_repo_row`/`view_ready`, propagado da seção que
/// renderizou este item.
fn view_fetch_control<'a>(
    plugin_name: &'a str,
    item: &'a RepositoryViewModel,
) -> Element<'a, Message> {
    if item.fetch_in_flight {
        return text("buscando...").width(Length::FillPortion(1)).into();
    }

    let action = &item.fetch_action;
    let on_press = action.enabled.then(|| Message::ActionInvokeRequested {
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
