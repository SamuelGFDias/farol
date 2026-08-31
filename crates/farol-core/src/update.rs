//! Transições de `Farol`/`PluginConnection` a partir de `Message` (T022,
//! T026, T035, T038) e composição das `Subscription`s do app (worker do
//! plugin + timer de refresh periódico).

use std::time::Duration;

use iced::Subscription;

use crate::model::{PluginIdentity, PluginState, RepositoryViewModel, UnavailableReason};
use crate::plugin_worker::{
    self, ActionOutcome, HandshakeOutcome, WidgetOutcome, WorkerEvent, WorkerInput,
};
use crate::{Farol, Message};

/// Intervalo de refresh aplicado quando o plugin não sugeriu nenhum no
/// handshake (`WidgetDeclaration.suggested_refresh_interval_ms` ausente) —
/// FR-011.
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_millis(30_000);

impl Farol {
    pub(crate) fn update(&mut self, message: Message) {
        match message {
            Message::Worker(event) => self.handle_worker_event(event),
            Message::RefreshTick => self.handle_refresh_tick(),
            Message::FetchRequested {
                action_id,
                target,
                timeout_hint_ms,
            } => self.handle_fetch_requested(action_id, target, timeout_hint_ms),
        }
    }

    /// `Subscription`s ativas do app — sempre inclui o worker do plugin
    /// (para o processo continuar de pé e o handshake acontecer); inclui o
    /// timer de refresh (T026) apenas quando a conexão já está `Ready` (não
    /// há o que atualizar em `Starting`/`Handshaking`/`Unavailable`).
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let worker_subscription = plugin_worker::subscription().map(Message::Worker);

