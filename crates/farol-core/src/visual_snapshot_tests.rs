//! Verificação visual declarativa (US4, T023-T025 da feature 003): extrai,
//! via a `Selector` API de `iced_test`, uma representação textual
//! determinística e estável de todo o texto visível de uma tela renderizada
//! do Farol (`Farol::view()`), e compara contra um snapshot de referência
//! commitado (`insta`) — sem nunca renderizar pixel nenhum, sem GPU, sem
//! display, sem Xvfb (`contracts/visual-snapshot-contract.md`).
//!
//! # Por que este módulo mora em `src/`, e não em `crates/farol-core/tests/`
//!
//! Mesmo motivo de `e2e_tests.rs` (achado N3 de `research.md` da feature
//! 003): `farol-core` é um crate só-`bin` (`[[bin]] name = "farol"`, sem
//! `src/lib.rs` nem target `lib`) — um teste de integração em `tests/`
//! compilaria como crate separado, que só consegue importar de um target
//! `lib`. Segue o mesmo padrão `#[cfg(test)] mod` dentro do próprio bin
//! target já usado por `update.rs`/`config_store.rs`/`secrets_store.rs`/
//! `e2e_tests.rs`.
//!
//! # `Simulator`, não `Emulator`
//!
//! `e2e_tests.rs` (Camada 1 do harness, US1) dirige o `Program` inteiro —
//! `Subscription`, processo filho real, handshake JSON-RPC — através do
//! `iced_test::Emulator`. Este módulo não precisa de nada disso: os três
//! `screen_id`s cobertos por T024 (`data-model.md` §3) são estados de
//! `Farol` construídos diretamente em memória (reaproveitando os
//! construtores de fixture já usados pelos testes unitários de `update.rs`,
//! ver [`dashboard_ready_state`]/[`setup_form_state`]/
//! [`version_incompatible_state`]), sem processo filho nem I/O assíncrona
//! nenhuma envolvida. `iced_test::simulator()`/`Simulator` constrói a árvore
//! de widgets de um único `Element<Message>` já pronto (`Farol::view()`) e
//! permite percorrê-la com um `Selector` — o par mais leve para esse caso, e
//! ainda assim usa exatamente o mesmo backend de renderização headless
//! (`iced_test::renderer::Headless`) já comprovado sem display/GPU pelo
//! `Emulator` no gate T004 desta mesma feature.
//!
//! # T023 — extração de texto determinística
//!
//! [`extract_visible_text`] percorre a árvore com um `Selector` (a
//! implementação de `Selector` para `FnMut(Candidate<'_>) -> Option<T>`, ver
//! `iced_selector::lib`) que **nunca** "encontra" nada — sempre devolve
//! `None`. Isso é o que faz o `Finder` de `iced_selector`
//! (`iced_selector::find`) visitar a árvore **inteira** em vez de parar no
//! primeiro widget de texto: `Selector::find`/`One::is_done` só encerra a
//! busca quando `select` devolve `Some` em algum candidato, o que nunca
//! acontece aqui. O texto de cada candidato relevante
//! (`Candidate::Text`/`Candidate::TextInput`) é acumulado como efeito
//! colateral do closure, na mesma ordem de travessia em profundidade que
//! `Finder` usa — a mesma ordem em que `view.rs` compõe `column!`/`row!`
//! (primeiro filho primeiro, sem reordenação artificial,
//! `contracts/visual-snapshot-contract.md`), portanto determinística e
//! estável entre execuções: o mesmo `Farol` sempre produz a mesma string,
//! byte a byte.
//!
//! Por que não usar `Simulator::find`/`Selector::find_all` diretamente:
//! `Simulator::find` só expõe a estratégia "primeiro encontrado"
//! (`Selector::find`, `Finder<One<S>>`) — `Selector::find_all` existe
//! (`Finder<All<S>>`), mas `Simulator` não tem um método público que o
//! aceite (o `UserInterface`/`renderer` internos de `Simulator` são campos
//! privados da própria `iced_test`, inacessíveis daqui). O selector "nunca
//! encontra nada, mas registra tudo que vê" contorna essa limitação sem
//! duplicar a montagem de `UserInterface`/renderer headless que `Simulator`
//! já faz.

use std::sync::{Arc, Mutex};

use farol_protocol::messages::{MonitorStatus, MonitorStatusItem};
use iced::Element;
use iced_test::selector::Candidate;
use iced_test::simulator;

use crate::model::{self, PluginState, UnavailableReason};
use crate::{Farol, Message};

