//! Teste de contrato (T047): valida que o JSON produzido/aceito pelos tipos de
//! `farol_protocol::messages` é genuinamente válido contra os 4 JSON Schemas normativos em
//! `protocol/schema/v0.1/*.schema.json` — não apenas "o Rust concorda consigo mesmo" (isso já é
//! coberto pelos testes de unidade internos de round-trip em `src/framing.rs`, `src/version.rs` e
//! `src/messages.rs`), mas "o Rust concorda com o contrato normativo do protocolo".
//!
//! ## Por que este arquivo mora aqui e não em `tests/contract/` na raiz do workspace
//!
//! A task T047 e `tests/contract/README.md` (na raiz do workspace) descrevem `tests/contract/`
//! como o diretório conceitual dos testes de contrato do protocolo. Só que o Cargo só reconhece
//! testes de integração dentro de `<crate>/tests/*.rs` — um diretório `tests/` solto na raiz do
//! workspace (fora de qualquer crate) não é compilado nem rodado por `cargo test`, seja qual for
//! o layout de dentro dele. Como este teste depende diretamente dos tipos de `farol_protocol`
//! (crate de biblioteca), a única forma de fazê-lo rodar via `cargo test` é como teste de
//! integração dentro do próprio crate, em `crates/farol-protocol/tests/`. O
//! `tests/contract/README.md` da raiz foi atualizado para apontar para cá e explicar essa
//! limitação — não é uma decisão arbitrária de layout, é a única forma de o Cargo enxergar o
//! teste.
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

use farol_protocol::{
    ActionDeclaration, ActionInvokeParams, ActionInvokeRequest, ActionInvokeResponse,
    ActionInvokeResult, ActionTarget, CapabilityManifest, ErrorData, ErrorObject, GitRepository,
    HandshakeHello, HandshakeHelloRequest, HandshakeHelloResponse, HandshakeHelloResult,
    ProtocolVersion, RemoteStatus, RequestId, WidgetDeclaration, WidgetGetParams,
    WidgetGetRequest, WidgetGetResponse, WidgetGetResult, WidgetItem,
};

// Conteúdo bruto dos 4 schemas normativos, embutido em tempo de compilação. Caminho relativo a
// este arquivo: `crates/farol-protocol/tests/` -> raiz do workspace -> `protocol/schema/v0.1/`.
const HANDSHAKE_SCHEMA: &str = include_str!("../../../protocol/schema/v0.1/handshake.schema.json");
const WIDGET_SCHEMA: &str = include_str!("../../../protocol/schema/v0.1/widget.schema.json");
const ACTION_SCHEMA: &str = include_str!("../../../protocol/schema/v0.1/action.schema.json");
const ERROR_SCHEMA: &str = include_str!("../../../protocol/schema/v0.1/error.schema.json");

/// Registra os 4 schemas juntos (por causa do `$ref` cruzado entre eles) e devolve um validador
/// para cada um, montado sobre esse registry compartilhado.
struct Schemas {
    handshake: Validator,
    widget: Validator,
    action: Validator,
    error: Validator,
}

fn load_schemas() -> Schemas {
    let handshake_value: Value = serde_json::from_str(HANDSHAKE_SCHEMA).expect("handshake.schema.json inválido");
    let widget_value: Value = serde_json::from_str(WIDGET_SCHEMA).expect("widget.schema.json inválido");
    let action_value: Value = serde_json::from_str(ACTION_SCHEMA).expect("action.schema.json inválido");
    let error_value: Value = serde_json::from_str(ERROR_SCHEMA).expect("error.schema.json inválido");

    // Os 4 documentos são registrados sob suas próprias URLs `$id` para que `$ref` absolutos
    // (ex.: `https://farol.dev/protocol/v0.1/error.schema.json`) resolvam entre eles.
    let registry = Registry::new()
        .add(
            "https://farol.dev/protocol/v0.1/handshake.schema.json",
            handshake_value.clone(),
        )
        .expect("URI de handshake.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.1/widget.schema.json",
            widget_value.clone(),
        )
        .expect("URI de widget.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.1/action.schema.json",
            action_value.clone(),
        )
        .expect("URI de action.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.1/error.schema.json",
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
            protocol_version: ProtocolVersion::new(0, 1),
            core_name: "farol-core".to_string(),
        },
    );
    let instance = serde_json::to_value(&req).unwrap();
    assert_valid(&schemas.handshake, &instance, "handshake.schema.json", "HandshakeHelloRequest positivo");
}

#[test]
fn handshake_hello_response_success_matches_schema() {
    let schemas = load_schemas();
    let resp = HandshakeHelloResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(1),
        result: HandshakeHelloResult {
            protocol_version: ProtocolVersion::new(0, 1),
            plugin_name: "git-local".to_string(),
            capabilities: CapabilityManifest {
                capabilities: vec!["exec".to_string()],
            },
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
    assert_valid(&schemas.handshake, &instance, "handshake.schema.json", "HandshakeHelloResponse::Success positivo");
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
    assert_valid(&schemas.handshake, &instance, "handshake.schema.json", "HandshakeHelloResponse::Error positivo");
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
            "protocol_version": "0.1"
            // "core_name" ausente de propósito
        }
    });
    assert_invalid(&schemas.handshake, &instance, "handshake.schema.json", "HandshakeHelloRequest sem core_name");
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
    assert_valid(&schemas.widget, &instance, "widget.schema.json", "WidgetGetRequest positivo");
}

#[test]
fn widget_get_result_matches_schema() {
    let schemas = load_schemas();
    let resp = WidgetGetResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(2),
        result: WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: vec![WidgetItem {
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
            }],
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(&schemas.widget, &instance, "widget.schema.json", "WidgetGetResult positivo (com item)");
}

/// Caso positivo adicional: `WidgetGetResult` com `items: []` — MAY ser vazia por desenho
/// (`protocol/SPEC.md`), não deve ser tratado como erro pelo schema.
#[test]
fn widget_get_result_with_empty_items_matches_schema() {
    let schemas = load_schemas();
    let resp = WidgetGetResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(3),
        result: WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: vec![],
        },
    };
    let instance = serde_json::to_value(&resp).unwrap();
    assert_valid(&schemas.widget, &instance, "widget.schema.json", "WidgetGetResult com items vazio");
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
    assert_invalid(&schemas.widget, &instance, "widget.schema.json", "GitRepository sem remote_status");
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
    assert_valid(&schemas.action, &instance, "action.schema.json", "ActionInvokeRequest positivo");
}

#[test]
fn action_invoke_response_success_matches_schema() {
    let schemas = load_schemas();
    let resp = ActionInvokeResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: RequestId::Integer(4),
        result: ActionInvokeResult {
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
    assert_valid(&schemas.action, &instance, "action.schema.json", "ActionInvokeResponse::Success positivo");
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
    assert_valid(&schemas.action, &instance, "action.schema.json", "ActionInvokeResponse::Error positivo");
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
    assert_invalid(&schemas.action, &instance, "action.schema.json", "ActionInvokeRequest sem target");
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
    assert_valid(&schemas.error, &instance, "error.schema.json", "ErrorObject positivo (com data.reason)");
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
    assert_valid(&schemas.error, &instance, "error.schema.json", "ErrorObject positivo sem data");
}

/// Caso negativo: `ErrorObject` sem `message` (campo obrigatório).
#[test]
fn error_object_missing_message_is_rejected() {
    let schemas = load_schemas();
    let instance = json!({
        "code": -32000
        // "message" ausente de propósito
    });
    assert_invalid(&schemas.error, &instance, "error.schema.json", "ErrorObject sem message");
}
