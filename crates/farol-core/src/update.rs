//! Transições de `Farol`/`PluginConnection` a partir de `Message` (T022,
//! T026, T035, T038) e composição das `Subscription`s do app (worker de
//! cada plugin conhecido + timer de refresh periódico por conexão `Ready`).
//!
//! **T015 (correção C2 parte 2)**: generalizado de uma única conexão
//! implícita para uma coleção (`Farol::plugins: Vec<PluginSlot>`, main.rs) —
//! toda função que antes operava sobre `self.plugin`/`self.worker_sender`
//! agora recebe (ou já embute) um `plugin_name: &str` e localiza a entrada
//! correspondente via [`Farol::slot_mut`] antes de agir. Mensagens que não
//! encontram mais nenhum slot (não deveria acontecer em uso normal — todo
//! `Message` desta forma só é produzido a partir de um slot que já existe em
//! `Farol::plugins`, um registro fixo que nunca cresce/encolhe em runtime)
//! são silenciosamente ignoradas, mesmo padrão defensivo que o código já
//! aplicava para "canal do worker ainda não existe" antes desta feature.

use std::time::Duration;

use iced::Subscription;

use crate::model::{self, PluginIdentity, PluginState, RepositoryViewModel, UnavailableReason};
use crate::plugin_worker::{
    self, ActionOutcome, HandshakeOutcome, WidgetOutcome, WorkerEvent, WorkerInput,
};
use crate::{Farol, Message, PluginSlot};

/// Intervalo de refresh aplicado quando o plugin não sugeriu nenhum no
/// handshake (`WidgetDeclaration.suggested_refresh_interval_ms` ausente) —
/// FR-011.
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_millis(30_000);

impl Farol {
    pub(crate) fn update(&mut self, message: Message) {
        match message {
            Message::Worker { plugin_name, event } => self.handle_worker_event(&plugin_name, event),
            Message::RefreshTick { plugin_name } => self.handle_refresh_tick(&plugin_name),
            Message::FetchRequested {
                plugin_name,
                action_id,
                target,
                timeout_hint_ms,
            } => self.handle_fetch_requested(&plugin_name, action_id, target, timeout_hint_ms),
        }
    }

    /// `Subscription`s ativas do app — uma por plugin conhecido (T015; o
    /// worker daquele plugin, para o processo continuar de pé e o handshake
    /// acontecer), mais o timer de refresh (T026) de cada conexão que já
    /// está `Ready` (não há o que atualizar em
    /// `Starting`/`Handshaking`/`Unavailable`, incluindo
    /// `Unavailable{NotConfigured}`, T019 — o core não chama `widget/get`
    /// para uma conexão nesse estado).
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::with_capacity(self.plugins.len() * 2);

        for slot in &self.plugins {
            let plugin_name = slot.spawn_config.plugin_name.clone();
            let worker_plugin_name = plugin_name.clone();
            let worker_subscription = plugin_worker::subscription(slot.spawn_config.clone()).map(
                move |event| Message::Worker {
                    plugin_name: worker_plugin_name.clone(),
                    event,
                },
            );
            subscriptions.push(worker_subscription);

            if slot.connection.state == PluginState::Ready {
                let interval = refresh_interval(&slot.connection);
                let tick_plugin_name = plugin_name.clone();
                let refresh_subscription = iced::time::every(interval).map(move |_instant| {
                    Message::RefreshTick {
                        plugin_name: tick_plugin_name.clone(),
                    }
                });
                subscriptions.push(refresh_subscription);
            }
        }