/// T023: extrai todo o texto visível de um `Element<Message>` já construído
/// (o retorno de `Farol::view()` para um estado conhecido), em ordem de
/// composição estável — ver docstring do módulo para o mecanismo.
///
/// Cobre os dois tipos de widget que carregam texto legível nas três telas
/// de T024: `text!`/rótulos de `button` (viram `Candidate::Text`, já que um
/// `button` é um contêiner cujo `operate` repassa para o conteúdo) e
/// `text_input` (`Candidate::TextInput`, cujo `state.text()` devolve o
/// `value` atual OU o `placeholder` quando vazio —
/// `iced_core::widget::operation::TextInput::text`, exatamente o "com seu
/// `value`/placeholder" que `contracts/visual-snapshot-contract.md` pede).
/// Um `TextInput` vazio (`state.text()` vazio, ou seja, sem `value` nem
/// `placeholder`) não produz linha nenhuma — não há nada visível a
/// registrar.
fn extract_visible_text(element: Element<'_, Message>) -> String {
    let mut ui = simulator(element);

    let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&collected);

    // Fecho que nunca "seleciona" nada (sempre `None`) — ver docstring do
    // módulo sobre por que isso faz o `Finder` percorrer a árvore inteira em
    // vez de parar no primeiro achado.
    let selector = move |candidate: Candidate<'_>| -> Option<()> {
        let mut texts = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match candidate {
            Candidate::Text { content, .. } => texts.push(content.to_string()),
            Candidate::TextInput { state, .. } => {
                let text = state.text();
                if !text.is_empty() {
                    texts.push(text.to_string());
                }
            }
            _ => {}
        }
        None
    };

    // Resultado ignorado de propósito: este `Selector` nunca "encontra"
    // nada (sempre `None`), então `Simulator::find` sempre devolve
    // `Err(SelectorNotFound)` — o que importa é o efeito colateral
    // acumulado em `collected` durante a travessia completa da árvore.
    let _ = ui.find(selector);

    let texts = Arc::try_unwrap(collected)
        .expect("nenhum outro Arc deveria sobreviver depois de Simulator::find retornar")
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    texts.join("\n")
}

// ---------------------------------------------------------------------------
// T024 — os três construtores de estado (`data-model.md` §3)
// ---------------------------------------------------------------------------

/// `screen_id` `DashboardReady` (`data-model.md` §3): pelo menos um plugin
/// `Ready` com widget populado — aqui, `uptime-kuma` com o widget
/// `monitor-status-grid`.
///
/// Reaproveita `update::tests::farol_with_monitor_widget` (T024, tornado
/// `pub(crate)`) para a identidade/capacidades/declaração do widget — o que
/// falta ali (`monitor_widget.monitors` vazio, porque aquele construtor
/// serve também a testes que não precisam de dados) é completado aqui com
/// dois monitores conhecidos, deliberadamente incluindo o sentinela `-1`
/// (`response_time_ms: None`, T006/`research.md` D3) para esta tela também
/// exercitar essa formatação ("—", nunca "0 ms").
fn dashboard_ready_state() -> Farol {
    let mut app = crate::update::tests::farol_with_monitor_widget();

    let slot = app
        .plugins
        .iter_mut()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("uptime-kuma é um plugin conhecido");
    slot.connection.monitor_widget.monitors = vec![
        MonitorStatusItem {
            name: "farol-api".to_string(),
            status: MonitorStatus::Up,
            response_time_ms: Some(42),
        },
        MonitorStatusItem {
            name: "farol-db".to_string(),
            status: MonitorStatus::Down,
            response_time_ms: Some(7),
        },
        MonitorStatusItem {
            name: "farol-container".to_string(),
            status: MonitorStatus::Up,
            response_time_ms: None,
        },
    ];

    app
}

/// `screen_id` `SetupForm` (`data-model.md` §3):
/// `Unavailable{NotConfigured}` — a tela de setup construída por
/// `view_setup_form` (T035 da feature 002), visível quando
/// `PluginConnection::setup_form` está preenchido.
///
/// Reaproveita `update::tests::sample_required_config` (T024, tornado
/// `pub(crate)`) para os dois campos declarados (`base_url`/`api_key`) —
/// os mesmos que o handshake real de `uptime-kuma` declara
/// (`sample_handshake_result`, `update.rs`), evitando uma segunda lista
/// divergente de `RequiredConfigItem`. Os valores começam vazios (mesma
/// convenção de `update::handle_handshake_outcome`: um par
/// `(RequiredConfigItem, "")` por item), então o texto visível desta tela
/// vem inteiramente dos rótulos/placeholders — determinístico sem depender
/// de nenhuma entrada de usuário simulada.
fn setup_form_state() -> Farol {
    let mut app = Farol::default();

    let slot = app
        .plugins
        .iter_mut()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("uptime-kuma é um plugin conhecido");
    let fields = crate::update::tests::sample_required_config()
        .into_iter()
        .map(|item| (item, String::new()))
        .collect();
    slot.connection.setup_form = Some(model::SetupForm {
        plugin_name: "uptime-kuma".to_string(),
        fields,
    });
    slot.connection.state = PluginState::Unavailable {
        reason: UnavailableReason::NotConfigured,
        detail: "required_config incompleto (fixture de snapshot visual)".to_string(),
    };

    app
}

