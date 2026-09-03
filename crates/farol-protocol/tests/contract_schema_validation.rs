//! Teste de contrato: valida que o JSON produzido/aceito pelos tipos de `farol_protocol::messages`
//! é genuinamente válido contra os 4 JSON Schemas normativos em `protocol/schema/v0.3/*.schema.json`
//! — não apenas "o Rust concorda consigo mesmo" (isso já é coberto pelos testes de unidade internos
//! de round-trip em `src/framing.rs`, `src/version.rs` e `src/messages.rs`), mas "o Rust concorda
//! com o contrato normativo do protocolo".
//!
//! ## Correção H3 (`specs/002-uptime-kuma-plugin/tasks.md` T043)
//!
//! Este arquivo validava originalmente contra `protocol/schema/v0.1/*.schema.json`, usando o
//! formato antigo de `CapabilityManifest.capabilities` (`Vec<String>`) e `WidgetGetResult.items`
//! (`Vec<WidgetItem>` fixo). Os tipos de `farol_protocol::messages` evoluíram para o protocolo
//! `"0.2"` (`Capability` discriminado por `kind`, `RequiredConfigItem`/`required_config`,
//! `MonitorStatusItem` e `WidgetItems` como união discriminada) — o formato antigo não compila
//! mais contra esses tipos. Passou então a carregar e validar contra
//! `protocol/schema/v0.2/*.schema.json`.
//!
//! ## T011 (`specs/004-vpn-status-plugin/tasks.md`) — migração para `v0.3`
//!
//! Bump aditivo (`research.md` D2 da feature 004): `ActionInvokeResult` generaliza para `oneOf`/
//! enum untagged (`Git`/`Vpn`) e `WidgetItems` ganha a variante `Vpn`. O wire já emitido por
//! `git-local`/`uptime-kuma` continua validando sem alteração nenhuma — só os 4 `include_str!`/
//! `$id` deste arquivo migram para `protocol/schema/v0.3/*.schema.json`, e dois exemplos novos são
//! adicionados (`VpnStatusItem` em `WidgetGetResult`, `ActionInvokeResult::Vpn`), no mesmo padrão
//! dos exemplos já existentes para `GitRepository`/`MonitorStatusItem`. `protocol/schema/v0.1/` e
//! `v0.2/` permanecem intocados como registro histórico do formato que `git-local`/`uptime-kuma`
//! (até a migração da própria constante `PROTOCOL_VERSION`, fora do escopo desta task) ainda
//! falam.
//!
//! ## Por que este arquivo mora aqui e não em `tests/contract/` na raiz do workspace
//!
//! A task original (T047, feature 001) e `tests/contract/README.md` (na raiz do workspace)
//! descrevem `tests/contract/` como o diretório conceitual dos testes de contrato do protocolo. Só
//! que o Cargo só reconhece testes de integração dentro de `<crate>/tests/*.rs` — um diretório
//! `tests/` solto na raiz do workspace (fora de qualquer crate) não é compilado nem rodado por
//! `cargo test`, seja qual for o layout de dentro dele. Como este teste depende diretamente dos
//! tipos de `farol_protocol` (crate de biblioteca), a única forma de fazê-lo rodar via
//! `cargo test` é como teste de integração dentro do próprio crate, em
//! `crates/farol-protocol/tests/`. O `tests/contract/README.md` da raiz foi atualizado para apontar
//! para cá e explicar essa limitação — não é uma decisão arbitrária de layout, é a única forma de o
//! Cargo enxergar o teste.
//!
//! ## Como os 4 schemas são carregados
//!
//! `handshake.schema.json`, `widget.schema.json`, `action.schema.json` e `error.schema.json` se
//! referenciam entre si por `$id`/`$ref` (ex.: `widget.schema.json` referencia
//! `ActionDeclaration` de `handshake.schema.json` por URL absoluta). Por isso os 4 são carregados
//! juntos e registrados num único `jsonschema::Registry` antes de qualquer validação — validar um
//! schema isolado, sem os outros três no registry, falharia ao resolver esses `$ref` cruzados.

