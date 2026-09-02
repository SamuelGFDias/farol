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