/// `screen_id` `VersionIncompatible` (`data-model.md` §3): o estado que
/// `git-local` sempre alcança contra este core (débito técnico #4,
/// `AGENTS.md`/`e2e_tests.rs`) — protocolo `"0.1"` do plugin contra `"0.2"`
/// do core, `Unavailable{VersionIncompatible}` sob o regime `MAJOR == 0` de
/// `ProtocolVersion::is_compatible_with` (D7 da feature 001).
///
/// Sem construtor dedicado em `update.rs` a reaproveitar aqui (o estado é
/// montado inline em cada teste que o exercita, ex.
/// `handshake_version_incompatible_outcome_transitions_to_unavailable`) —
/// construído diretamente por mutação de campo, no mesmo estilo que aqueles
/// testes já usam (`PluginState`/`UnavailableReason` são tipos `pub` de
/// `model.rs`, campos de `PluginConnection`/`PluginSlot` já acessíveis
/// crate-wide, mesmo padrão que `e2e_tests.rs::plugin_state` usa para ler
/// `slot.connection.state`).
fn version_incompatible_state() -> Farol {
    let mut app = Farol::default();

    let slot = app
        .plugins
        .iter_mut()
        .find(|slot| slot.spawn_config.plugin_name == "git-local")
        .expect("git-local é um plugin conhecido");
    slot.connection.state = PluginState::Unavailable {
        reason: UnavailableReason::VersionIncompatible,
        detail: "plugin fala protocolo 0.1, core fala 0.2 (débito técnico #4)".to_string(),
    };

    app
}

/// `screen_id` `MonitorWidgetError` (T045, `specs/002-uptime-kuma-plugin/tasks.md`) — extensão do
/// conjunto de `data-model.md` §3 (FR-013 de `visual-snapshot-contract.md`: um novo `screen_id` é
/// só mais um construtor + `assert_snapshot!`, sem mudar o mecanismo).
///
/// Cobre o estado que só os testes e2e (`e2e_tests.rs`, T037/T040/T041 — lentos, processo real)
/// exercitavam até aqui: `monitor_widget.last_error` preenchido, **distinto** tanto do estado "0
/// monitores, sem erro" (T039, `Nenhum monitor cadastrado nesta instância.`) quanto de "lista
/// populada, sem erro" ([`dashboard_ready_state`]) — `view_monitor_grid`
/// (`crates/farol-core/src/view.rs`) MUST mostrar só o texto do erro quando não há lista prévia
/// preservada (FR-008/FR-013/FR-014/SC-001/SC-005 de `specs/002-uptime-kuma-plugin/spec.md`, ver a
/// docstring de `view_monitor_grid`). Além disso, esta fixture preserva um monitor de uma leitura
/// anterior bem-sucedida (`data-model.md` §3.1: erro pontual não apaga `monitors`, FR-017) — o
/// mesmo cenário provado ao vivo por T040, aqui como snapshot rápido, sem processo filho.
fn monitor_widget_error_state() -> Farol {
    let mut app = crate::update::tests::farol_with_monitor_widget();

    let slot = app
        .plugins
        .iter_mut()
        .find(|slot| slot.spawn_config.plugin_name == "uptime-kuma")
        .expect("uptime-kuma é um plugin conhecido");
    slot.connection.monitor_widget.monitors = vec![MonitorStatusItem {
        name: "farol-api".to_string(),
        status: MonitorStatus::Up,
        response_time_ms: Some(42),
    }];
    slot.connection.monitor_widget.last_error =
        Some("falha ao consultar /metrics da instância Uptime Kuma configurada".to_string());

    app
}