use jsonschema::{Registry, Validator};
use serde_json::{json, Value};

// `Capability`, `KnownCapability`, `RequiredConfigItem`, `MonitorStatus`, `MonitorStatusItem`,
// `WidgetItems` (v0.2) e `VpnConnectionState`/`VpnProfile`/`VpnStatusItem` (novos em v0.3, T008)
// ainda não são reexportados na raiz do crate (`src/lib.rs`, fora do escopo desta task — só este
// arquivo de teste é editado aqui) — importados via `farol_protocol::messages` diretamente, que já
// os declara `pub`.
use farol_protocol::messages::{
    Capability, KnownCapability, MonitorStatus, MonitorStatusItem, RequiredConfigItem,
    VpnConnectionState, VpnProfile, VpnStatusItem, WidgetItems,
};
use farol_protocol::{
    ActionDeclaration, ActionInvokeParams, ActionInvokeRequest, ActionInvokeResponse,
    ActionInvokeResult, ActionTarget, CapabilityManifest, ErrorData, ErrorObject, GitRepository,
    HandshakeHello, HandshakeHelloRequest, HandshakeHelloResponse, HandshakeHelloResult,
    ProtocolVersion, RemoteStatus, RequestId, WidgetDeclaration, WidgetGetParams, WidgetGetRequest,
    WidgetGetResponse, WidgetGetResult, WidgetItem,
};

// Conteúdo bruto dos 4 schemas normativos, embutido em tempo de compilação. Caminho relativo a
// este arquivo: `crates/farol-protocol/tests/` -> raiz do workspace -> `protocol/schema/v0.3/`.
const HANDSHAKE_SCHEMA: &str = include_str!("../../../protocol/schema/v0.3/handshake.schema.json");
const WIDGET_SCHEMA: &str = include_str!("../../../protocol/schema/v0.3/widget.schema.json");
const ACTION_SCHEMA: &str = include_str!("../../../protocol/schema/v0.3/action.schema.json");
const ERROR_SCHEMA: &str = include_str!("../../../protocol/schema/v0.3/error.schema.json");

/// Registra os 4 schemas juntos (por causa do `$ref` cruzado entre eles) e devolve um validador
/// para cada um, montado sobre esse registry compartilhado.
struct Schemas {
    handshake: Validator,
    widget: Validator,
    action: Validator,
    error: Validator,
}

fn load_schemas() -> Schemas {
    let handshake_value: Value =
        serde_json::from_str(HANDSHAKE_SCHEMA).expect("handshake.schema.json inválido");
    let widget_value: Value =
        serde_json::from_str(WIDGET_SCHEMA).expect("widget.schema.json inválido");
    let action_value: Value =
        serde_json::from_str(ACTION_SCHEMA).expect("action.schema.json inválido");
    let error_value: Value =
        serde_json::from_str(ERROR_SCHEMA).expect("error.schema.json inválido");

    // Os 4 documentos são registrados sob suas próprias URLs `$id` para que `$ref` absolutos
    // (ex.: `https://farol.dev/protocol/v0.3/error.schema.json`) resolvam entre eles.
    let registry = Registry::new()
        .add(
            "https://farol.dev/protocol/v0.3/handshake.schema.json",
            handshake_value.clone(),
        )
        .expect("URI de handshake.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.3/widget.schema.json",
            widget_value.clone(),
        )
        .expect("URI de widget.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.3/action.schema.json",
            action_value.clone(),
        )
        .expect("URI de action.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.3/error.schema.json",
            error_value.clone(),
        )
        .expect("URI de error.schema.json inválida")
        .prepare()
        .expect("registry deveria preparar com sucesso (schemas consistentes entre si)");

    let build = |schema: &Value| {
        jsonschema::options()
            .with_registry(&registry)
            .build(schema)
            .expect("schema deveria compilar contra Draft 2020-12")
    };

    Schemas {
        handshake: build(&handshake_value),
        widget: build(&widget_value),
        action: build(&action_value),
        error: build(&error_value),
    }
}