        Subscription::batch(subscriptions)
    }

    /// Localiza a entrada de `plugins` correspondente a `plugin_name`
    /// (T015) — `None` só é alcançável se uma mensagem chegar referenciando
    /// um plugin fora do registro fixo (`plugin_worker::known_plugins()`),
    /// o que não deveria ocorrer em uso normal (nenhuma `Subscription`/`view`
    /// produz um `plugin_name` que não veio de uma entrada já existente).
    fn slot_mut(&mut self, plugin_name: &str) -> Option<&mut PluginSlot> {
        self.plugins
            .iter_mut()
            .find(|slot| slot.spawn_config.plugin_name == plugin_name)
    }

    fn handle_worker_event(&mut self, plugin_name: &str, event: WorkerEvent) {
        match event {
            WorkerEvent::Ready(sender) => {
                let Some(slot) = self.slot_mut(plugin_name) else {
                    return;
                };
                slot.worker_sender = Some(sender);
                // data-model.md §3: "Starting -- (spawn ok) --> Handshaking".
                // O worker já dispara o handshake internamente assim que
                // registra este canal (ver plugin_worker::worker) — aqui só
                // refletimos a transição de estado correspondente.
                slot.connection.state = PluginState::Handshaking;
            }
            WorkerEvent::SpawnFailed(detail) => {
                let Some(slot) = self.slot_mut(plugin_name) else {
                    return;
                };
                slot.connection.state = PluginState::Unavailable {
                    reason: UnavailableReason::FailedToStart,
                    detail,
                };
            }
            WorkerEvent::HandshakeCompleted(outcome) => {
                self.handle_handshake_outcome(plugin_name, outcome)
            }
            WorkerEvent::WidgetGetCompleted(outcome) => {
                self.handle_widget_outcome(plugin_name, outcome)
            }
            WorkerEvent::ActionInvokeCompleted(outcome) => {
                self.handle_action_outcome(plugin_name, outcome)
            }
            WorkerEvent::Crashed(detail) => {
                // T038/T039 (FR-019): processo morreu depois de já estar
                // `Ready` — converge para a MESMA variante `Unavailable`
                // que `Unresponsive` (só o `reason`/`detail` interno muda);
                // é essa igualdade estrutural que garante que `view.rs`
                // desenha os dois no mesmo braço de `match`, sem duplicar
                // lógica de renderização por motivo.
                let Some(slot) = self.slot_mut(plugin_name) else {
                    return;
                };
                slot.connection.state = PluginState::Unavailable {
                    reason: UnavailableReason::Crashed,
                    detail,
                };
            }
        }
    }

    /// T022: aplica o resultado do handshake à máquina de estados.
    ///
    /// **T019 (D8)**: quando o handshake é compatível
    /// (`HandshakeOutcome::Ready`), a transição não é mais incondicionalmente
    /// para `PluginState::Ready` — depende de
    /// `all_required_config_present` (calculado em `plugin_worker`, ver
    /// `HandshakeOutcome::Ready`): se `false`, a conexão vai para
    /// `Unavailable { reason: NotConfigured, .. }` em vez de `Ready`, e o
    /// primeiro `widget/get` (disparo imediato de sempre, ver comentário
    /// abaixo) simplesmente não é enviado — `handle_refresh_tick` já é
    /// defensivo quanto a `state != Ready` por conta própria.
    fn handle_handshake_outcome(&mut self, plugin_name: &str, outcome: HandshakeOutcome) {
        match outcome {
            HandshakeOutcome::Ready {
                result,
                all_required_config_present,
            } => {
                if let Some(slot) = self.slot_mut(plugin_name) {
                    slot.connection.identity = Some(PluginIdentity {
                        plugin_name: result.plugin_name,
                        protocol_version: result.protocol_version,
                        capabilities: result.capabilities,
                    });
                    slot.connection.widgets = result.widgets;
                    slot.connection.state = if all_required_config_present {
                        PluginState::Ready
                    } else {
                        // T019: única variante não-terminal de `Unavailable`
                        // nesta feature — ver `UnavailableReason::NotConfigured`
                        // (model.rs) para o caminho de volta (tela de setup,
                        // T029-T035, fora do escopo desta subtarefa).
                        PluginState::Unavailable {
                            reason: UnavailableReason::NotConfigured,
                            detail:
                                "configuração obrigatória ausente — preencha a tela de setup deste plugin"
                                    .to_string(),
                        }
                    };
                }

                if all_required_config_present {
                    // Correção pós-onda: fetch imediato ao ficar Ready — sem
                    // isto, o primeiro `widget/get` só sairia no primeiro
                    // tick do timer de refresh (até 30s depois, ver
                    // `subscription`/`handle_refresh_tick`), e a UI mostrava
                    // "nenhum repositório encontrado" nesse intervalo por
                    // não distinguir "ainda não busquei" de "busquei e está
                    // vazio". Reaproveita a mesma lógica de disparo (guarda
                    // de estado, `try_send` não-bloqueante) em vez de
                    // duplicá-la aqui. Nunca disparado quando a conexão foi
                    // para `Unavailable{NotConfigured}` (T019) — não há
                    // widget a buscar nesse estado.
                    self.handle_refresh_tick(plugin_name);
                }
            }
            HandshakeOutcome::VersionIncompatible {
                plugin_version,
                core_version,
            } => {
                let Some(slot) = self.slot_mut(plugin_name) else {
                    return;
                };
                slot.connection.state = PluginState::Unavailable {
                    reason: UnavailableReason::VersionIncompatible,
                    detail: format!(
                        "plugin fala protocolo {plugin_version}, este core só suporta {core_version}"
                    ),
                };
            }
            HandshakeOutcome::Unresponsive => {
                let Some(slot) = self.slot_mut(plugin_name) else {
                    return;
                };
                slot.connection.state = PluginState::Unavailable {
                    reason: UnavailableReason::Unresponsive,
                    detail: format!(
                        "plugin não respondeu ao handshake dentro de {:?}",
                        plugin_worker::RPC_TIMEOUT_CONTROL
                    ),
                };
            }
        }
    }

    /// T026: aplica o resultado de um ciclo de `widget/get`. Um erro pontual
    /// do plugin não muda `state` (`widget-protocol.md`); só timeout
    /// (`Unresponsive`) transiciona a conexão para `Unavailable`.
    fn handle_widget_outcome(&mut self, plugin_name: &str, outcome: WidgetOutcome) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        match outcome {
            WidgetOutcome::Success(result) => {
                // T035: preserva `fetch_in_flight`/`last_error` dos
                // repositórios já conhecidos — um refresh periódico não
                // deve apagar o feedback de uma ação em andamento/com erro
                // que ainda não terminou (ver `merge_widget_items`).
                slot.connection.items = merge_widget_items(&slot.connection.items, result.items);
                slot.connection.last_widget_error = None;
            }
            WidgetOutcome::PluginError(message) => {
                slot.connection.last_widget_error = Some(message);
            }
            WidgetOutcome::Unresponsive => {
                slot.connection.state = PluginState::Unavailable {
                    reason: UnavailableReason::Unresponsive,
                    detail: format!(
                        "plugin não respondeu a um ciclo de widget/get dentro de {:?}",
                        plugin_worker::RPC_TIMEOUT_CONTROL
                    ),
                };
            }
        }
    }

    /// T035: funde o resultado de uma invocação de `action/invoke` (fetch)
    /// no `RepositoryViewModel` do repositório-alvo. Nenhuma variante muda
    /// `PluginState` — erro e timeout de ação são pontuais da chamada, não
    /// indisponibilidade do plugin (D6, `contracts/action-protocol.md`).
    fn handle_action_outcome(&mut self, plugin_name: &str, outcome: ActionOutcome) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        match outcome {
            ActionOutcome::Success(result) => {
                let repo_id = result.repo.id.clone();
                if let Some(item) = slot
                    .connection
                    .items
                    .iter_mut()
                    .find(|item| item.repo.id == repo_id)
                {
                    // FR-018: o `GitRepository` pós-fetch substitui
                    // diretamente o anterior — sem `widget/get` adicional.
                    item.repo = result.repo;
                    item.fetch_in_flight = false;
                    item.last_error = None;
                }
            }
            ActionOutcome::PluginError { target, message } => {
                set_fetch_error(&mut slot.connection.items, &target.id, message);
            }
            ActionOutcome::Timeout { target } => {
                set_fetch_error(
                    &mut slot.connection.items,
                    &target.id,
                    format!(
                        "ação não respondeu dentro do orçamento de {:?} (RPC_TIMEOUT_ACTION/timeout_hint_ms)",
                        plugin_worker::RPC_TIMEOUT_ACTION
                    ),
                );
            }
        }
    }

    /// T033/T036: dispara `action/invoke` pelo worker para a ação de fetch
    /// de um repositório, marcando `fetch_in_flight` imediatamente para
    /// feedback de UI (a resposta chega depois, assíncrona, como
    /// `Message::Worker` — D5, nunca bloqueia `update`).
    fn handle_fetch_requested(
        &mut self,
        plugin_name: &str,
        action_id: String,
        target: farol_protocol::ActionTarget,
        timeout_hint_ms: Option<u64>,
    ) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };

        // T040: nunca envia nada ao worker fora de `Ready` — defensivo,
        // mesmo padrão de `handle_refresh_tick` (o botão de fetch só é
        // renderizado dentro de `view_ready`, então isto normalmente nem é
        // alcançável fora de `Ready`, mas não custa garantir).
        if slot.connection.state != PluginState::Ready {
            return;
        }

        let already_in_flight = slot
            .connection
            .items
            .iter()
            .any(|item| item.repo.id == target.id && item.fetch_in_flight);
        if already_in_flight {
            // Defesa contra reentrância: já há uma invocação pendente para
            // este repositório (o botão também já estaria desabilitado
            // nesse estado — ver `view_fetch_control`).
            return;
        }

        // Só marca `fetch_in_flight` depois de garantir que o canal do
        // worker existe — senão o campo ficaria travado em `true` para
        // sempre (nenhuma resposta viria para limpá-lo).
        let Some(sender) = slot.worker_sender.as_mut() else {
            return;
        };
        let _ = sender.try_send(WorkerInput::InvokeAction {
            action_id,
            target: target.clone(),
            timeout_hint_ms,
        });

        if let Some(item) = slot
            .connection
            .items
            .iter_mut()
            .find(|item| item.repo.id == target.id)
        {
            item.fetch_in_flight = true;
        }
    }

    /// T026: a cada tick do timer de refresh (ou logo após o handshake
    /// ficar `Ready`, T019), pede uma atualização do primeiro widget
    /// declarado, desde que a conexão esteja `Ready` e o canal do worker já
    /// exista.
    fn handle_refresh_tick(&mut self, plugin_name: &str) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        if slot.connection.state != PluginState::Ready {
            return;
        }
        let Some(widget) = slot.connection.widgets.first() else {
            return;
        };
        let Some(sender) = slot.worker_sender.as_mut() else {
            return;
        };

        // `try_send` — não-bloqueante (D5): `update` nunca espera o worker
        // responder aqui; a resposta chega depois como `Message::Worker`.
        let _ = sender.try_send(WorkerInput::RequestWidget {
            widget_id: widget.id.clone(),
        });
    }
}