/// `screen_id` `VpnWidgetConnected` (T027, `specs/004-vpn-status-plugin/tasks.md`, extensão do
/// conjunto de `data-model.md` §3 via FR-013 de `visual-snapshot-contract.md`, mesmo mecanismo de
/// [`monitor_widget_error_state`]) — o widget `vpn-status` (`openfortivpn-vpn`) `Ready` e populado
/// no estado `connected`: perfil ativo, e a lista de perfis conhecidos (o próprio perfil ativo
/// inclusive — `data-model.md` §1.3: "não há necessidade de excluir o ativo da lista, ele
/// simplesmente aparece com `connect_action.enabled == false`").
///
/// Construído diretamente em memória (sem processo filho — mesma filosofia do módulo, ver
/// docstring de topo, e de [`dashboard_ready_state`]), preenchendo `PluginConnection::vpn_widget`
/// do slot `openfortivpn-vpn` (`known_plugins()` já registra este terceiro plugin) com um
/// `VpnStatusItem` equivalente ao que `plugins/openfortivpn-vpn/vpn_cli.py::query_status` mapearia
/// a partir de uma sessão `connected` real — mesmos valores simulados por
/// `tests/fixtures/fake-openfortivpn-gui/` no cenário e2e irmão
/// (`e2e_tests.rs::openfortivpn_vpn_reaches_ready_and_populates_the_vpn_widget`, T026), para as
/// duas verificações (snapshot rápido aqui, ciclo de protocolo real ali) afirmarem sobre o mesmo
/// estado de domínio.
///
/// `elapsed_seconds` é populado (`Some(3725.0)`, i.e. 1h 2min 5s) por fidelidade ao shape real de
/// um `VpnStatusItem` `Connected` (`data-model.md` §1.3: obrigatório quando `Connected`) e,
/// desde T034/US3, é renderizado por `format_elapsed` como "Conectado há: 1h 2min" — um valor de
/// horas deliberadamente não-trivial para deixar a formatação óbvia no snapshot (em vez de um
/// valor pequeno que só exercitaria o ramo de segundos).
fn vpn_widget_connected_state() -> Farol {
    use farol_protocol::messages::{
        Capability, KnownCapability, VpnConnectionState, VpnProfile, VpnStatusItem,
    };
    use farol_protocol::{ActionDeclaration, ActionTarget, CapabilityManifest, ProtocolVersion};

    let mut app = Farol::default();
    let slot = app
        .plugins
        .iter_mut()
        .find(|slot| slot.spawn_config.plugin_name == "openfortivpn-vpn")
        .expect("openfortivpn-vpn é um plugin conhecido (known_plugins(), T016)");

    slot.connection.state = PluginState::Ready;
    slot.connection.identity = Some(model::PluginIdentity {
        plugin_name: "openfortivpn-vpn".to_string(),
        protocol_version: ProtocolVersion::new(0, 3),
        capabilities: CapabilityManifest {
            capabilities: vec![Capability::Known(KnownCapability::Exec)],
        },
    });
    slot.connection.widgets = vec![farol_protocol::WidgetDeclaration {
        id: "vpn-connection".to_string(),
        kind: "vpn-status".to_string(),
        title: "VPN".to_string(),
        suggested_refresh_interval_ms: None,
    }];

    let connect_action = |profile: &str| ActionDeclaration {
        id: "vpn.connect".to_string(),
        label: format!("Conectar a {profile}"),
        target: ActionTarget {
            r#type: "vpn-profile".to_string(),
            id: profile.to_string(),
        },
        // `state == Connected` ⟹ todo `connect_action.enabled == false` (D4, `data-model.md`
        // §1.3), inclusive para o perfil já ativo.
        enabled: false,
        timeout_hint_ms: None,
    };

    slot.connection.vpn_widget.status = Some(VpnStatusItem {
        state: VpnConnectionState::Connected,
        active_profile: Some("escritorio".to_string()),
        elapsed_seconds: Some(3725.0),
        available_profiles: vec![
            VpnProfile {
                name: "escritorio".to_string(),
                connect_action: connect_action("escritorio"),
            },
            VpnProfile {
                name: "casa".to_string(),
                connect_action: connect_action("casa"),
            },
        ],
        disconnect_action: ActionDeclaration {
            id: "vpn.disconnect".to_string(),
            label: "Desconectar".to_string(),
            target: ActionTarget {
                r#type: "vpn-connection".to_string(),
                id: "active".to_string(),
            },
            enabled: true,
            timeout_hint_ms: None,
        },
    });

    app
}

// ---------------------------------------------------------------------------
// Um `insta::assert_snapshot!` por `screen_id`
// ---------------------------------------------------------------------------

#[test]
fn dashboard_ready_screen_matches_snapshot() {
    let app = dashboard_ready_state();
    insta::assert_snapshot!("DashboardReady", extract_visible_text(app.view()));
}

#[test]
fn setup_form_screen_matches_snapshot() {
    let app = setup_form_state();
    insta::assert_snapshot!("SetupForm", extract_visible_text(app.view()));
}

#[test]
fn version_incompatible_screen_matches_snapshot() {
    let app = version_incompatible_state();
    insta::assert_snapshot!("VersionIncompatible", extract_visible_text(app.view()));
}

#[test]
fn monitor_widget_error_screen_matches_snapshot() {
    let app = monitor_widget_error_state();
    insta::assert_snapshot!("MonitorWidgetError", extract_visible_text(app.view()));
}

#[test]
fn vpn_widget_connected_screen_matches_snapshot() {
    let app = vpn_widget_connected_state();
    insta::assert_snapshot!("VpnWidgetConnected", extract_visible_text(app.view()));
}