/// Falha o teste com uma mensagem clara, listando todos os erros de validação encontrados —
/// nunca apenas "invalid", para que uma regressão real seja diagnosticável direto na saída do
/// `cargo test`.
fn assert_valid(validator: &Validator, instance: &Value, schema_name: &str, case: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} (em {})", e, e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "esperava que a instância do caso '{case}' fosse válida contra {schema_name}, mas falhou:\n{}\n\ninstância: {}",
        errors.join("\n"),
        serde_json::to_string_pretty(instance).unwrap(),
    );
}

/// Confirma que o validador REJEITA uma instância propositalmente inválida — evita um teste que
/// "valida sempre true" por engano de configuração (ex.: registry vazio, draft errado).
fn assert_invalid(validator: &Validator, instance: &Value, schema_name: &str, case: &str) {
    assert!(
        !validator.is_valid(instance),
        "esperava que a instância do caso negativo '{case}' fosse REJEITADA por {schema_name}, mas foi aceita:\n{}",
        serde_json::to_string_pretty(instance).unwrap(),
    );
}

// -------------------------------------------------------------------------------------------
// handshake.schema.json
// -------------------------------------------------------------------------------------------

#[test]
fn handshake_hello_request_matches_schema() {
    let schemas = load_schemas();
    let req = HandshakeHelloRequest::new(
        RequestId::Integer(1),
        HandshakeHello {
            protocol_version: ProtocolVersion::new(0, 2),
            core_name: "farol-core".to_string(),
        },
    );
    let instance = serde_json::to_value(&req).unwrap();
    assert_valid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloRequest positivo",
    );
}

#[test]
fn handshake_hello_response_success_matches_schema() {
    let schemas = load_schemas();
    let resp = HandshakeHelloResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(1),
        result: HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 2),
            plugin_name: "git-local".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec![Capability::Known(KnownCapability::Exec)],
            },
            required_config: vec![],
            widgets: vec![WidgetDeclaration {
                id: "repo-status".to_string(),
                kind: "status-grid".to_string(),
                title: "Repositórios Git".to_string(),
                suggested_refresh_interval_ms: None,
            }],
            actions: vec![],
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloResponse::Success positivo (capability exec)",
    );
}

/// Caso positivo novo: capacidade `network` (`host`/`port`), declarada por um plugin como
/// `uptime-kuma` quando `base_url` já foi resolvido (`research.md` D1/D8). Junto com
/// `required_config` não-vazio, cobre a nova forma estruturada de `Capability` introduzida em
/// v0.2 (correção H3, T043).
#[test]
fn handshake_hello_response_success_with_network_capability_matches_schema() {
    let schemas = load_schemas();
    let resp = HandshakeHelloResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(2),
        result: HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 2),
            plugin_name: "uptime-kuma".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec![Capability::Known(KnownCapability::Network {
                    host: "kuma.example.com".to_string(),
                    port: Some(443),
                })],
            },
            required_config: vec![],
            widgets: vec![WidgetDeclaration {
                id: "uptime-kuma-monitors".to_string(),
                kind: "monitor-status-grid".to_string(),
                title: "Uptime Kuma".to_string(),
                suggested_refresh_interval_ms: Some(30000),
            }],
            actions: vec![],
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloResponse::Success com capability network",
    );
}