/// Intervalo efetivo do refresh periódico de uma conexão (data-model.md
/// §2.3): o `suggested_refresh_interval_ms` do primeiro widget declarado, ou
/// o default de 30s (FR-011). Só faz sentido chamar quando `state == Ready`,
/// ou seja, quando `widgets` já foi preenchido pelo handshake — extraída
/// como função livre (T015) porque `subscription` precisa calculá-la para
/// cada slot da coleção, não mais só para "o" plugin.
fn refresh_interval(connection: &model::PluginConnection) -> Duration {
    connection
        .widgets
        .first()
        .and_then(|widget| widget.suggested_refresh_interval_ms)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_REFRESH_INTERVAL)
}

/// T036: marca o erro da última invocação de fetch para `repo_id`, limpando
/// o indicador de "em andamento" — os dados de `repo` já conhecidos
/// (ahead/behind, dirty) são preservados (FR-018: nenhum dado é perdido por
/// causa de um fetch que falhou). Função livre (T015) — opera sobre a lista
/// de itens de um slot específico, já localizado pelo chamador.
fn set_fetch_error(items: &mut [RepositoryViewModel], repo_id: &str, message: String) {
    if let Some(item) = items.iter_mut().find(|item| item.repo.id == repo_id) {
        item.fetch_in_flight = false;
        item.last_error = Some(message);
    }
}