        if self.plugin.state == PluginState::Ready {
            let refresh_subscription =
                iced::time::every(self.refresh_interval()).map(|_instant| Message::RefreshTick);
            Subscription::batch([worker_subscription, refresh_subscription])
        } else {
            worker_subscription
        }
    }

    /// Intervalo efetivo do refresh periódico (data-model.md §2.3): o
    /// `suggested_refresh_interval_ms` do primeiro widget declarado, ou o
    /// default de 30s (FR-011). Só é chamado quando `state == Ready`, ou
    /// seja, quando `widgets` já foi preenchido pelo handshake.
    fn refresh_interval(&self) -> Duration {
        self.plugin
            .widgets
            .first()
            .and_then(|widget| widget.suggested_refresh_interval_ms)
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_REFRESH_INTERVAL)
    }

    fn handle_worker_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Ready(sender) => {
                self.worker_sender = Some(sender);
                // data-model.md §3: "Starting -- (spawn ok) --> Handshaking".
                // O worker já dispara o handshake internamente assim que
                // registra este canal (ver plugin_worker::worker) — aqui só
                // refletimos a transição de estado correspondente.
                self.plugin.state = PluginState::Handshaking;
            }
            WorkerEvent::SpawnFailed(detail) => {
                self.plugin.state = PluginState::Unavailable {
                    reason: UnavailableReason::FailedToStart,
                    detail,
                };
            }
            WorkerEvent::HandshakeCompleted(outcome) => self.handle_handshake_outcome(outcome),
            WorkerEvent::WidgetGetCompleted(outcome) => self.handle_widget_outcome(outcome),
            WorkerEvent::ActionInvokeCompleted(outcome) => self.handle_action_outcome(outcome),
            WorkerEvent::Crashed(detail) => {
                // T038/T039 (FR-019): processo morreu depois de já estar
                // `Ready` — converge para a MESMA variante `Unavailable`
                // que `Unresponsive` (só o `reason`/`detail` interno muda);
                // é essa igualdade estrutural que garante que `view.rs`
                // desenha os dois no mesmo braço de `match`, sem duplicar
                // lógica de renderização por motivo.
                self.plugin.state = PluginState::Unavailable {
                    reason: UnavailableReason::Crashed,
                    detail,
                };
            }
        }
    }

    /// T022: aplica o resultado do handshake à máquina de estados.
    fn handle_handshake_outcome(&mut self, outcome: HandshakeOutcome) {
        match outcome {
            HandshakeOutcome::Ready(result) => {
                self.plugin.identity = Some(PluginIdentity {
                    plugin_name: result.plugin_name,
                    protocol_version: result.protocol_version,
                    capabilities: result.capabilities,
                });
                self.plugin.widgets = result.widgets;
                self.plugin.state = PluginState::Ready;

                // Correção pós-onda: fetch imediato ao ficar Ready — sem
                // isto, o primeiro `widget/get` só sairia no primeiro tick
                // do timer de refresh (até 30s depois, ver
                // `subscription`/`handle_refresh_tick`), e a UI mostrava
                // "nenhum repositório encontrado" nesse intervalo por não
                // distinguir "ainda não busquei" de "busquei e está vazio".
                // Reaproveita a mesma lógica de disparo (guarda de estado,
                // `try_send` não-bloqueante) em vez de duplicá-la aqui.
                self.handle_refresh_tick();
            }
            HandshakeOutcome::VersionIncompatible {
                plugin_version,
                core_version,
            } => {
                self.plugin.state = PluginState::Unavailable {
                    reason: UnavailableReason::VersionIncompatible,
                    detail: format!(
                        "plugin fala protocolo {plugin_version}, este core só suporta {core_version}"
                    ),
                };
            }
            HandshakeOutcome::Unresponsive => {
                self.plugin.state = PluginState::Unavailable {
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
    fn handle_widget_outcome(&mut self, outcome: WidgetOutcome) {
        match outcome {
            WidgetOutcome::Success(result) => {
                // T035: preserva `fetch_in_flight`/`last_error` dos
                // repositórios já conhecidos — um refresh periódico não
                // deve apagar o feedback de uma ação em andamento/com erro
                // que ainda não terminou (ver `merge_widget_items`).
                self.plugin.items = merge_widget_items(&self.plugin.items, result.items);
                self.plugin.last_widget_error = None;
            }
            WidgetOutcome::PluginError(message) => {
                self.plugin.last_widget_error = Some(message);
            }
            WidgetOutcome::Unresponsive => {
                self.plugin.state = PluginState::Unavailable {
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
    fn handle_action_outcome(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Success(result) => {
                let repo_id = result.repo.id.clone();
                if let Some(item) = self.find_item_mut(&repo_id) {
                    // FR-018: o `GitRepository` pós-fetch substitui
                    // diretamente o anterior — sem `widget/get` adicional.
                    item.repo = result.repo;
                    item.fetch_in_flight = false;
                    item.last_error = None;
                }
            }
            ActionOutcome::PluginError { target, message } => {
                self.set_fetch_error(&target.id, message);
            }
            ActionOutcome::Timeout { target } => {
                self.set_fetch_error(
                    &target.id,
                    format!(
                        "ação não respondeu dentro do orçamento de {:?} (RPC_TIMEOUT_ACTION/timeout_hint_ms)",
                        plugin_worker::RPC_TIMEOUT_ACTION
                    ),
                );
            }
        }
    }

    fn find_item_mut(&mut self, repo_id: &str) -> Option<&mut RepositoryViewModel> {
        self.plugin.items.iter_mut().find(|item| item.repo.id == repo_id)
    }

    /// T036: marca o erro da última invocação de fetch para `repo_id`,
    /// limpando o indicador de "em andamento" — os dados de `repo` já
    /// conhecidos (ahead/behind, dirty) são preservados (FR-018: nenhum
    /// dado é perdido por causa de um fetch que falhou).
    fn set_fetch_error(&mut self, repo_id: &str, message: String) {
        if let Some(item) = self.find_item_mut(repo_id) {
            item.fetch_in_flight = false;
            item.last_error = Some(message);
        }
    }

    /// T033/T036: dispara `action/invoke` pelo worker para a ação de fetch
    /// de um repositório, marcando `fetch_in_flight` imediatamente para
    /// feedback de UI (a resposta chega depois, assíncrona, como
    /// `Message::Worker` — D5, nunca bloqueia `update`).
    fn handle_fetch_requested(
        &mut self,
        action_id: String,
        target: farol_protocol::ActionTarget,
        timeout_hint_ms: Option<u64>,
    ) {
        // T040: nunca envia nada ao worker fora de `Ready` — defensivo,
        // mesmo padrão de `handle_refresh_tick` (o botão de fetch só é
        // renderizado dentro de `view_ready`, então isto normalmente nem é
        // alcançável fora de `Ready`, mas não custa garantir).
        if self.plugin.state != PluginState::Ready {
            return;
        }

        let already_in_flight = self
            .plugin
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
        let Some(sender) = self.worker_sender.as_mut() else {
            return;
        };
        let _ = sender.try_send(WorkerInput::InvokeAction {
            action_id,
            target: target.clone(),
            timeout_hint_ms,
        });

        if let Some(item) = self.find_item_mut(&target.id) {
            item.fetch_in_flight = true;
        }
    }

    /// T026: a cada tick do timer de refresh, pede uma atualização do
    /// primeiro widget declarado (o plugin de referência `git-local` declara
    /// só um, `repo-status` — `contracts/git-local-plugin.md`), desde que a
    /// conexão esteja `Ready` e o canal do worker já exista.
    fn handle_refresh_tick(&mut self) {
        if self.plugin.state != PluginState::Ready {
            return;
        }
        let Some(widget) = self.plugin.widgets.first() else {
            return;
        };
        let Some(sender) = self.worker_sender.as_mut() else {
            return;
        };

        // `try_send` — não-bloqueante (D5): `update` nunca espera o worker
        // responder aqui; a resposta chega depois como `Message::Worker`.
        let _ = sender.try_send(WorkerInput::RequestWidget {
            widget_id: widget.id.clone(),
        });
    }
}

/// T035: funde uma nova lista de `WidgetItem` (recém-chegada de
/// `widget/get`) com os `RepositoryViewModel` já conhecidos, preservando
/// `fetch_in_flight`/`last_error` de qualquer repositório presente em
/// ambas as listas (casado por `repo.id`). Repositórios que somem da nova
/// lista são descartados; repositórios novos entram sem estado de UI prévio
/// (`RepositoryViewModel::from`).
fn merge_widget_items(
    previous: &[RepositoryViewModel],
    new_items: Vec<farol_protocol::WidgetItem>,
) -> Vec<RepositoryViewModel> {
    new_items
        .into_iter()
        .map(|item| {
            let mut view_model = RepositoryViewModel::from(item);
            if let Some(prev) = previous.iter().find(|prev| prev.repo.id == view_model.repo.id) {
                view_model.fetch_in_flight = prev.fetch_in_flight;
                view_model.last_error = prev.last_error.clone();
            }
            view_model
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use farol_protocol::{CapabilityManifest, ProtocolVersion};

    fn farol_with_widget(suggested_ms: Option<u64>) -> Farol {
        let mut app = Farol::default();
        app.plugin.state = PluginState::Ready;
        app.plugin.identity = Some(PluginIdentity {
            plugin_name: "git-local".to_string(),
            protocol_version: ProtocolVersion::new(0, 1),
            capabilities: CapabilityManifest {
                capabilities: vec!["exec".to_string()],
            },
        });
        app.plugin.widgets = vec![farol_protocol::WidgetDeclaration {
            id: "repo-status".to_string(),
            kind: "status-grid".to_string(),
            title: "Repositórios Git".to_string(),
            suggested_refresh_interval_ms: suggested_ms,
        }];
        app
    }

    #[test]
    fn refresh_interval_uses_plugin_suggestion_when_present() {
        let app = farol_with_widget(Some(5_000));
        assert_eq!(app.refresh_interval(), Duration::from_millis(5_000));
    }

    #[test]
    fn refresh_interval_falls_back_to_default_when_absent() {
        let app = farol_with_widget(None);
        assert_eq!(app.refresh_interval(), DEFAULT_REFRESH_INTERVAL);
    }

    #[test]
    fn handshake_ready_outcome_transitions_to_ready_and_freezes_widgets() {
        let mut app = Farol::default();
        assert_eq!(app.plugin.state, PluginState::Starting);

        let result = farol_protocol::HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 1),
            plugin_name: "git-local".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec!["exec".to_string()],
            },
            widgets: vec![farol_protocol::WidgetDeclaration {
                id: "repo-status".to_string(),
                kind: "status-grid".to_string(),
                title: "Repositórios Git".to_string(),
                suggested_refresh_interval_ms: None,
            }],
            actions: vec![],
        };

        app.handle_handshake_outcome(HandshakeOutcome::Ready(result));

        assert_eq!(app.plugin.state, PluginState::Ready);
        assert_eq!(app.plugin.widgets.len(), 1);
        assert!(app.plugin.identity.is_some());
    }

    #[test]
    fn handshake_version_incompatible_outcome_is_unavailable_without_widgets() {
        let mut app = Farol::default();

        app.handle_handshake_outcome(HandshakeOutcome::VersionIncompatible {
            plugin_version: ProtocolVersion::new(9, 9),
            core_version: ProtocolVersion::new(0, 1),
        });

        match app.plugin.state {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::VersionIncompatible);
            }
            other => panic!("esperava Unavailable{{VersionIncompatible}}, obteve {other:?}"),
        }
        assert!(app.plugin.widgets.is_empty());
        assert!(app.plugin.identity.is_none());
    }

    #[test]
    fn handshake_unresponsive_outcome_is_unavailable() {
        let mut app = Farol::default();

        app.handle_handshake_outcome(HandshakeOutcome::Unresponsive);

        match app.plugin.state {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::Unresponsive);
            }
            other => panic!("esperava Unavailable{{Unresponsive}}, obteve {other:?}"),
        }
    }

    #[test]
    fn widget_plugin_error_keeps_last_items_and_does_not_change_state() {
        let mut app = farol_with_widget(None);
        app.plugin.items = vec![sample_repo_view_model("tracked-repo")];

        app.handle_widget_outcome(WidgetOutcome::PluginError(
            "scan_root inacessível".to_string(),
        ));

        assert_eq!(app.plugin.state, PluginState::Ready);
        assert_eq!(app.plugin.items.len(), 1);
        assert_eq!(
            app.plugin.last_widget_error.as_deref(),
            Some("scan_root inacessível")
        );
    }

    #[test]
    fn widget_unresponsive_outcome_transitions_to_unavailable() {
        let mut app = farol_with_widget(None);

        app.handle_widget_outcome(WidgetOutcome::Unresponsive);

        match app.plugin.state {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(reason, UnavailableReason::Unresponsive);
            }
            other => panic!("esperava Unavailable{{Unresponsive}}, obteve {other:?}"),
        }
    }

    #[test]
    fn widget_success_outcome_replaces_items_and_clears_previous_error() {
        let mut app = farol_with_widget(None);
        app.plugin.last_widget_error = Some("erro antigo".to_string());

        let result = farol_protocol::WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: vec![sample_widget_item("farol")],
        };
        app.handle_widget_outcome(WidgetOutcome::Success(result));

        assert_eq!(app.plugin.items.len(), 1);
        assert_eq!(app.plugin.items[0].repo.name, "farol");
        assert!(app.plugin.last_widget_error.is_none());
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

        app.handle_worker_event(WorkerEvent::Crashed(
            "processo do plugin encerrou inesperadamente: exit status: 1".to_string(),
        ));

        match app.plugin.state {
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
        crashed_app.handle_worker_event(WorkerEvent::Crashed("morreu".to_string()));

        let mut unresponsive_app = farol_with_widget(None);
        unresponsive_app.handle_widget_outcome(WidgetOutcome::Unresponsive);

        assert!(matches!(
            crashed_app.plugin.state,
            PluginState::Unavailable { .. }
        ));
        assert!(matches!(
            unresponsive_app.plugin.state,
            PluginState::Unavailable { .. }
        ));

        // Distinguíveis apenas no `reason` interno, não na categoria.
        match (crashed_app.plugin.state, unresponsive_app.plugin.state) {
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
        app.plugin.items = vec![sample_repo_view_model("farol")];
        app.plugin.items[0].fetch_in_flight = true;
        app.plugin.items[0].last_error = Some("erro antigo".to_string());

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
        app.handle_action_outcome(ActionOutcome::Success(farol_protocol::ActionInvokeResult {
            repo: updated_repo.clone(),
        }));

        assert_eq!(app.plugin.items[0].repo, updated_repo);
        assert!(!app.plugin.items[0].fetch_in_flight);
        assert!(app.plugin.items[0].last_error.is_none());
    }

    #[test]
    fn action_plugin_error_outcome_sets_last_error_and_keeps_repo_data() {
        let mut app = farol_with_widget(None);
        app.plugin.items = vec![sample_repo_view_model("farol")];
        app.plugin.items[0].fetch_in_flight = true;
        let original_repo = app.plugin.items[0].repo.clone();

        app.handle_action_outcome(ActionOutcome::PluginError {
            target: farol_protocol::ActionTarget {
                r#type: "repo".to_string(),
                id: "/home/dev/farol".to_string(),
            },
            message: "git fetch falhou".to_string(),
        });

        assert_eq!(app.plugin.items[0].repo, original_repo);
        assert!(!app.plugin.items[0].fetch_in_flight);
        assert_eq!(
            app.plugin.items[0].last_error.as_deref(),
            Some("git fetch falhou")
        );
    }

    #[test]
    fn action_timeout_outcome_sets_last_error_without_changing_plugin_state() {
        let mut app = farol_with_widget(None);
        app.plugin.items = vec![sample_repo_view_model("farol")];
        app.plugin.items[0].fetch_in_flight = true;

        app.handle_action_outcome(ActionOutcome::Timeout {
            target: farol_protocol::ActionTarget {
                r#type: "repo".to_string(),
                id: "/home/dev/farol".to_string(),
            },
        });

        // D6/action-protocol.md: timeout de ação NUNCA marca o plugin como
        // indisponível — só é um erro pontual daquela ação.
        assert_eq!(app.plugin.state, PluginState::Ready);
        assert!(!app.plugin.items[0].fetch_in_flight);
        assert!(app.plugin.items[0].last_error.is_some());
    }

    #[test]
    fn widget_refresh_preserves_fetch_in_flight_and_last_error_for_matching_repo() {
        let mut app = farol_with_widget(None);
        app.plugin.items = vec![sample_repo_view_model("farol")];
        app.plugin.items[0].fetch_in_flight = true;
        app.plugin.items[0].last_error = Some("erro anterior".to_string());

        let result = farol_protocol::WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: vec![sample_widget_item("farol")],
        };
        app.handle_widget_outcome(WidgetOutcome::Success(result));

        assert!(app.plugin.items[0].fetch_in_flight);
        assert_eq!(
            app.plugin.items[0].last_error.as_deref(),
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
        app.worker_sender = Some(sender);

        let result = farol_protocol::HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 1),
            plugin_name: "git-local".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec!["exec".to_string()],
            },
            widgets: vec![farol_protocol::WidgetDeclaration {
                id: "repo-status".to_string(),
                kind: "status-grid".to_string(),
                title: "Repositórios Git".to_string(),
                suggested_refresh_interval_ms: None,
            }],
            actions: vec![],
        };

        app.handle_handshake_outcome(HandshakeOutcome::Ready(result));

        assert_eq!(app.plugin.state, PluginState::Ready);
        match receiver.try_recv() {
            Ok(WorkerInput::RequestWidget { widget_id }) => {
                assert_eq!(widget_id, "repo-status");
            }
            other => panic!("esperava Ok(RequestWidget{{repo-status}}), obteve {other:?}"),
        }
    }
}