/// Caso positivo novo: `required_config` não-vazio (`RequiredConfigItem`), um item não-secreto
/// (`base_url`) e um secreto (`api_key`) — forma que `uptime-kuma` sempre declara,
/// independentemente de já haver valor armazenado (`research.md` D8).
#[test]
fn handshake_hello_response_success_with_required_config_matches_schema() {
    let schemas = load_schemas();
    let resp = HandshakeHelloResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(3),
        result: HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 2),
            plugin_name: "uptime-kuma".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec![],
            },
            required_config: vec![
                RequiredConfigItem {
                    name: "base_url".to_string(),
                    secret: false,
                    description: "URL base da instância Uptime Kuma".to_string(),
                },
                RequiredConfigItem {
                    name: "api_key".to_string(),
                    secret: true,
                    description: "API Key da instância Uptime Kuma".to_string(),
                },
            ],
            widgets: vec![WidgetDeclaration {
                id: "uptime-kuma-monitors".to_string(),
                kind: "monitor-status-grid".to_string(),
                title: "Uptime Kuma".to_string(),
                suggested_refresh_interval_ms: Some(30000),
            }],
            actions: vec![],
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloResponse::Success com required_config não-vazio",
    );
}

#[test]
fn handshake_hello_response_error_matches_schema() {
    let schemas = load_schemas();
    let resp = HandshakeHelloResponse::Error {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(1),
        error: ErrorObject {
            code: -32000,
            message: "versao incompativel".to_string(),
            data: Some(ErrorData {
                reason: Some("protocol_version_incompatible".to_string()),
                extra: Default::default(),
            }),
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloResponse::Error positivo",
    );
}

/// Caso negativo: `HandshakeHelloParams` sem `core_name` (campo obrigatório) — nenhuma das 3
/// alternativas do `oneOf` (request/success/error) deve casar.
#[test]
fn handshake_request_missing_core_name_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "handshake/hello",
        "params": {
            "protocol_version": "0.2"
            // "core_name" ausente de propósito
        }
    });
    assert_invalid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloRequest sem core_name",
    );
}

/// Caso negativo: `HandshakeHelloResult` sem `required_config` (campo obrigatório em v0.2,
/// diferente de v0.1) — confirma que o schema realmente exige o campo novo, não só que os tipos
/// Rust o preenchem.
#[test]
fn handshake_result_missing_required_config_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "protocol_version": "0.2",
            "plugin_name": "uptime-kuma",
            "capabilities": { "capabilities": [] },
            // "required_config" ausente de propósito
            "widgets": [],
            "actions": []
        }
    });
    assert_invalid(
        &schemas.handshake,
        &instance,
        "handshake.schema.json",
        "HandshakeHelloResult sem required_config",
    );
}

// -------------------------------------------------------------------------------------------
// widget.schema.json
// -------------------------------------------------------------------------------------------

#[test]
fn widget_get_request_matches_schema() {
    let schemas = load_schemas();
    let req = WidgetGetRequest::new(
        RequestId::String("req-1".to_string()),
        WidgetGetParams {
            widget_id: "repo-status".to_string(),
        },
    );
    let instance = serde_json::to_value(&req).unwrap();
    assert_valid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "WidgetGetRequest positivo",
    );
}

#[test]
fn widget_get_result_matches_schema() {
    let schemas = load_schemas();
    let resp = WidgetGetResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(2),
        result: WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: WidgetItems::Git(vec![WidgetItem {
                repo: GitRepository {
                    id: "/home/dev/projetos/farol".to_string(),
                    name: "farol".to_string(),
                    path: "/home/dev/projetos/farol".to_string(),
                    dirty: true,
                    remote_status: RemoteStatus::Tracked {
                        ahead: 1,
                        behind: 0,
                    },
                },
                fetch_action: ActionDeclaration {
                    id: "git.fetch".to_string(),
                    label: "Fetch".to_string(),
                    target: ActionTarget {
                        r#type: "repo".to_string(),
                        id: "/home/dev/projetos/farol".to_string(),
                    },
                    enabled: true,
                    timeout_hint_ms: None,
                },
            }]),
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "WidgetGetResult positivo (com item git, WidgetItems::Git)",
    );
}