/// T035: funde uma nova lista de itens (recém-chegada de `widget/get`) com
/// os `RepositoryViewModel` já conhecidos, preservando
/// `fetch_in_flight`/`last_error` de qualquer repositório presente em ambas
/// as listas (casado por `repo.id`). Repositórios que somem da nova lista
/// são descartados; repositórios novos entram sem estado de UI prévio
/// (`RepositoryViewModel::from`).
///
/// **Correção H2 (T013)**: `new_items` deixou de ser `Vec<farol_protocol::WidgetItem>`
/// fixo e passou a ser `farol_protocol::WidgetItems` — a união discriminada
/// introduzida pela correção C3 (T010) para `WidgetGetResult.items` aceitar
/// tanto `WidgetItem` (git, widget `status-grid`) quanto `MonitorStatusItem`
/// (uptime-kuma, widget `monitor-status-grid`). Esta função continua
/// tratando só a variante `Git` com a mesma lógica de sempre — a variante
/// `Monitor` ainda não tem um view-model próprio no `Model`
/// (`MonitorWidgetViewModel`, `data-model.md` §3.1 da feature 002, chega em
/// T029-T031, fora do escopo desta subtarefa): por ora, uma resposta
/// `Monitor` simplesmente preserva `previous` sem alteração — um no-op
/// seguro que nem perde os dados git já exibidos, nem inventa um campo de
/// estado que ainda não existe no `Model`. `handle_widget_outcome` já
/// garante, por construção (T015, um slot por plugin), que só o slot de
/// `uptime-kuma` deveria receber uma resposta `Monitor`, e só o de
/// `git-local` uma resposta `Git` — mas esta função não assume isso, apenas
/// trata cada variante da forma correspondente, sem panic em nenhum caso.
fn merge_widget_items(
    previous: &[RepositoryViewModel],
    // `farol_protocol::WidgetItems` (novo em v0.2) ainda não está na lista de re-exports de
    // `crates/farol-protocol/src/lib.rs` — mesmo gap documentado em `plugin_worker.rs`/`view.rs`,
    // fora do escopo desta subtarefa corrigir; referenciado via `farol_protocol::messages::WidgetItems`.
    new_items: farol_protocol::messages::WidgetItems,
) -> Vec<RepositoryViewModel> {
    match new_items {
        farol_protocol::messages::WidgetItems::Git(items) => items
            .into_iter()
            .map(|item| {
                let mut view_model = RepositoryViewModel::from(item);
                if let Some(prev) = previous.iter().find(|prev| prev.repo.id == view_model.repo.id)
                {
                    view_model.fetch_in_flight = prev.fetch_in_flight;
                    view_model.last_error = prev.last_error.clone();
                }
                view_model
            })
            .collect(),
        farol_protocol::messages::WidgetItems::Monitor(_) => previous.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use farol_protocol::messages::{Capability, KnownCapability, WidgetItems};
    use farol_protocol::{CapabilityManifest, ProtocolVersion};

    /// Constrói um `Farol` de teste com uma única entrada (`plugin_name`
    /// fixo `"git-local"`, T015) já `Ready`, com um widget `status-grid`
    /// declarado — substitui o antigo `farol_with_widget` de antes da
    /// generalização em `Vec<PluginSlot>`.
    fn farol_with_widget(suggested_ms: Option<u64>) -> Farol {
        let mut app = Farol::default();
        let slot = app.slot_mut("git-local").expect("git-local é um plugin conhecido");
        slot.connection.state = PluginState::Ready;
        slot.connection.identity = Some(PluginIdentity {
            plugin_name: "git-local".to_string(),
            protocol_version: ProtocolVersion::new(0, 2),
            capabilities: CapabilityManifest {
                capabilities: vec![Capability::Known(KnownCapability::Exec)],
            },
        });
        slot.connection.widgets = vec![farol_protocol::WidgetDeclaration {
            id: "repo-status".to_string(),
            kind: "status-grid".to_string(),
            title: "Repositórios Git".to_string(),
            suggested_refresh_interval_ms: suggested_ms,
        }];
        app
    }

    fn connection_state(app: &Farol, plugin_name: &str) -> PluginState {
        app.plugins
            .iter()
            .find(|slot| slot.spawn_config.plugin_name == plugin_name)
            .expect("plugin conhecido")
            .connection
            .state
            .clone()
    }

    #[test]
    fn default_farol_has_one_slot_per_known_plugin() {
        let app = Farol::default();
        let names: Vec<&str> = app
            .plugins
            .iter()
            .map(|slot| slot.spawn_config.plugin_name.as_str())
            .collect();
        assert!(names.contains(&"git-local"));
        assert!(names.contains(&"uptime-kuma"));
        assert!(app
            .plugins
            .iter()
            .all(|slot| slot.connection.state == PluginState::Starting));
    }

    #[test]
    fn refresh_interval_uses_plugin_suggestion_when_present() {
        let app = farol_with_widget(Some(5_000));
        let slot = app
            .plugins
            .iter()
            .find(|slot| slot.spawn_config.plugin_name == "git-local")
            .unwrap();
        assert_eq!(refresh_interval(&slot.connection), Duration::from_millis(5_000));
    }

    #[test]
    fn refresh_interval_falls_back_to_default_when_absent() {
        let app = farol_with_widget(None);
        let slot = app
            .plugins
            .iter()
            .find(|slot| slot.spawn_config.plugin_name == "git-local")
            .unwrap();
        assert_eq!(refresh_interval(&slot.connection), DEFAULT_REFRESH_INTERVAL);
    }

    fn sample_handshake_result(
        required_config: Vec<farol_protocol::messages::RequiredConfigItem>,
    ) -> farol_protocol::HandshakeHelloResult {
        farol_protocol::HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 2),
            plugin_name: "git-local".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec![Capability::Known(KnownCapability::Exec)],
            },
            required_config,
            widgets: vec![farol_protocol::WidgetDeclaration {
                id: "repo-status".to_string(),
                kind: "status-grid".to_string(),
                title: "Repositórios Git".to_string(),
                suggested_refresh_interval_ms: None,
            }],
            actions: vec![],
        }
    }

    #[test]
    fn handshake_ready_outcome_transitions_to_ready_and_freezes_widgets() {
        let mut app = Farol::default();
        assert_eq!(connection_state(&app, "git-local"), PluginState::Starting);

        app.handle_handshake_outcome(
            "git-local",
            HandshakeOutcome::Ready {
                result: sample_handshake_result(vec![]),
                all_required_config_present: true,
            },
        );

        assert_eq!(connection_state(&app, "git-local"), PluginState::Ready);
        let slot = app
            .plugins
            .iter()
            .find(|slot| slot.spawn_config.plugin_name == "git-local")
            .unwrap();
        assert_eq!(slot.connection.widgets.len(), 1);
        assert!(slot.connection.identity.is_some());
    }

    /// T019 (D8): `required_config` incompleto ⟹ `Unavailable{NotConfigured}`
    /// em vez de `Ready`, mesmo com handshake de versão compatível.
    #[test]
    fn handshake_ready_outcome_with_missing_required_config_is_not_configured() {
        let mut app = Farol::default();

        app.handle_handshake_outcome(
            "git-local",
            HandshakeOutcome::Ready {
                result: sample_handshake_result(vec![farol_protocol::messages::RequiredConfigItem {
                    name: "base_url".to_string(),
                    secret: false,
                    description: "URL base".to_string(),
                }]),
                all_required_config_present: false,
            },
        );

        match connection_state(&app, "git-local") {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::NotConfigured);
            }
            other => panic!("esperava Unavailable{{NotConfigured}}, obteve {other:?}"),
        }
    }

    #[test]
    fn handshake_version_incompatible_outcome_is_unavailable_without_widgets() {
        let mut app = Farol::default();

        app.handle_handshake_outcome(
            "git-local",
            HandshakeOutcome::VersionIncompatible {
                plugin_version: ProtocolVersion::new(9, 9),
                core_version: ProtocolVersion::new(0, 2),
            },
        );

        match connection_state(&app, "git-local") {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::VersionIncompatible);
            }
            other => panic!("esperava Unavailable{{VersionIncompatible}}, obteve {other:?}"),
        }
        let slot = app
            .plugins
            .iter()
            .find(|slot| slot.spawn_config.plugin_name == "git-local")
            .unwrap();
        assert!(slot.connection.widgets.is_empty());
        assert!(slot.connection.identity.is_none());
    }

    #[test]
    fn handshake_unresponsive_outcome_is_unavailable() {
        let mut app = Farol::default();

        app.handle_handshake_outcome("git-local", HandshakeOutcome::Unresponsive);

        match connection_state(&app, "git-local") {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::Unresponsive);
            }
            other => panic!("esperava Unavailable{{Unresponsive}}, obteve {other:?}"),
        }
    }

    #[test]
    fn widget_plugin_error_keeps_last_items_and_does_not_change_state() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items = vec![sample_repo_view_model("tracked-repo")];
        }

        app.handle_widget_outcome(
            "git-local",
            WidgetOutcome::PluginError("scan_root inacessível".to_string()),
        );

        assert_eq!(connection_state(&app, "git-local"), PluginState::Ready);
        let slot = app.slot_mut("git-local").unwrap();
        assert_eq!(slot.connection.items.len(), 1);
        assert_eq!(
            slot.connection.last_widget_error.as_deref(),
            Some("scan_root inacessível")
        );
    }

    #[test]
    fn widget_unresponsive_outcome_transitions_to_unavailable() {
        let mut app = farol_with_widget(None);

        app.handle_widget_outcome("git-local", WidgetOutcome::Unresponsive);

        match connection_state(&app, "git-local") {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::Unresponsive);
            }
            other => panic!("esperava Unavailable{{Unresponsive}}, obteve {other:?}"),
        }
    }

    #[test]
    fn widget_success_outcome_replaces_items_and_clears_previous_error() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.last_widget_error = Some("erro antigo".to_string());
        }

        let result = farol_protocol::WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: WidgetItems::Git(vec![sample_widget_item("farol")]),
        };
        app.handle_widget_outcome("git-local", WidgetOutcome::Success(result));

        let slot = app.slot_mut("git-local").unwrap();
        assert_eq!(slot.connection.items.len(), 1);
        assert_eq!(slot.connection.items[0].repo.name, "farol");
        assert!(slot.connection.last_widget_error.is_none());
    }

    fn sample_widget_item(name: &str) -> farol_protocol::WidgetItem {
        farol_protocol::WidgetItem {
            repo: farol_protocol::GitRepository {
                id: format!("/home/dev/{name}"),
                name: name.to_string(),
                path: format!("/home/dev/{name}"),
                dirty: false,
                remote_status: farol_protocol::RemoteStatus::NoRemote,
            },
            fetch_action: farol_protocol::ActionDeclaration {
                id: "git.fetch".to_string(),
                label: "Fetch".to_string(),
                target: farol_protocol::ActionTarget {
                    r#type: "repo".to_string(),
                    id: format!("/home/dev/{name}"),
                },
                enabled: false,
                timeout_hint_ms: None,
            },
        }
    }

    fn sample_repo_view_model(name: &str) -> RepositoryViewModel {
        RepositoryViewModel::from(sample_widget_item(name))
    }

    // --- T048: máquina de estados PluginState — crash (T038), convergência
    // visual Crashed/Unresponsive (T039) e fusão de resultado de ação
    // (T035) ---

    #[test]
    fn worker_crashed_event_transitions_to_unavailable_crashed() {
        let mut app = farol_with_widget(None);

        app.handle_worker_event(
            "git-local",
            WorkerEvent::Crashed(
                "processo do plugin encerrou inesperadamente: exit status: 1".to_string(),
            ),
        );

        match connection_state(&app, "git-local") {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::Crashed);
            }
            other => panic!("esperava Unavailable{{Crashed}}, obteve {other:?}"),
        }
    }

    /// T039: `Crashed` (T038, morte do processo) e `Unresponsive` (T026,
    /// timeout de refresh) devem convergir para a MESMA variante
    /// `PluginState::Unavailable { .. }` — é essa igualdade de variante do
    /// enum que garante que `view.rs` desenha ambos no mesmo braço de
    /// `match` (mesma categoria visual básica), sem precisar duplicar
    /// lógica de renderização por `reason`.
    #[test]
    fn crashed_and_unresponsive_converge_to_the_same_unavailable_variant() {
        let mut crashed_app = farol_with_widget(None);
        crashed_app.handle_worker_event("git-local", WorkerEvent::Crashed("morreu".to_string()));

        let mut unresponsive_app = farol_with_widget(None);
        unresponsive_app.handle_widget_outcome("git-local", WidgetOutcome::Unresponsive);

        assert!(matches!(
            connection_state(&crashed_app, "git-local"),
            PluginState::Unavailable { .. }
        ));
        assert!(matches!(
            connection_state(&unresponsive_app, "git-local"),
            PluginState::Unavailable { .. }
        ));

        // Distinguíveis apenas no `reason` interno, não na categoria.
        match (
            connection_state(&crashed_app, "git-local"),
            connection_state(&unresponsive_app, "git-local"),
        ) {
            (
                PluginState::Unavailable { reason: r1, .. },
                PluginState::Unavailable { reason: r2, .. },
            ) => {
                assert_eq!(r1, UnavailableReason::Crashed);
                assert_eq!(r2, UnavailableReason::Unresponsive);
                assert_ne!(r1, r2);
            }
            _ => unreachable!("já verificado acima que ambos são Unavailable"),
        }
    }

    #[test]
    fn action_success_outcome_updates_repo_clears_error_and_flight_flag() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items = vec![sample_repo_view_model("farol")];
            slot.connection.items[0].fetch_in_flight = true;
            slot.connection.items[0].last_error = Some("erro antigo".to_string());
        }

        let updated_repo = farol_protocol::GitRepository {
            id: "/home/dev/farol".to_string(),
            name: "farol".to_string(),
            path: "/home/dev/farol".to_string(),
            dirty: false,
            remote_status: farol_protocol::RemoteStatus::Tracked {
                ahead: 0,
                behind: 2,
            },
        };
        app.handle_action_outcome(
            "git-local",
            ActionOutcome::Success(farol_protocol::ActionInvokeResult {
                repo: updated_repo.clone(),
            }),
        );

        let slot = app.slot_mut("git-local").unwrap();
        assert_eq!(slot.connection.items[0].repo, updated_repo);
        assert!(!slot.connection.items[0].fetch_in_flight);
        assert!(slot.connection.items[0].last_error.is_none());
    }

    #[test]
    fn action_plugin_error_outcome_sets_last_error_and_keeps_repo_data() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items = vec![sample_repo_view_model("farol")];
            slot.connection.items[0].fetch_in_flight = true;
        }
        let original_repo = {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items[0].repo.clone()
        };

        app.handle_action_outcome(
            "git-local",
            ActionOutcome::PluginError {
                target: farol_protocol::ActionTarget {
                    r#type: "repo".to_string(),
                    id: "/home/dev/farol".to_string(),
                },
                message: "git fetch falhou".to_string(),
            },
        );

        let slot = app.slot_mut("git-local").unwrap();
        assert_eq!(slot.connection.items[0].repo, original_repo);
        assert!(!slot.connection.items[0].fetch_in_flight);
        assert_eq!(
            slot.connection.items[0].last_error.as_deref(),
            Some("git fetch falhou")
        );
    }

    #[test]
    fn action_timeout_outcome_sets_last_error_without_changing_plugin_state() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items = vec![sample_repo_view_model("farol")];
            slot.connection.items[0].fetch_in_flight = true;
        }

        app.handle_action_outcome(
            "git-local",
            ActionOutcome::Timeout {
                target: farol_protocol::ActionTarget {
                    r#type: "repo".to_string(),
                    id: "/home/dev/farol".to_string(),
                },
            },
        );

        // D6/action-protocol.md: timeout de ação NUNCA marca o plugin como
        // indisponível — só é um erro pontual daquela ação.
        assert_eq!(connection_state(&app, "git-local"), PluginState::Ready);
        let slot = app.slot_mut("git-local").unwrap();
        assert!(!slot.connection.items[0].fetch_in_flight);
        assert!(slot.connection.items[0].last_error.is_some());
    }

    #[test]
    fn widget_refresh_preserves_fetch_in_flight_and_last_error_for_matching_repo() {
        let mut app = farol_with_widget(None);
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.items = vec![sample_repo_view_model("farol")];
            slot.connection.items[0].fetch_in_flight = true;
            slot.connection.items[0].last_error = Some("erro anterior".to_string());
        }

        let result = farol_protocol::WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: WidgetItems::Git(vec![sample_widget_item("farol")]),
        };
        app.handle_widget_outcome("git-local", WidgetOutcome::Success(result));

        let slot = app.slot_mut("git-local").unwrap();
        assert!(slot.connection.items[0].fetch_in_flight);
        assert_eq!(
            slot.connection.items[0].last_error.as_deref(),
            Some("erro anterior")
        );
    }

    /// Correção pós-onda: `HandshakeOutcome::Ready` deve disparar o primeiro
    /// `widget/get` imediatamente (via `handle_refresh_tick`), sem esperar o
    /// primeiro tick do timer de 30s — reproduz o bug de UX em que a tela
    /// mostrava "nenhum repositório encontrado" por até 30s mesmo com
    /// repositórios de verdade, porque o único gatilho de busca era o timer
    /// periódico.
    #[test]
    fn handshake_ready_outcome_immediately_requests_first_widget() {
        let mut app = Farol::default();
        let (sender, mut receiver) = iced::futures::channel::mpsc::channel::<WorkerInput>(16);
        app.slot_mut("git-local").unwrap().worker_sender = Some(sender);

        app.handle_handshake_outcome(
            "git-local",
            HandshakeOutcome::Ready {
                result: sample_handshake_result(vec![]),
                all_required_config_present: true,
            },
        );

        assert_eq!(connection_state(&app, "git-local"), PluginState::Ready);
        match receiver.try_recv() {
            Ok(WorkerInput::RequestWidget { widget_id }) => {
                assert_eq!(widget_id, "repo-status");
            }
            other => panic!("esperava Ok(RequestWidget{{repo-status}}), obteve {other:?}"),
        }
    }
}
