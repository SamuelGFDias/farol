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

use std::collections::BTreeMap;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::Stream;
use iced::stream;
use iced::Subscription;

use crate::model::{
    self, PluginIdentity, PluginState, RepositoryViewModel, SetupForm, UnavailableReason,
};
use crate::plugin_worker::{
    self, ActionOutcome, HandshakeOutcome, WidgetOutcome, WorkerEvent, WorkerInput,
};
use crate::{Farol, Message, PluginSlot};

/// Intervalo de refresh aplicado quando o plugin não sugeriu nenhum no
/// handshake (`WidgetDeclaration.suggested_refresh_interval_ms` ausente) —
/// FR-011.
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_millis(30_000);

/// `kind` do widget do plugin `uptime-kuma` (`data-model.md` §1.5, D4 de
/// `research.md`) — usado para distinguir, num `WorkerEvent::WidgetGetCompleted`
/// de erro (`WidgetOutcome::PluginError`, que não carrega o `widget_id`/`kind`
/// que originou a chamada), se o erro pertence ao widget `monitor-status-grid`
/// (T031: deve atualizar só `PluginConnection::monitor_widget.last_error`) ou
/// ao widget `status-grid` (deve atualizar `PluginConnection::last_widget_error`,
/// mecanismo já existente da feature 001). Cada plugin conhecido declara
/// exatamente um widget relevante (`slot.connection.widgets.first()`, mesma
/// suposição já usada por `refresh_interval`/`handle_refresh_tick`), então
/// checar `slot.connection.widgets` é suficiente para essa distinção.
const MONITOR_WIDGET_KIND: &str = "monitor-status-grid";

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
            Message::SetupFieldChanged {
                plugin_name,
                field_name,
                value,
            } => self.handle_setup_field_changed(&plugin_name, &field_name, value),
            Message::SetupSubmitted { plugin_name } => self.handle_setup_submitted(&plugin_name),
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
            // Closure não-capturante (só usa o próprio parâmetro) — requisito de
            // `iced::Subscription::map` (ver docstring de `plugin_worker::subscription`
            // sobre a correção que moveu o `plugin_name` para dentro do stream).
            // T032 (D8): `slot.connection.setup_attempt` é passado como argumento
            // comum de função (não capturado por nenhum closure de
            // `Subscription::map`) — é ele que muda a identidade da `Subscription`
            // dentro de `plugin_worker::subscription` sempre que a tela de setup
            // deste plugin é submetida, forçando a reconexão do worker.
            let worker_subscription = plugin_worker::subscription(
                slot.spawn_config.clone(),
                slot.connection.setup_attempt,
            )
            .map(|(plugin_name, event)| Message::Worker { plugin_name, event });
            subscriptions.push(worker_subscription);

            if slot.connection.state == PluginState::Ready {
                // Migração `iced` 0.14 (achado N2 de `research.md` da feature
                // 003): `Subscription::run_with_id(id, stream)` não existe
                // mais; a identidade vem do `Hash` do dado passado a
                // `Subscription::run_with`, e o stream é construído por um
                // `fn(&D) -> S` não-capturante (ver `refresh_tick_stream`).
                let refresh_subscription = Subscription::run_with(
                    RefreshSubscriptionKey {
                        plugin_name: slot.spawn_config.plugin_name.clone(),
                        interval: refresh_interval(&slot.connection),
                    },
                    refresh_tick_stream,
                );
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
                    // T030: capturado antes de `result.plugin_name`/`.widgets`
                    // serem movidos abaixo — precisamos dele para (re)construir
                    // `SetupForm` quando `all_required_config_present` é falso.
                    let required_config = result.required_config.clone();
                    slot.connection.identity = Some(PluginIdentity {
                        plugin_name: result.plugin_name,
                        protocol_version: result.protocol_version,
                        capabilities: result.capabilities,
                    });
                    slot.connection.widgets = result.widgets;
                    slot.connection.state = if all_required_config_present {
                        // T030: sai de `NotConfigured` (se estava) — não há
                        // mais formulário a exibir.
                        slot.connection.setup_form = None;
                        PluginState::Ready
                    } else {
                        // T019: única variante não-terminal de `Unavailable`
                        // nesta feature — ver `UnavailableReason::NotConfigured`
                        // (model.rs) para o caminho de volta (tela de setup).
                        //
                        // T030: (re)constrói o formulário de setup a partir do
                        // `required_config` deste handshake — um campo por
                        // item, inicializado vazio (`data-model.md` §3.2:
                        // "reexibido" após uma tentativa que ainda falhou não
                        // reaproveita valores digitados antes, já que o
                        // processo do plugin foi reiniciado do zero, T032).
                        slot.connection.setup_form = Some(SetupForm {
                            plugin_name: plugin_name.to_string(),
                            fields: required_config
                                .into_iter()
                                .map(|item| (item, String::new()))
                                .collect(),
                        });
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

    /// T026/T031: aplica o resultado de um ciclo de `widget/get`. Um erro
    /// pontual do plugin não muda `state` (`widget-protocol.md`); só timeout
    /// (`Unresponsive`) transiciona a conexão para `Unavailable`.
    ///
    /// **T031**: generalizado para também tratar a variante `Monitor` de
    /// `farol_protocol::messages::WidgetItems` (widget `monitor-status-grid`
    /// do plugin `uptime-kuma`) — sucesso popula
    /// `PluginConnection::monitor_widget.monitors`; um erro pontual
    /// (`not_configured`/`metrics_unreachable`/`metrics_parse_error`) popula
    /// só `monitor_widget.last_error`, preservando `monitors` anterior, sem
    /// alterar `PluginState` — mesmo mecanismo genérico de `protocol/SPEC.md`
    /// §5.2 já usado para `git-local`/`last_widget_error`. `WidgetOutcome::
    /// PluginError` não carrega o `widget_id`/`kind` que originou a chamada
    /// (só a mensagem de erro), então `is_monitor_widget` (abaixo) decide,
    /// pelo `kind` já congelado em `slot.connection.widgets` no handshake,
    /// qual dos dois campos de erro atualizar — cada plugin conhecido só
    /// declara um widget relevante (mesma suposição de
    /// `refresh_interval`/`handle_refresh_tick`).
    fn handle_widget_outcome(&mut self, plugin_name: &str, outcome: WidgetOutcome) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        let is_monitor_widget = slot
            .connection
            .widgets
            .iter()
            .any(|widget| widget.kind == MONITOR_WIDGET_KIND);
        match outcome {
            WidgetOutcome::Success(result) => {
                let items = normalize_widget_items(result.items, is_monitor_widget);
                match merge_widget_items(&slot.connection.items, items) {
                    MergedWidgetItems::Git(items) => {
                        // T035: preserva `fetch_in_flight`/`last_error` dos
                        // repositórios já conhecidos — um refresh periódico não
                        // deve apagar o feedback de uma ação em andamento/com
                        // erro que ainda não terminou (ver `merge_widget_items`).
                        slot.connection.items = items;
                        slot.connection.last_widget_error = None;
                    }
                    MergedWidgetItems::Monitor(monitors) => {
                        slot.connection.monitor_widget.monitors = monitors;
                        slot.connection.monitor_widget.last_error = None;
                    }
                }
            }
            WidgetOutcome::PluginError(message) => {
                if is_monitor_widget {
                    slot.connection.monitor_widget.last_error = Some(message);
                } else {
                    slot.connection.last_widget_error = Some(message);
                }
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

    /// T032 (D8): atualiza o valor digitado de um campo do formulário de
    /// setup deste plugin (`Message::SetupFieldChanged`, disparada por
    /// keystroke na `view` — T035). Só tem efeito quando a conexão tem um
    /// `SetupForm` ativo (`state == Unavailable{NotConfigured}`, ver
    /// `handle_handshake_outcome`); nas demais situações não há formulário
    /// para editar e a chamada é silenciosamente ignorada — mesmo padrão
    /// defensivo do resto deste arquivo (T015).
    fn handle_setup_field_changed(&mut self, plugin_name: &str, field_name: &str, value: String) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        let Some(form) = slot.connection.setup_form.as_mut() else {
            return;
        };
        if let Some((_, current_value)) = form
            .fields
            .iter_mut()
            .find(|(item, _)| item.name == field_name)
        {
            *current_value = value;
        }
    }

    /// T032 (D8): submissão do formulário de setup deste plugin
    /// (`Message::SetupSubmitted`, disparada pelo botão de confirmar —
    /// T035). Persiste cada valor digitado em `config.toml`
    /// (`config_store::save_plugin_config`, itens com `secret: false`) ou
    /// `secrets.toml` (`secrets_store::save_plugin_secrets`, itens com
    /// `secret: true`), conforme a flag `secret` de cada
    /// `RequiredConfigItem` — mesma fonte que
    /// `plugin_worker::resolve_required_config_value` usa para decidir qual
    /// dos dois arquivos ler. Erros de I/O ao persistir (disco cheio,
    /// permissão) são silenciosamente ignorados aqui — mesma tolerância já
    /// adotada em todo o resto do mecanismo de config/secrets (D8: um
    /// arquivo de storage do core ausente/malformado nunca impede o
    /// processo do plugin de subir; se a persistência falhar aqui, o
    /// próximo handshake simplesmente encontra o mesmo item ainda ausente e
    /// a conexão volta a `NotConfigured`, com o formulário reexibido).
    ///
    /// Dispara a reconexão do worker deste plugin incrementando
    /// `PluginConnection::setup_attempt` (consumido por
    /// `Farol::subscription` acima / `plugin_worker::subscription` — ver a
    /// documentação daquela função para o mecanismo completo de
    /// reconexão). Reseta `state` para `Starting`, `identity` para `None` e
    /// `worker_sender` para `None` imediatamente — mesmo shape do estado
    /// inicial de qualquer conexão (`PluginConnection::default`) — para que
    /// a UI não continue mostrando o formulário/erro antigo enquanto a
    /// reconexão está em andamento; o handshake do novo processo (fluxo já
    /// existente, T021/T022) decide depois `Ready` vs.
    /// `Unavailable{NotConfigured}` de novo.
    fn handle_setup_submitted(&mut self, plugin_name: &str) {
        let Some(slot) = self.slot_mut(plugin_name) else {
            return;
        };
        let Some(form) = slot.connection.setup_form.take() else {
            return;
        };

        let mut config_values: BTreeMap<String, String> = BTreeMap::new();
        let mut secret_values: BTreeMap<String, String> = BTreeMap::new();
        for (item, value) in form.fields {
            if item.secret {
                secret_values.insert(item.name, value);
            } else {
                config_values.insert(item.name, value);
            }
        }
        if !config_values.is_empty() {
            let _ = crate::config_store::save_plugin_config(plugin_name, &config_values);
        }
        if !secret_values.is_empty() {
            let _ = crate::secrets_store::save_plugin_secrets(plugin_name, &secret_values);
        }

        slot.connection.setup_attempt += 1;
        slot.connection.state = PluginState::Starting;
        slot.connection.identity = None;
        slot.worker_sender = None;
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

/// Identidade (`D: Hash` de `Subscription::run_with`) da `Subscription` do
/// timer de refresh de um plugin `Ready` — ver [`refresh_tick_stream`].
///
/// **Nota de migração (`iced` 0.14, achado N2 de `research.md` da feature
/// 003)**: sob `Subscription::run_with_id` (0.13) o `id` era só
/// `"{plugin_name}-refresh"`, e `interval` **não** participava da
/// identidade — se um plugin passasse a sugerir outro
/// `suggested_refresh_interval_ms`, o timer antigo continuaria rodando com o
/// intervalo antigo. Aqui `interval` entra no `Hash`, então uma mudança de
/// intervalo encerra o timer antigo e inicia um novo com o intervalo certo.
/// Na prática isso não muda nada hoje (`PluginConnection::widgets` é
/// congelado no handshake, `model.rs`, e `refresh_interval` deriva dele),
/// mas é a semântica correta e a única alternativa seria um `impl Hash`
/// manual inconsistente com `Eq`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RefreshSubscriptionKey {
    plugin_name: String,
    interval: Duration,
}

/// Stream do timer de refresh periódico de um plugin `Ready` (T026).
///
/// Segunda ocorrência real da armadilha documentada no `AGENTS.md`
/// ("`iced::Subscription::map` exige closure não-capturante"):
/// `iced::time::every(interval).map(move |_| ...)` captura `plugin_name`
/// dentro do closure passado a `Subscription::map`, que exige
/// `size_of::<F>() == 0`. Mesma correção de `plugin_worker::worker`:
/// embutir `plugin_name` dentro do *stream* via `iced::stream::channel`
/// (permitido — a restrição de zero-size é só de `Subscription::map`), e dar
/// identidade estável via `Subscription::run_with` no chamador
/// (`Farol::subscription`).
///
/// **Migração `iced` 0.14 (feature 003)**: em 0.13 essa armadilha era um
/// `debug_assert!` de runtime; em 0.14 virou `const { check_zero_sized::<F>() }`,
/// ou seja, **erro de compilação** (`E0080`) — a classe de bug deixou de ser
/// possível de existir num binário compilado. Esta função passou de
/// `fn(String, Duration) -> impl Stream` para `fn(&RefreshSubscriptionKey) ->
/// impl Stream`, a forma de `builder` exigida por `Subscription::run_with`
/// (ponteiro de função não-capturante que recebe a identidade por
/// referência).
fn refresh_tick_stream(key: &RefreshSubscriptionKey) -> impl Stream<Item = Message> {
    let plugin_name = key.plugin_name.clone();
    let interval = key.interval;

    stream::channel(1, move |mut output: mpsc::Sender<Message>| async move {
        use iced::futures::SinkExt;

        let mut ticker = tokio::time::interval(interval);
        // `tokio::time::interval` dispara o primeiro tick imediatamente na
        // criação — descartado aqui de propósito, sem emitir `RefreshTick`:
        // o primeiro `widget/get` de uma conexão que acabou de ficar `Ready`
        // já é disparado por outro mecanismo ("Correção pós-onda: fetch
        // imediato ao ficar Ready", ver `Farol::handle_handshake_outcome`) —
        // se este stream também emitisse no instante zero, haveria um fetch
        // duplicado logo na entrada em `Ready`.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let message = Message::RefreshTick {
                plugin_name: plugin_name.clone(),
            };
            if output.send(message).await.is_err() {
                break;
            }
        }
    })
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

/// Resultado de [`merge_widget_items`] — união discriminada pelo mesmo
/// `kind` que já discrimina `farol_protocol::messages::WidgetItems` (T031).
/// Existe porque as duas variantes de entrada produzem tipos de saída
/// diferentes: `Git` funde com o `RepositoryViewModel` já conhecido
/// (preservando `fetch_in_flight`/`last_error` de UI local); `Monitor` não
/// tem nenhum estado de UI local por item a preservar (`MonitorStatusItem`
/// não carrega `ActionDeclaration`/estado de fetch, `data-model.md` §1.3 —
/// diferente de `WidgetItem`/`RepositoryViewModel`), então é só a lista
/// recém-chegada, repassada como está.
enum MergedWidgetItems {
    Git(Vec<RepositoryViewModel>),
    Monitor(Vec<farol_protocol::messages::MonitorStatusItem>),
}

/// Corrige a ambiguidade documentada de `farol_protocol::messages::WidgetItems`
/// (`#[serde(untagged)]`, `crates/farol-protocol/src/messages.rs`): as duas
/// variantes serializam como `Vec<T>` simples, então um array `items: []`
/// desserializa sempre como a primeira variante tentada (`Git`), mesmo
/// quando a resposta veio de um widget `monitor-status-grid` (débito #5,
/// issue #7 — achado ao corrigir T039/T051: uma instância Uptime Kuma real
/// sem monitores cadastrados devolve `items: []`, que
/// `handle_widget_outcome` roteava para `connection.items`, o campo errado,
/// deixando `monitor_widget.last_error` da leitura anterior nunca limpo).
///
/// O core já sabe, pelo `kind` que este `widget_id` declarou no handshake
/// (`is_monitor_widget`, calculado por `handle_widget_outcome` antes de
/// chamar esta função), qual vocabulário esperar — a correção mora aqui, no
/// ponto de consumo, e não no formato wire (`protocol/schema/v0.2/
/// widget.schema.json` continua um `oneOf` de dois arrays, sem tag). Um
/// array **não vazio** nunca é ambíguo (os campos de `WidgetItem` e
/// `MonitorStatusItem` não coincidem, então o `serde` já resolve certo) —
/// só o caso vazio precisa de ajuda.
fn normalize_widget_items(
    items: farol_protocol::messages::WidgetItems,
    is_monitor_widget: bool,
) -> farol_protocol::messages::WidgetItems {
    match (items, is_monitor_widget) {
        (farol_protocol::messages::WidgetItems::Git(items), true) if items.is_empty() => {
            farol_protocol::messages::WidgetItems::Monitor(Vec::new())
        }
        (items, _) => items,
    }
}

/// T031 (generaliza T035 original — feature 001): funde uma nova lista de
/// itens (recém-chegada de `widget/get`) com o estado de UI já conhecido.
/// Para a variante `Git`, preserva `fetch_in_flight`/`last_error` de
/// qualquer repositório presente em ambas as listas (casado por `repo.id`)
/// — repositórios que somem da nova lista são descartados; repositórios
/// novos entram sem estado de UI prévio (`RepositoryViewModel::from`). Para
/// a variante `Monitor`, não há nada a preservar por item (ver
/// [`MergedWidgetItems`]) — o chamador (`handle_widget_outcome`) é quem
/// decide, a partir da variante devolvida aqui, qual campo de
/// `PluginConnection` atualizar (`items` vs. `monitor_widget.monitors`).
///
/// **Correção H2 (T013) + T031 (feature 002)**: `new_items` deixou de ser
/// `Vec<farol_protocol::WidgetItem>` fixo e passou a ser
/// `farol_protocol::WidgetItems` — a união discriminada introduzida pela
/// correção C3 (T010) para `WidgetGetResult.items` aceitar tanto
/// `WidgetItem` (git, widget `status-grid`) quanto `MonitorStatusItem`
/// (uptime-kuma, widget `monitor-status-grid`). Antes de T031, a variante
/// `Monitor` era um no-op que só preservava `previous`, porque
/// `MonitorWidgetViewModel` (`data-model.md` §3.1) ainda não existia no
/// `Model` — T029 introduziu o tipo, T031 conecta esta função a ele.
fn merge_widget_items(
    previous: &[RepositoryViewModel],
    // `farol_protocol::WidgetItems` (novo em v0.2) ainda não está na lista de re-exports de
    // `crates/farol-protocol/src/lib.rs` — mesmo gap documentado em `plugin_worker.rs`/`view.rs`,
    // fora do escopo desta subtarefa corrigir; referenciado via `farol_protocol::messages::WidgetItems`.
    new_items: farol_protocol::messages::WidgetItems,
) -> MergedWidgetItems {
    match new_items {
        farol_protocol::messages::WidgetItems::Git(items) => MergedWidgetItems::Git(
            items
                .into_iter()
                .map(|item| {
                    let mut view_model = RepositoryViewModel::from(item);
                    if let Some(prev) = previous
                        .iter()
                        .find(|prev| prev.repo.id == view_model.repo.id)
                    {
                        view_model.fetch_in_flight = prev.fetch_in_flight;
                        view_model.last_error = prev.last_error.clone();
                    }
                    view_model
                })
                .collect(),
        ),
        farol_protocol::messages::WidgetItems::Monitor(items) => MergedWidgetItems::Monitor(items),
    }
}

#[cfg(test)]
// `pub(crate)` (T024 da feature 003): `visual_snapshot_tests.rs` (US4, módulo
// irmão deste — nenhum dos dois é descendente do outro) reaproveita
// `farol_with_monitor_widget`/`sample_required_config` daqui em vez de
// duplicar a construção de `PluginIdentity`/`CapabilityManifest`/
// `WidgetDeclaration` — só a visibilidade do módulo e das duas funções
// mudou, nenhuma lógica nova foi adicionada.
pub(crate) mod tests {
    use super::*;
    use farol_protocol::messages::{Capability, KnownCapability, WidgetItems};
    use farol_protocol::{CapabilityManifest, ProtocolVersion};

    /// Constrói um `Farol` de teste com uma única entrada (`plugin_name`
    /// fixo `"git-local"`, T015) já `Ready`, com um widget `status-grid`
    /// declarado — substitui o antigo `farol_with_widget` de antes da
    /// generalização em `Vec<PluginSlot>`.
    fn farol_with_widget(suggested_ms: Option<u64>) -> Farol {
        let mut app = Farol::default();
        let slot = app
            .slot_mut("git-local")
            .expect("git-local é um plugin conhecido");
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
        assert_eq!(
            refresh_interval(&slot.connection),
            Duration::from_millis(5_000)
        );
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
                result: sample_handshake_result(vec![
                    farol_protocol::messages::RequiredConfigItem {
                        name: "base_url".to_string(),
                        secret: false,
                        description: "URL base".to_string(),
                    },
                ]),
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

    // --- T029-T032 (feature 002): MonitorWidgetViewModel, SetupForm,
    // reconexão via `setup_attempt` ---

    /// Análogo de `farol_with_widget`, mas para a entrada `uptime-kuma`
    /// (T015: um slot fixo por plugin conhecido) já `Ready`, com um widget
    /// `monitor-status-grid` declarado (T031/T033).
    ///
    /// `pub(crate)` (T024 da feature 003): reaproveitado por
    /// `visual_snapshot_tests.rs` para o `screen_id` `DashboardReady`, que só
    /// precisa completar com `monitor_widget.monitors` não vazio — ver
    /// docstring do módulo.
    pub(crate) fn farol_with_monitor_widget() -> Farol {
        let mut app = Farol::default();
        let slot = app
            .slot_mut("uptime-kuma")
            .expect("uptime-kuma é um plugin conhecido");
        slot.connection.state = PluginState::Ready;
        slot.connection.identity = Some(PluginIdentity {
            plugin_name: "uptime-kuma".to_string(),
            protocol_version: ProtocolVersion::new(0, 2),
            capabilities: CapabilityManifest {
                capabilities: vec![Capability::Known(KnownCapability::Network {
                    host: "monitor.example.com".to_string(),
                    port: Some(443),
                })],
            },
        });
        slot.connection.widgets = vec![farol_protocol::WidgetDeclaration {
            id: "uptime-kuma-monitors".to_string(),
            kind: MONITOR_WIDGET_KIND.to_string(),
            title: "Uptime Kuma".to_string(),
            suggested_refresh_interval_ms: Some(30_000),
        }];
        app
    }

    /// `pub(crate)` (T024 da feature 003): reaproveitado por
    /// `visual_snapshot_tests.rs` para o `screen_id` `SetupForm` — mesmos
    /// dois campos que o handshake real de `uptime-kuma` declara
    /// (`sample_handshake_result`/T030 acima), evitando uma segunda lista
    /// divergente de `RequiredConfigItem`.
    pub(crate) fn sample_required_config() -> Vec<farol_protocol::messages::RequiredConfigItem> {
        vec![
            farol_protocol::messages::RequiredConfigItem {
                name: "base_url".to_string(),
                secret: false,
                description: "URL base da instância Uptime Kuma".to_string(),
            },
            farol_protocol::messages::RequiredConfigItem {
                name: "api_key".to_string(),
                secret: true,
                description: "API Key de métricas do Uptime Kuma".to_string(),
            },
        ]
    }

    fn sample_monitor_item(name: &str) -> farol_protocol::messages::MonitorStatusItem {
        farol_protocol::messages::MonitorStatusItem {
            name: name.to_string(),
            status: farol_protocol::messages::MonitorStatus::Up,
            response_time_ms: Some(42),
        }
    }

    /// T030: `HandshakeOutcome::Ready` com `required_config` incompleto MUST
    /// (re)construir `PluginConnection::setup_form` — um par
    /// `(RequiredConfigItem, "")` por item declarado, na mesma ordem, valor
    /// inicial sempre vazio (`data-model.md` §3.2).
    #[test]
    fn handshake_not_configured_outcome_builds_setup_form() {
        let mut app = Farol::default();
        let required_config = sample_required_config();

        app.handle_handshake_outcome(
            "uptime-kuma",
            HandshakeOutcome::Ready {
                result: farol_protocol::HandshakeHelloResult {
                    protocol_version: ProtocolVersion::new(0, 2),
                    plugin_name: "uptime-kuma".to_string(),
                    capabilities: CapabilityManifest {
                        capabilities: vec![],
                    },
                    required_config: required_config.clone(),
                    widgets: vec![farol_protocol::WidgetDeclaration {
                        id: "uptime-kuma-monitors".to_string(),
                        kind: MONITOR_WIDGET_KIND.to_string(),
                        title: "Uptime Kuma".to_string(),
                        suggested_refresh_interval_ms: None,
                    }],
                    actions: vec![],
                },
                all_required_config_present: false,
            },
        );

        let slot = app.slot_mut("uptime-kuma").unwrap();
        match &slot.connection.state {
            PluginState::Unavailable { reason, .. } => {
                assert_eq!(*reason, UnavailableReason::NotConfigured);
            }
            other => panic!("esperava Unavailable{{NotConfigured}}, obteve {other:?}"),
        }
        let form = slot
            .connection
            .setup_form
            .as_ref()
            .expect("setup_form deveria estar preenchido em NotConfigured");
        assert_eq!(form.plugin_name, "uptime-kuma");
        assert_eq!(
            form.fields,
            required_config
                .into_iter()
                .map(|item| (item, String::new()))
                .collect::<Vec<_>>()
        );
    }

    /// T030: uma vez que `all_required_config_present` é `true`, qualquer
    /// `setup_form` anterior (de uma tentativa `NotConfigured` prévia) MUST
    /// ser limpo.
    #[test]
    fn handshake_ready_outcome_clears_previous_setup_form() {
        let mut app = Farol::default();
        {
            let slot = app.slot_mut("git-local").unwrap();
            slot.connection.setup_form = Some(model::SetupForm {
                plugin_name: "git-local".to_string(),
                fields: vec![],
            });
        }

        app.handle_handshake_outcome(
            "git-local",
            HandshakeOutcome::Ready {
                result: sample_handshake_result(vec![]),
                all_required_config_present: true,
            },
        );

        let slot = app.slot_mut("git-local").unwrap();
        assert!(slot.connection.setup_form.is_none());
    }

    /// T031: sucesso de `widget/get` para o widget `monitor-status-grid`
    /// popula `monitor_widget.monitors` (não `items`, usado só pelo widget
    /// `status-grid`/`git-local`) e limpa `monitor_widget.last_error`.
    #[test]
    fn widget_success_outcome_with_monitor_items_populates_monitor_widget() {
        let mut app = farol_with_monitor_widget();
        {
            let slot = app.slot_mut("uptime-kuma").unwrap();
            slot.connection.monitor_widget.last_error = Some("erro antigo".to_string());
        }

        let result = farol_protocol::WidgetGetResult {
            widget_id: "uptime-kuma-monitors".to_string(),
            items: WidgetItems::Monitor(vec![sample_monitor_item("api")]),
        };
        app.handle_widget_outcome("uptime-kuma", WidgetOutcome::Success(result));

        let slot = app.slot_mut("uptime-kuma").unwrap();
        assert_eq!(slot.connection.monitor_widget.monitors.len(), 1);
        assert_eq!(slot.connection.monitor_widget.monitors[0].name, "api");
        assert!(slot.connection.monitor_widget.last_error.is_none());
        // `items` (git) MUST NOT ser tocado por uma resposta `Monitor`.
        assert!(slot.connection.items.is_empty());
    }

    /// T031: um erro pontual de `widget/get` para o widget
    /// `monitor-status-grid` (`not_configured`/`metrics_unreachable`/
    /// `metrics_parse_error`) MUST atualizar só `monitor_widget.last_error`,
    /// preservando `monitor_widget.monitors` anterior e sem alterar
    /// `PluginState` (FR-017) — nunca `last_widget_error` (campo do
    /// mecanismo genérico de `git-local`).
    #[test]
    fn widget_plugin_error_for_monitor_widget_updates_only_monitor_last_error() {
        let mut app = farol_with_monitor_widget();
        {
            let slot = app.slot_mut("uptime-kuma").unwrap();
            slot.connection.monitor_widget.monitors = vec![sample_monitor_item("api")];
        }

        app.handle_widget_outcome(
            "uptime-kuma",
            WidgetOutcome::PluginError("not_configured".to_string()),
        );

        let slot = app.slot_mut("uptime-kuma").unwrap();
        assert_eq!(slot.connection.state, PluginState::Ready);
        assert_eq!(slot.connection.monitor_widget.monitors.len(), 1);
        assert_eq!(
            slot.connection.monitor_widget.last_error.as_deref(),
            Some("not_configured")
        );
        assert!(slot.connection.last_widget_error.is_none());
    }

    /// T032: `Message::SetupFieldChanged` (aqui exercitada diretamente via
    /// `handle_setup_field_changed`, ver nota de escopo no próprio método)
    /// atualiza só o campo cujo `item.name` corresponde a `field_name`,
    /// preservando os demais.
    #[test]
    fn handle_setup_field_changed_updates_only_matching_field() {
        let mut app = Farol::default();
        {
            let slot = app.slot_mut("uptime-kuma").unwrap();
            slot.connection.setup_form = Some(model::SetupForm {
                plugin_name: "uptime-kuma".to_string(),
                fields: sample_required_config()
                    .into_iter()
                    .map(|item| (item, String::new()))
                    .collect(),
            });
        }

        app.handle_setup_field_changed("uptime-kuma", "api_key", "s3cr3t".to_string());

        let slot = app.slot_mut("uptime-kuma").unwrap();
        let form = slot.connection.setup_form.as_ref().unwrap();
        let base_url_value = &form
            .fields
            .iter()
            .find(|(item, _)| item.name == "base_url")
            .unwrap()
            .1;
        let api_key_value = &form
            .fields
            .iter()
            .find(|(item, _)| item.name == "api_key")
            .unwrap()
            .1;
        assert_eq!(base_url_value, "");
        assert_eq!(api_key_value, "s3cr3t");
    }

    /// T032: sem `setup_form` ativo (conexão não está em `NotConfigured`),
    /// a mensagem é silenciosamente ignorada — mesmo padrão defensivo do
    /// resto do arquivo.
    #[test]
    fn handle_setup_field_changed_without_active_form_is_noop() {
        let mut app = Farol::default();
        app.handle_setup_field_changed("uptime-kuma", "api_key", "s3cr3t".to_string());
        let slot = app.slot_mut("uptime-kuma").unwrap();
        assert!(slot.connection.setup_form.is_none());
    }

    /// T032 (D8): submissão do formulário incrementa `setup_attempt`
    /// (mecanismo de reconexão via `id` da `Subscription`, ver
    /// `plugin_worker::subscription`), reseta a conexão para `Starting` e
    /// limpa `setup_form`/`identity`/`worker_sender`.
    ///
    /// Usa um `SetupForm` sem `fields` (lista vazia) deliberadamente — evita
    /// exercitar `config_store::save_plugin_config`/
    /// `secrets_store::save_plugin_secrets` de verdade neste teste, que
    /// dependem de `$XDG_CONFIG_HOME`/`$HOME` do ambiente real (mesma
    /// cautela já documentada nos testes de `config_store`/`secrets_store`:
    /// "não deve ser mutado por um teste unitário"). O caminho de
    /// persistência em si (`item.secret` decidindo `config.toml` vs.
    /// `secrets.toml`) é uma chamada direta e trivial às duas funções já
    /// testadas isoladamente em `config_store`/`secrets_store` — testado
    /// aqui é o comportamento de reconexão, que é a parte nova desta
    /// subtarefa.
    #[test]
    fn handle_setup_submitted_resets_connection_and_increments_setup_attempt() {
        let mut app = Farol::default();
        {
            let slot = app.slot_mut("uptime-kuma").unwrap();
            slot.connection.setup_form = Some(model::SetupForm {
                plugin_name: "uptime-kuma".to_string(),
                fields: vec![],
            });
            slot.connection.setup_attempt = 0;
            slot.connection.state = PluginState::Unavailable {
                reason: UnavailableReason::NotConfigured,
                detail: "configuração obrigatória ausente".to_string(),
            };
            slot.connection.identity = Some(PluginIdentity {
                plugin_name: "uptime-kuma".to_string(),
                protocol_version: ProtocolVersion::new(0, 2),
                capabilities: CapabilityManifest {
                    capabilities: vec![],
                },
            });
            let (sender, _receiver) = iced::futures::channel::mpsc::channel::<WorkerInput>(16);
            slot.worker_sender = Some(sender);
        }

        app.handle_setup_submitted("uptime-kuma");

        let slot = app.slot_mut("uptime-kuma").unwrap();
        assert_eq!(slot.connection.setup_attempt, 1);
        assert_eq!(slot.connection.state, PluginState::Starting);
        assert!(slot.connection.setup_form.is_none());
        assert!(slot.connection.identity.is_none());
        assert!(slot.worker_sender.is_none());
    }

    /// T032: sem `setup_form` ativo, a submissão é ignorada — `setup_attempt`
    /// não deve incrementar (não há nada a reconectar).
    #[test]
    fn handle_setup_submitted_without_active_form_is_noop() {
        let mut app = Farol::default();
        app.handle_setup_submitted("uptime-kuma");
        let slot = app.slot_mut("uptime-kuma").unwrap();
        assert_eq!(slot.connection.setup_attempt, 0);
    }

    /// T032: `Farol::subscription` compõe o `id` da `Subscription` do
    /// worker usando `setup_attempt` — verificado indiretamente aqui
    /// checando que `subscription()` não entra em pânico para os dois
    /// valores possíveis mais comuns (0 = nunca configurado, 1 = após uma
    /// submissão) e que o app continua com um slot por plugin conhecido
    /// (a identidade exata de `Subscription` não é inspecionável fora do
    /// runtime `iced`, mesma limitação documentada no módulo
    /// `plugin_worker` — só um `cargo run` real exercitaria isso de
    /// verdade, T023).
    #[test]
    fn subscription_does_not_panic_after_setup_attempt_increments() {
        let mut app = Farol::default();
        let _ = app.subscription();
        app.slot_mut("uptime-kuma")
            .unwrap()
            .connection
            .setup_attempt = 1;
        let _ = app.subscription();
        assert_eq!(app.plugins.len(), 2);
    }

    /// Regressão: `Farol::subscription` panicava em runtime
    /// (`iced::Subscription::map` — "the closure ... is capturing") assim
    /// que pelo menos um plugin chegava a `PluginState::Ready`, porque o
    /// timer de refresh usava `iced::time::every(interval).map(move |_| ...)`
    /// capturando `plugin_name`. Esse branch (`if slot.connection.state ==
    /// PluginState::Ready`) só é alcançado com uma conexão `Ready` — nenhum
    /// outro teste deste módulo exercitava esse caminho (`Farol::default()`
    /// deixa todos os slots em `Starting`), o que é exatamente o gap de
    /// cobertura que deixou o bug passar despercebido até uma execução real
    /// do binário (ver AGENTS.md, "Armadilha real já corrigida:
    /// `iced::Subscription::map` exige closure não-capturante"). Não há como
    /// inspecionar a `Subscription` retornada fora do runtime `iced` — o
    /// teste serve apenas para garantir que a montagem não panica, mesma
    /// limitação documentada em `subscription_does_not_panic_after_setup_attempt_increments`.
    #[test]
    fn subscription_does_not_panic_with_a_ready_plugin() {
        let app = farol_with_widget(Some(5_000));
        assert_eq!(connection_state(&app, "git-local"), PluginState::Ready);
        let _ = app.subscription();
    }
}