/// Caso positivo novo: `WidgetGetResult` com `items: WidgetItems::Monitor(...)` — o novo
/// vocabulário `MonitorStatusItem` para widgets `kind: "monitor-status-grid"` (correção C3,
/// exercitado aqui contra `widget.schema.json` v0.2 pela primeira vez neste arquivo).
#[test]
fn widget_get_result_with_monitor_items_matches_schema() {
    let schemas = load_schemas();
    let resp = WidgetGetResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(4),
        result: WidgetGetResult {
            widget_id: "uptime-kuma-monitors".to_string(),
            items: WidgetItems::Monitor(vec![
                MonitorStatusItem {
                    name: "api_example_com".to_string(),
                    status: MonitorStatus::Up,
                    response_time_ms: Some(42),
                },
                MonitorStatusItem {
                    name: "internal_service".to_string(),
                    status: MonitorStatus::Down,
                    response_time_ms: None,
                },
            ]),
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "WidgetGetResult positivo (monitor-status-grid, WidgetItems::Monitor)",
    );
}

/// Caso positivo novo (T011, feature 004): `WidgetGetResult` com `items: WidgetItems::Vpn(...)` —
/// o novo vocabulário `VpnStatusItem` para um widget `kind: "vpn-status"` (`research.md` D2-D4,
/// `data-model.md` §1.1-§1.4), exercitado aqui contra `widget.schema.json` v0.3 pela primeira vez
/// neste arquivo. `state == Connected`, então (invariante de `data-model.md` §1.3)
/// `active_profile`/`elapsed_seconds` são `Some` e `disconnect_action.enabled == true`; os dois
/// `VpnProfile.connect_action` ficam `enabled == false` (só um perfil ativo por vez, D3/D4).
#[test]
fn widget_get_result_with_vpn_item_matches_schema() {
    let schemas = load_schemas();
    let resp = WidgetGetResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(5),
        result: WidgetGetResult {
            widget_id: "vpn-connection".to_string(),
            items: WidgetItems::Vpn(vec![VpnStatusItem {
                state: VpnConnectionState::Connected,
                active_profile: Some("work-vpn".to_string()),
                elapsed_seconds: Some(125.5),
                available_profiles: vec![
                    VpnProfile {
                        name: "work-vpn".to_string(),
                        connect_action: ActionDeclaration {
                            id: "vpn.connect".to_string(),
                            label: "Conectar".to_string(),
                            target: ActionTarget {
                                r#type: "vpn-profile".to_string(),
                                id: "work-vpn".to_string(),
                            },
                            enabled: false,
                            timeout_hint_ms: None,
                        },
                    },
                    VpnProfile {
                        name: "home-vpn".to_string(),
                        connect_action: ActionDeclaration {
                            id: "vpn.connect".to_string(),
                            label: "Conectar".to_string(),
                            target: ActionTarget {
                                r#type: "vpn-profile".to_string(),
                                id: "home-vpn".to_string(),
                            },
                            enabled: false,
                            timeout_hint_ms: None,
                        },
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
            }]),
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "WidgetGetResult positivo (vpn-status, WidgetItems::Vpn)",
    );
}

/// Caso positivo: `items: []` com `anyOf` (correção da definição em `widget.schema.json` v0.2).
/// Com `oneOf`, um array vazio satisfaria *ambas* as alternativas (`WidgetItem[]` vs.
/// `MonitorStatusItem[]`) — as duas ramas são `{"type":"array","items":{$ref ...}}` sem
/// discriminador algum e sem `minItems`, então um array vazio satisfaz ambas (nada a validar
/// contra `$ref` quando não há elementos). Mudanza `oneOf` → `anyOf` permite que `[]` seja
/// válido (precisa casar com pelo menos uma alternativa, não exatamente uma) e preserva a
/// discriminação no caso não-vazio (array de `WidgetItem` com propriedades obrigatórias
/// incompatíveis com `MonitorStatusItem`, e vice-versa).
#[test]
fn widget_get_result_with_empty_items_matches_schema_via_any_of() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "widget_id": "repo-status", "items": [] }
    });
    assert_valid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "WidgetGetResult com items vazio (anyOf discriminado)",
    );
}

/// Caso negativo: `GitRepository` sem `remote_status` (campo obrigatório).
#[test]
fn widget_get_result_missing_remote_status_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "widget_id": "repo-status",
            "items": [{
                "repo": {
                    "id": "/home/dev/projetos/farol",
                    "name": "farol",
                    "path": "/home/dev/projetos/farol",
                    "dirty": false
                    // "remote_status" ausente de propósito
                },
                "fetch_action": {
                    "id": "git.fetch",
                    "label": "Fetch",
                    "target": { "type": "repo", "id": "/home/dev/projetos/farol" },
                    "enabled": true
                }
            }]
        }
    });
    assert_invalid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "GitRepository sem remote_status",
    );
}

/// Caso negativo: `MonitorStatusItem` com `status` fora do vocabulário conhecido
/// (`"up"|"down"|"pending"|"maintenance"`).
#[test]
fn widget_get_result_monitor_item_with_invalid_status_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "widget_id": "uptime-kuma-monitors",
            "items": [{
                "name": "api_example_com",
                "status": "unknown_status",
                "response_time_ms": null
            }]
        }
    });
    assert_invalid(
        &schemas.widget,
        &instance,
        "widget.schema.json",
        "MonitorStatusItem com status inválido",
    );
}

// -------------------------------------------------------------------------------------------
// action.schema.json
// -------------------------------------------------------------------------------------------

#[test]
fn action_invoke_request_matches_schema() {
    let schemas = load_schemas();
    let req = ActionInvokeRequest::new(
        RequestId::Integer(4),
        ActionInvokeParams {
            action_id: "git.fetch".to_string(),
            target: ActionTarget {
                r#type: "repo".to_string(),
                id: "/home/dev/projetos/farol".to_string(),
            },
        },
    );
    let instance = serde_json::to_value(&req).unwrap();
    assert_valid(
        &schemas.action,
        &instance,
        "action.schema.json",
        "ActionInvokeRequest positivo",
    );
}

#[test]
fn action_invoke_response_success_matches_schema() {
    let schemas = load_schemas();
    let resp = ActionInvokeResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(4),
        result: ActionInvokeResult::Git {
            repo: GitRepository {
                id: "/home/dev/projetos/farol".to_string(),
                name: "farol".to_string(),
                path: "/home/dev/projetos/farol".to_string(),
                dirty: false,
                remote_status: RemoteStatus::NoRemote,
            },
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.action,
        &instance,
        "action.schema.json",
        "ActionInvokeResponse::Success positivo",
    );
}

/// Caso positivo novo (T011, feature 004): `ActionInvokeResponse::Success` com
/// `result: ActionInvokeResult::Vpn { vpn_status }` — o resultado devolvido por `vpn.connect`/
/// `vpn.disconnect` do plugin `openfortivpn-gui` (`research.md` D2/D4-D5, `data-model.md` §1.5),
/// exercitado aqui contra `action.schema.json` v0.3 pela primeira vez neste arquivo. O wire
/// `{"vpn_status": {...}}` é a segunda alternativa do `oneOf` de `ActionInvokeResult` — distinta,
/// sem ambiguidade, da primeira (`{"repo": {...}}`, `git.fetch`) exercitada no teste acima.
#[test]
fn action_invoke_response_success_with_vpn_status_matches_schema() {
    let schemas = load_schemas();
    let resp = ActionInvokeResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(6),
        result: ActionInvokeResult::Vpn {
            vpn_status: VpnStatusItem {
                state: VpnConnectionState::Disconnected,
                active_profile: None,
                elapsed_seconds: None,
                available_profiles: vec![VpnProfile {
                    name: "work-vpn".to_string(),
                    connect_action: ActionDeclaration {
                        id: "vpn.connect".to_string(),
                        label: "Conectar".to_string(),
                        target: ActionTarget {
                            r#type: "vpn-profile".to_string(),
                            id: "work-vpn".to_string(),
                        },
                        enabled: true,
                        timeout_hint_ms: None,
                    },
                }],
                disconnect_action: ActionDeclaration {
                    id: "vpn.disconnect".to_string(),
                    label: "Desconectar".to_string(),
                    target: ActionTarget {
                        r#type: "vpn-connection".to_string(),
                        id: "active".to_string(),
                    },
                    enabled: false,
                    timeout_hint_ms: None,
                },
            },
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.action,
        &instance,
        "action.schema.json",
        "ActionInvokeResponse::Success positivo (vpn.disconnect, ActionInvokeResult::Vpn)",
    );
}

#[test]
fn action_invoke_response_error_matches_schema() {
    let schemas = load_schemas();
    let resp = ActionInvokeResponse::Error {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(4),
        error: ErrorObject {
            code: -32001,
            message: "git fetch falhou".to_string(),
            data: Some(ErrorData {
                reason: Some("fetch_failed".to_string()),
                extra: Default::default(),
            }),
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(
        &schemas.action,
        &instance,
        "action.schema.json",
        "ActionInvokeResponse::Error positivo",
    );
}

/// Caso negativo: `ActionInvokeParams` sem `target` (campo obrigatório).
#[test]
fn action_invoke_request_missing_target_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "action/invoke",
        "params": {
            "action_id": "git.fetch"
            // "target" ausente de propósito
        }
    });
    assert_invalid(
        &schemas.action,
        &instance,
        "action.schema.json",
        "ActionInvokeRequest sem target",
    );
}

// -------------------------------------------------------------------------------------------
// error.schema.json
// -------------------------------------------------------------------------------------------

#[test]
fn error_object_matches_schema() {
    let schemas = load_schemas();
    let error = ErrorObject {
        code: -32004,
        message: "scan_root inacessível".to_string(),
        data: Some(ErrorData {
            reason: Some("scan_root_unreadable".to_string()),
            extra: Default::default(),
        }),
    };
    let instance = serde_json::to_value(&error).unwrap();
    assert_valid(
        &schemas.error,
        &instance,
        "error.schema.json",
        "ErrorObject positivo (com data.reason)",
    );
}

/// Caso positivo adicional: `ErrorObject` sem `data` — o campo é opcional no schema (só
/// requerido pela prosa normativa quando `code` é um código de domínio Farol, não pela validação
/// de JSON Schema em si).
#[test]
fn error_object_without_data_matches_schema() {
    let schemas = load_schemas();
    let error = ErrorObject {
        code: -32602,
        message: "invalid params".to_string(),
        data: None,
    };
    let instance = serde_json::to_value(&error).unwrap();
    assert_valid(
        &schemas.error,
        &instance,
        "error.schema.json",
        "ErrorObject positivo sem data",
    );
}

/// Caso positivo adicional: os três novos `reason`s de v0.2 (`not_configured`,
/// `metrics_unreachable`, `metrics_parse_error`, `research.md` D9) — o catálogo textual é novo,
/// mas a forma de `ErrorObject`/`ErrorData` não muda; confirma que o schema aceita `reason`
/// livremente (string aberta, `contracts/error-model-delta.md`).
#[test]
fn error_object_with_uptime_kuma_reasons_matches_schema() {
    let schemas = load_schemas();
    for (code, reason) in [
        (-32005, "not_configured"),
        (-32006, "metrics_unreachable"),
        (-32007, "metrics_parse_error"),
    ] {
        let error = ErrorObject {
            code,
            message: format!("erro de domínio: {reason}"),
            data: Some(ErrorData {
                reason: Some(reason.to_string()),
                extra: Default::default(),
            }),
        };
        let instance = serde_json::to_value(&error).unwrap();
        assert_valid(
            &schemas.error,
            &instance,
            "error.schema.json",
            &format!("ErrorObject com reason '{reason}'"),
        );
    }
}

/// Caso negativo: `ErrorObject` sem `message` (campo obrigatório).
#[test]
fn error_object_missing_message_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "code": -32000
        // "message" ausente de propósito
    });
    assert_invalid(
        &schemas.error,
        &instance,
        "error.schema.json",
        "ErrorObject sem message",
    );
}
