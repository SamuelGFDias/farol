//! Gerador de casos de borda de contrato (US2, T011-T016) — `contracts/contract-boundary-
//! testing.md`.
//!
//! Diferente de `contract_schema_validation.rs` (exemplos manuais, escritos à mão um a um), este
//! arquivo deriva casos de borda (`MinimumMinusOne`/`Minimum`/`MaximumPlusOne`/`Maximum`/
//! `NoMinimumNegative`/`Null`/`MissingRequired`) diretamente do CONTEÚDO REAL dos JSON Schemas
//! `protocol/schema/v0.2/*.schema.json`, em tempo de execução do teste — nunca de uma constante
//! Rust paralela que apenas descreve o schema. Se o schema mudar (um `minimum` for editado, um
//! campo deixar de ser `required`...), os valores gerados aqui mudam junto, sem precisar tocar
//! este arquivo — é essa propriedade que faz o teste realmente quebrar quando schema e binding
//! Rust divergem, em vez de continuar "verde" com um valor esperado hardcoded à parte
//! (`contracts/contract-boundary-testing.md` § Entrada).
//!
//! ## Por que este arquivo não importa `load_schemas()` de `contract_schema_validation.rs`
//!
//! Cada arquivo em `crates/<crate>/tests/*.rs` é compilado pelo Cargo como um binário de teste de
//! integração independente — não há como um importar itens `fn`/`struct` privados de outro sem um
//! módulo compartilhado (`tests/common/mod.rs`), fora do escopo de arquivos autorizados desta
//! task. `load_schema_set()` abaixo reimplementa o mesmo carregamento dos 4 schemas + registry
//! compartilhado (necessário pelos `$ref` cruzados entre eles, ex. `widget.schema.json` referencia
//! `ActionDeclaration` de `handshake.schema.json`) já descrito em `contract_schema_validation.rs`.
//!
//! ## T011 — a função geradora
//!
//! `numeric_and_null_boundary_cases()` e `missing_required_case()`, abaixo, são o gerador exigido
//! por T011: dado o `serde_json::Value` de UMA propriedade (ou do objeto pai, para
//! `MissingRequired`), devolvem os casos aplicáveis segundo a tabela de
//! `contracts/contract-boundary-testing.md`. T012-T015 só aplicam esse gerador a propriedades
//! específicas dos 4 schemas — nenhuma delas reimplementa a lógica de derivação.
//!
//! ### Extensão documentada: `exclusiveMinimum`/`exclusiveMaximum`
//!
//! A tabela do contrato só define `MinimumMinusOne`/`Minimum`/`MaximumPlusOne`/`Maximum` em
//! termos de `minimum`/`maximum` (inclusivos). Duas propriedades reais do protocolo
//! (`WidgetDeclaration.suggested_refresh_interval_ms`, `ActionDeclaration.timeout_hint_ms`) usam
//! `exclusiveMinimum` em vez de `minimum`. Em vez de inventar um `boundary_kind` fora do
//! vocabulário do contrato, o gerador reaproveita os mesmos rótulos `MinimumMinusOne`/`Minimum`
//! apontando, respectivamente, para o valor exclusivo em si (ainda inválido) e seu vizinho
//! imediato (o menor valor válido) — mesma semântica de "um abaixo do limite / o limite", só que
//! medida a partir do lado exclusivo. Ver `numeric_and_null_boundary_cases()`.

use jsonschema::{Registry, Validator};
use serde_json::{json, Value};

use farol_protocol::messages::{Capability, KnownCapability, MonitorStatusItem};
use farol_protocol::{ActionDeclaration, ErrorObject, RemoteStatus, WidgetDeclaration};

const HANDSHAKE_ID: &str = "https://farol.dev/protocol/v0.2/handshake.schema.json";
const WIDGET_ID: &str = "https://farol.dev/protocol/v0.2/widget.schema.json";
const ACTION_ID: &str = "https://farol.dev/protocol/v0.2/action.schema.json";
const ERROR_ID: &str = "https://farol.dev/protocol/v0.2/error.schema.json";

const HANDSHAKE_SCHEMA: &str = include_str!("../../../protocol/schema/v0.2/handshake.schema.json");
const WIDGET_SCHEMA: &str = include_str!("../../../protocol/schema/v0.2/widget.schema.json");
const ACTION_SCHEMA: &str = include_str!("../../../protocol/schema/v0.2/action.schema.json");
const ERROR_SCHEMA: &str = include_str!("../../../protocol/schema/v0.2/error.schema.json");

// -------------------------------------------------------------------------------------------
// Carregamento dos schemas (equivalente a `load_schemas()` de `contract_schema_validation.rs`,
// ver nota do módulo acima sobre por que não é reaproveitado por import direto).
// -------------------------------------------------------------------------------------------

struct SchemaSet<'a> {
    handshake: Value,
    widget: Value,
    action: Value,
    error: Value,
    registry: Registry<'a>,
}

fn load_schema_set<'a>() -> SchemaSet<'a> {
    let handshake: Value =
        serde_json::from_str(HANDSHAKE_SCHEMA).expect("handshake.schema.json inválido");
    let widget: Value = serde_json::from_str(WIDGET_SCHEMA).expect("widget.schema.json inválido");
    let action: Value = serde_json::from_str(ACTION_SCHEMA).expect("action.schema.json inválido");
    let error: Value = serde_json::from_str(ERROR_SCHEMA).expect("error.schema.json inválido");

    let registry = Registry::new()
        .add(
            "https://farol.dev/protocol/v0.2/handshake.schema.json",
            handshake.clone(),
        )
        .expect("URI de handshake.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.2/widget.schema.json",
            widget.clone(),
        )
        .expect("URI de widget.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.2/action.schema.json",
            action.clone(),
        )
        .expect("URI de action.schema.json inválida")
        .add(
            "https://farol.dev/protocol/v0.2/error.schema.json",
            error.clone(),
        )
        .expect("URI de error.schema.json inválida")
        .prepare()
        .expect("registry deveria preparar com sucesso (schemas consistentes entre si)");

    SchemaSet {
        handshake,
        widget,
        action,
        error,
        registry,
    }
}

impl<'a> SchemaSet<'a> {
    /// Compila um `Validator` para uma entrada nomeada de `$defs` de um dos 4 arquivos,
    /// compartilhando o registry deste conjunto (necessário para `$ref` cruzados entre arquivos).
    /// Compila um `{"$ref": "<$id do arquivo>#/$defs/<def_name>"}` em vez do subschema extraído
    /// isoladamente: um `$def` como `ActionDeclaration` contém, por sua vez, `$ref`s relativos
    /// (`#/$defs/ActionTarget`) que só resolvem tendo o documento completo do arquivo como base -
    /// extrair só o fragmento perderia essa base e o `$ref` relativo apontaria para lugar nenhum.
    fn def_validator(&self, file_id: &str, def_name: &str) -> Validator {
        let wrapper = json!({ "$ref": format!("{file_id}#/$defs/{def_name}") });
        jsonschema::options()
            .with_registry(&self.registry)
            .build(&wrapper)
            .unwrap_or_else(|e| panic!("subschema $defs/{def_name} deveria compilar: {e}"))
    }

    /// Navega um JSON Pointer (RFC 6901, sem o `#` inicial) dentro do `Value` bruto de um schema.
    /// É esta navegação, feita em tempo de execução do teste sobre o schema real, que garante que
    /// todo valor de borda gerado a partir daqui reflete o schema tal como ele está agora — nunca
    /// uma cópia estática do que ele dizia quando o teste foi escrito.
    fn at<'v>(&self, file: &'v Value, pointer: &str) -> &'v Value {
        file.pointer(pointer)
            .unwrap_or_else(|| panic!("json pointer '{pointer}' não encontrado no schema"))
    }
}

// -------------------------------------------------------------------------------------------
// T011 — o gerador
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryKind {
    MinimumMinusOne,
    Minimum,
    MaximumPlusOne,
    Maximum,
    NoMinimumNegative,
    Null,
    MissingRequired,
}

struct BoundaryCase {
    kind: BoundaryKind,
    /// `None` = campo omitido da instância (caso `MissingRequired`); `Some` = valor a substituir.
    value: Option<Value>,
    expected_schema_valid: bool,
}

fn type_includes(schema: &Value, needle: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(s)) => s == needle,
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(needle)),
        _ => false,
    }
}

/// Deriva os casos `MinimumMinusOne`/`Minimum`/`MaximumPlusOne`/`Maximum`/`NoMinimumNegative`/
/// `Null` de UMA propriedade, a partir do `serde_json::Value` real dessa propriedade
/// (`contracts/contract-boundary-testing.md` § Regras de derivação). Ver doc do módulo para a
/// extensão de `exclusiveMinimum`/`exclusiveMaximum`.
fn numeric_and_null_boundary_cases(property_schema: &Value) -> Vec<BoundaryCase> {
    let mut cases = Vec::new();

    let minimum = property_schema.get("minimum").and_then(Value::as_i64);
    let maximum = property_schema.get("maximum").and_then(Value::as_i64);
    let exclusive_minimum = property_schema
        .get("exclusiveMinimum")
        .and_then(Value::as_i64);
    let exclusive_maximum = property_schema
        .get("exclusiveMaximum")
        .and_then(Value::as_i64);

    match (minimum, exclusive_minimum) {
        (Some(min), _) => {
            cases.push(BoundaryCase {
                kind: BoundaryKind::MinimumMinusOne,
                value: Some(json!(min - 1)),
                expected_schema_valid: false,
            });
            cases.push(BoundaryCase {
                kind: BoundaryKind::Minimum,
                value: Some(json!(min)),
                expected_schema_valid: true,
            });
        }
        (None, Some(ex_min)) => {
            cases.push(BoundaryCase {
                kind: BoundaryKind::MinimumMinusOne,
                value: Some(json!(ex_min)),
                expected_schema_valid: false,
            });
            cases.push(BoundaryCase {
                kind: BoundaryKind::Minimum,
                value: Some(json!(ex_min + 1)),
                expected_schema_valid: true,
            });
        }
        (None, None) => {}
    }

    match (maximum, exclusive_maximum) {
        (Some(max), _) => {
            cases.push(BoundaryCase {
                kind: BoundaryKind::MaximumPlusOne,
                value: Some(json!(max + 1)),
                expected_schema_valid: false,
            });
            cases.push(BoundaryCase {
                kind: BoundaryKind::Maximum,
                value: Some(json!(max)),
                expected_schema_valid: true,
            });
        }
        (None, Some(ex_max)) => {
            cases.push(BoundaryCase {
                kind: BoundaryKind::MaximumPlusOne,
                value: Some(json!(ex_max)),
                expected_schema_valid: false,
            });
            cases.push(BoundaryCase {
                kind: BoundaryKind::Maximum,
                value: Some(json!(ex_max - 1)),
                expected_schema_valid: true,
            });
        }
        (None, None) => {}
    }

    let type_is_numeric =
        type_includes(property_schema, "integer") || type_includes(property_schema, "number");
    if minimum.is_none() && exclusive_minimum.is_none() && type_is_numeric {
        cases.push(BoundaryCase {
            kind: BoundaryKind::NoMinimumNegative,
            value: Some(json!(-1)),
            expected_schema_valid: true,
        });
    }

    if type_includes(property_schema, "null") {
        cases.push(BoundaryCase {
            kind: BoundaryKind::Null,
            value: Some(Value::Null),
            expected_schema_valid: true,
        });
    }

    cases
}

/// Deriva o caso `MissingRequired` de uma propriedade, checando se ela está no array `required`
/// do schema do objeto PAI real (não assumido por convenção).
fn missing_required_case(parent_schema: &Value, prop_name: &str) -> Option<BoundaryCase> {
    let required = parent_schema.get("required")?.as_array()?;
    if required.iter().any(|v| v.as_str() == Some(prop_name)) {
        Some(BoundaryCase {
            kind: BoundaryKind::MissingRequired,
            value: None,
            expected_schema_valid: false,
        })
    } else {
        None
    }
}

/// Aplica um caso a uma instância base (objeto JSON), substituindo ou removendo o campo alvo.
fn apply_case(base: &Value, field: &str, case: &BoundaryCase) -> Value {
    let mut instance = base.clone();
    let obj = instance
        .as_object_mut()
        .expect("instância base do caso de borda deve ser um objeto JSON");
    match &case.value {
        Some(v) => {
            obj.insert(field.to_string(), v.clone());
        }
        None => {
            obj.remove(field);
        }
    }
    instance
}

/// Passo (1) da execução (`contracts/contract-boundary-testing.md`): confirma que o resultado da
/// validação bate com `expected_schema_valid`. Uma discrepância aqui é um bug no GERADOR (a regra
/// de derivação não corresponde ao schema real), não no core.
fn assert_case_matches_schema(
    validator: &Validator,
    schema_file: &str,
    json_pointer: &str,
    case: &BoundaryCase,
    instance: &Value,
) {
    let actual_valid = validator.is_valid(instance);
    assert_eq!(
        actual_valid, case.expected_schema_valid,
        "gerador diverge do schema real em {schema_file} ({json_pointer}, boundary_kind={:?}): esperava valid={}, schema retornou {}. instância: {}",
        case.kind,
        case.expected_schema_valid,
        actual_valid,
        serde_json::to_string_pretty(instance).unwrap(),
    );
}

/// Passo (3) da execução, só chamado quando `expected_schema_valid == true`: desserializa a
/// mesma instância no tipo Rust correspondente. Falhar aqui é a condição de FR-006 — mensagem no
/// formato mínimo exigido por `contracts/contract-boundary-testing.md` § Saída esperada.
fn assert_case_deserializes<T: serde::de::DeserializeOwned>(
    schema_file: &str,
    json_pointer: &str,
    case: &BoundaryCase,
    instance: &Value,
    rust_type_field: &str,
) {
    let result: Result<T, _> = serde_json::from_value(instance.clone());
    assert!(
        result.is_ok(),
        "FR-006: valor de borda permitido pelo schema {schema_file}\n  ({json_pointer}, boundary_kind={:?}, value={:?}) foi REJEITADO pela desserialização Rust ({rust_type_field}).\n  erro: {}",
        case.kind,
        case.value,
        result.err().map(|e| e.to_string()).unwrap_or_default(),
    );
}

// -------------------------------------------------------------------------------------------
// T011 — testes do gerador em si, com schemas sintéticos: provam que os valores vêm do schema em
// tempo de execução, não de uma constante Rust paralela.
// -------------------------------------------------------------------------------------------

#[test]
fn generator_derives_boundary_values_from_the_schemas_own_declared_minimum_and_maximum() {
    let schema = json!({"type": "integer", "minimum": 10, "maximum": 20});
    let cases = numeric_and_null_boundary_cases(&schema);
    assert_eq!(
        cases
            .iter()
            .find(|c| c.kind == BoundaryKind::MinimumMinusOne)
            .unwrap()
            .value,
        Some(json!(9))
    );
    assert_eq!(
        cases
            .iter()
            .find(|c| c.kind == BoundaryKind::Minimum)
            .unwrap()
            .value,
        Some(json!(10))
    );
    assert_eq!(
        cases
            .iter()
            .find(|c| c.kind == BoundaryKind::MaximumPlusOne)
            .unwrap()
            .value,
        Some(json!(21))
    );
    assert_eq!(
        cases
            .iter()
            .find(|c| c.kind == BoundaryKind::Maximum)
            .unwrap()
            .value,
        Some(json!(20))
    );

    // Muda o schema (minimum:100) e o mesmo gerador MUST refletir o novo valor — prova de que não
    // há uma constante Rust paralela hardcoded em algum lugar.
    let changed = json!({"type": "integer", "minimum": 100, "maximum": 200});
    let changed_cases = numeric_and_null_boundary_cases(&changed);
    assert_eq!(
        changed_cases
            .iter()
            .find(|c| c.kind == BoundaryKind::Minimum)
            .unwrap()
            .value,
        Some(json!(100))
    );
    assert_eq!(
        changed_cases
            .iter()
            .find(|c| c.kind == BoundaryKind::MinimumMinusOne)
            .unwrap()
            .value,
        Some(json!(99))
    );
}

#[test]
fn generator_no_minimum_negative_only_when_schema_truly_has_no_lower_bound() {
    let unbounded = json!({"type": ["integer", "null"]});
    let cases = numeric_and_null_boundary_cases(&unbounded);
    assert!(cases
        .iter()
        .any(|c| c.kind == BoundaryKind::NoMinimumNegative && c.value == Some(json!(-1))));
    assert!(cases
        .iter()
        .any(|c| c.kind == BoundaryKind::Null && c.value == Some(Value::Null)));

    let bounded = json!({"type": "integer", "minimum": 0});
    let bounded_cases = numeric_and_null_boundary_cases(&bounded);
    assert!(
        !bounded_cases
            .iter()
            .any(|c| c.kind == BoundaryKind::NoMinimumNegative),
        "NoMinimumNegative não deve ser gerado quando o schema já declara minimum"
    );
}

#[test]
fn generator_missing_required_only_when_property_is_in_the_parent_required_array() {
    let parent = json!({"type": "object", "required": ["a", "b"]});
    assert!(missing_required_case(&parent, "a").is_some());
    assert!(missing_required_case(&parent, "z").is_none());

    let no_required = json!({"type": "object"});
    assert!(missing_required_case(&no_required, "a").is_none());
}

// -------------------------------------------------------------------------------------------
// T012 — handshake.schema.json
// -------------------------------------------------------------------------------------------

/// `Capability` (`kind: "network"`).`port` — `minimum: 1`, `maximum: 65535`, dentro do
/// `allOf[1].then` condicional para `kind == "network"`.
#[test]
fn handshake_capability_network_port_min_max_boundaries() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(HANDSHAKE_ID, "Capability");
    let pointer = "/$defs/Capability/allOf/1/then/properties/port";
    let property_schema = schemas.at(&schemas.handshake, pointer);
    let cases = numeric_and_null_boundary_cases(property_schema);
    assert!(
        !cases.is_empty(),
        "esperava ao menos um caso de borda para Capability(network).port (minimum/maximum declarados)"
    );

    let base = json!({"kind": "network", "host": "kuma.example.com", "port": 443});
    for case in &cases {
        let instance = apply_case(&base, "port", case);
        assert_case_matches_schema(
            &validator,
            "handshake.schema.json",
            pointer,
            case,
            &instance,
        );
        if case.expected_schema_valid {
            assert_case_deserializes::<Capability>(
                "handshake.schema.json",
                pointer,
                case,
                &instance,
                "Capability::Known(KnownCapability::Network { port: Option<u16>, .. })",
            );
            let parsed: Capability = serde_json::from_value(instance).unwrap();
            assert!(
                matches!(parsed, Capability::Known(KnownCapability::Network { .. })),
                "instância válida do schema desserializou como Capability::Unknown em vez de \
                 Known::Network (boundary_kind={:?}) — o `#[serde(untagged)]` de Capability tem um \
                 fallback silencioso para `kind` conhecido com payload malformado, ver messages.rs",
                case.kind,
            );
        }
    }
}

/// `WidgetDeclaration.suggested_refresh_interval_ms` — `exclusiveMinimum: 0`, campo opcional (sem
/// `MissingRequired` aplicável).
#[test]
fn handshake_widget_declaration_suggested_refresh_interval_ms_boundaries() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(HANDSHAKE_ID, "WidgetDeclaration");
    let pointer = "/$defs/WidgetDeclaration/properties/suggested_refresh_interval_ms";
    let property_schema = schemas.at(&schemas.handshake, pointer);
    let cases = numeric_and_null_boundary_cases(property_schema);
    assert!(
        !cases.is_empty(),
        "esperava ao menos um caso de borda para suggested_refresh_interval_ms (exclusiveMinimum declarado)"
    );

    let base = json!({
        "id": "uptime-kuma-monitors",
        "kind": "monitor-status-grid",
        "title": "Uptime Kuma",
        "suggested_refresh_interval_ms": 30000
    });
    for case in &cases {
        let instance = apply_case(&base, "suggested_refresh_interval_ms", case);
        assert_case_matches_schema(
            &validator,
            "handshake.schema.json",
            pointer,
            case,
            &instance,
        );
        if case.expected_schema_valid {
            assert_case_deserializes::<WidgetDeclaration>(
                "handshake.schema.json",
                pointer,
                case,
                &instance,
                "WidgetDeclaration::suggested_refresh_interval_ms: Option<u64>",
            );
        }
    }
}

/// `RequiredConfigItem.secret` (boolean) — o gerador numérico/nulo não produz nenhum caso (nem
/// `minimum`/`maximum`, nem `type` numérico/nulo), o que é o comportamento CORRETO e é verificado
/// aqui explicitamente em vez de só documentado em comentário. O que de fato se aplica a este
/// campo é `MissingRequired` (está em `required` de `RequiredConfigItem`).
#[test]
fn handshake_required_config_item_secret_has_no_numeric_case_but_is_required() {
    let schemas = load_schema_set();
    let pointer = "/$defs/RequiredConfigItem/properties/secret";
    let property_schema = schemas.at(&schemas.handshake, pointer);
    assert!(
        numeric_and_null_boundary_cases(property_schema).is_empty(),
        "RequiredConfigItem.secret é boolean — nenhum boundary_kind numérico/nulo do vocabulário se aplica; \
         se isto falhar, o schema mudou o tipo do campo e este teste precisa ser revisitado"
    );

    let parent = schemas.at(&schemas.handshake, "/$defs/RequiredConfigItem");
    let case = missing_required_case(parent, "secret")
        .expect("RequiredConfigItem.secret deveria estar em `required`");
    let validator = schemas.def_validator(HANDSHAKE_ID, "RequiredConfigItem");
    let base = json!({"name": "api_key", "secret": true, "description": "API Key"});
    let instance = apply_case(&base, "secret", &case);
    assert_case_matches_schema(
        &validator,
        "handshake.schema.json",
        "/$defs/RequiredConfigItem",
        &case,
        &instance,
    );
}

// -------------------------------------------------------------------------------------------
// T013 — widget.schema.json
// -------------------------------------------------------------------------------------------

/// `MonitorStatusItem.response_time_ms` (`type: ["integer","null"]`, `required`, sem `minimum`) —
/// os dois casos que hoje se comportam corretamente contra `Option<u32>`: `Null` (valor `null`) e
/// `MissingRequired` (campo obrigatório de fato ausente é rejeitado pelo schema). O terceiro caso
/// gerado para esta propriedade, `NoMinimumNegative` (`-1`), é o gap conhecido — ver o teste
/// `#[ignore]`d logo abaixo (T016, `research.md` D3).
#[test]
fn widget_monitor_status_item_response_time_ms_null_and_missing_required() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(WIDGET_ID, "MonitorStatusItem");
    let pointer = "/$defs/MonitorStatusItem/properties/response_time_ms";
    let property_schema = schemas.at(&schemas.widget, pointer);
    let cases = numeric_and_null_boundary_cases(property_schema);
    assert!(
        cases.iter().any(|c| c.kind == BoundaryKind::Null),
        "esperava um caso Null para response_time_ms (type inclui null)"
    );

    let base = json!({"name": "api_example_com", "status": "up", "response_time_ms": 42});

    let null_case = cases.iter().find(|c| c.kind == BoundaryKind::Null).unwrap();
    let null_instance = apply_case(&base, "response_time_ms", null_case);
    assert_case_matches_schema(
        &validator,
        "widget.schema.json",
        pointer,
        null_case,
        &null_instance,
    );
    assert_case_deserializes::<MonitorStatusItem>(
        "widget.schema.json",
        pointer,
        null_case,
        &null_instance,
        "MonitorStatusItem::response_time_ms: Option<u32>",
    );

    let parent = schemas.at(&schemas.widget, "/$defs/MonitorStatusItem");
    let missing_case = missing_required_case(parent, "response_time_ms")
        .expect("response_time_ms deveria estar em `required` de MonitorStatusItem");
    let missing_instance = apply_case(&base, "response_time_ms", &missing_case);
    assert_case_matches_schema(
        &validator,
        "widget.schema.json",
        pointer,
        &missing_case,
        &missing_instance,
    );
}

/// T016 / Cenário 2 de `quickstart.md`: `MonitorStatusItem.response_time_ms` com
/// `boundary_kind=NoMinimumNegative` (`value=-1`) é permitido por `widget.schema.json` (nenhum
/// `minimum` declarado) mas `Option<u32>` (`crates/farol-protocol/src/messages.rs`) não consegue
/// representar valor negativo algum — gap de contrato real, descoberto originalmente porque o
/// Uptime Kuma real emitiu `-1`. Este teste roda em `cargo test -- --ignored` e prova o mecanismo:
/// (1) a asserção de schema abaixo passa (o gerador concorda com o schema real: `-1` É válido) e
/// (2) a asserção de desserialização Rust FALHA (o binding rejeita), no formato de mensagem FR-006
/// exigido por `contracts/contract-boundary-testing.md`.
#[test]
#[ignore = "débito rastreado (issue #5, github.com/SamuelGFDias/farol/issues/5): \
            MonitorStatusItem.response_time_ms é Option<u32> (crates/farol-protocol/src/messages.rs) \
            mas widget.schema.json permite qualquer inteiro, incluindo negativo (sem `minimum`) - \
            ver research.md D3. Remover este #[ignore] só depois que a issue #5 for resolvida."]
fn widget_monitor_status_item_response_time_ms_negative_value_is_a_known_protocol_gap() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(WIDGET_ID, "MonitorStatusItem");
    let pointer = "/$defs/MonitorStatusItem/properties/response_time_ms";
    let property_schema = schemas.at(&schemas.widget, pointer);
    let cases = numeric_and_null_boundary_cases(property_schema);
    let case = cases
        .iter()
        .find(|c| c.kind == BoundaryKind::NoMinimumNegative)
        .expect("gerador deveria produzir NoMinimumNegative para response_time_ms (sem minimum, type integer|null)");
    assert_eq!(case.value, Some(json!(-1)));

    let base = json!({"name": "api_example_com", "status": "up", "response_time_ms": 42});
    let instance = apply_case(&base, "response_time_ms", case);

    // (1) o gerador concorda com o schema real: -1 é válido (sem `minimum` declarado).
    assert_case_matches_schema(&validator, "widget.schema.json", pointer, case, &instance);

    // (2) FR-006: a mesma instância, válida pelo schema, é rejeitada pela desserialização Rust.
    assert_case_deserializes::<MonitorStatusItem>(
        "widget.schema.json",
        pointer,
        case,
        &instance,
        "MonitorStatusItem::response_time_ms: Option<u32>",
    );
}

/// `RemoteStatus::Tracked { ahead, behind }` — ambos `minimum: 0`. Cobertura adicional dentro de
/// `widget.schema.json` que passa hoje (ao contrário de `response_time_ms`), demonstrando que o
/// gerador funciona corretamente na maioria dos casos, não só no caso conhecido como falho.
#[test]
fn widget_remote_status_tracked_ahead_and_behind_minimum_boundaries() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(WIDGET_ID, "RemoteStatus");

    for field in ["ahead", "behind"] {
        let pointer = format!("/$defs/RemoteStatus/oneOf/0/properties/{field}");
        let property_schema = schemas.at(&schemas.widget, &pointer);
        let cases = numeric_and_null_boundary_cases(property_schema);
        assert!(
            !cases.is_empty(),
            "esperava ao menos um caso de borda para RemoteStatus.{field} (minimum:0 declarado)"
        );

        let base = json!({"kind": "tracked", "ahead": 1, "behind": 0});
        for case in &cases {
            let instance = apply_case(&base, field, case);
            assert_case_matches_schema(&validator, "widget.schema.json", &pointer, case, &instance);
            if case.expected_schema_valid {
                assert_case_deserializes::<RemoteStatus>(
                    "widget.schema.json",
                    &pointer,
                    case,
                    &instance,
                    "RemoteStatus::Tracked { ahead: u64, behind: u64 }",
                );
            }
        }
    }
}

// -------------------------------------------------------------------------------------------
// T014 — action.schema.json
// -------------------------------------------------------------------------------------------
//
// `timeout_hint_ms` é fisicamente declarado em `$defs/ActionDeclaration` de
// `handshake.schema.json` (não em `action.schema.json` - `action.schema.json` só define os
// envelopes de request/response do método `action/invoke` e referencia `ActionTarget` de
// `handshake.schema.json`; `ActionDeclaration` em si é usado por `widget.schema.json` para
// compor `WidgetItem.fetch_action`). Ele é testado aqui, na seção conceitual "action", porque é
// isso que a task T014 nomeia (`timeout_hint_ms`) - `contracts/contract-boundary-testing.md` não
// exige que o arquivo físico do schema corresponda ao arquivo de teste, só que o valor seja
// derivado do schema real, o que este teste faz.

/// `ActionDeclaration.timeout_hint_ms` — `exclusiveMinimum: 0`, campo opcional, mesma forma de
/// `suggested_refresh_interval_ms` (T012).
#[test]
fn action_declaration_timeout_hint_ms_boundaries() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(HANDSHAKE_ID, "ActionDeclaration");
    let pointer = "/$defs/ActionDeclaration/properties/timeout_hint_ms";
    let property_schema = schemas.at(&schemas.handshake, pointer);
    let cases = numeric_and_null_boundary_cases(property_schema);
    assert!(
        !cases.is_empty(),
        "esperava ao menos um caso de borda para timeout_hint_ms (exclusiveMinimum declarado)"
    );

    let base = json!({
        "id": "git.fetch",
        "label": "Fetch",
        "target": {"type": "repo", "id": "/home/dev/projetos/farol"},
        "enabled": true,
        "timeout_hint_ms": 120000
    });
    for case in &cases {
        let instance = apply_case(&base, "timeout_hint_ms", case);
        assert_case_matches_schema(
            &validator,
            "handshake.schema.json",
            pointer,
            case,
            &instance,
        );
        if case.expected_schema_valid {
            assert_case_deserializes::<ActionDeclaration>(
                "handshake.schema.json",
                pointer,
                case,
                &instance,
                "ActionDeclaration::timeout_hint_ms: Option<u64>",
            );
        }
    }
}

/// `ActionInvokeParams.target` (`action.schema.json`, este sim fisicamente no arquivo) —
/// `MissingRequired`, cobertura de um campo não-numérico com o mesmo gerador (a tabela de
/// `contracts/contract-boundary-testing.md` não restringe `MissingRequired` a propriedades
/// numéricas).
#[test]
fn action_invoke_params_target_missing_required_is_rejected() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(ACTION_ID, "ActionInvokeParams");
    let parent = schemas.at(&schemas.action, "/$defs/ActionInvokeParams");
    let case = missing_required_case(parent, "target")
        .expect("ActionInvokeParams.target deveria estar em `required`");

    let base = json!({
        "action_id": "git.fetch",
        "target": {"type": "repo", "id": "/home/dev/projetos/farol"}
    });
    let instance = apply_case(&base, "target", &case);
    assert_case_matches_schema(
        &validator,
        "action.schema.json",
        "/$defs/ActionInvokeParams",
        &case,
        &instance,
    );
}

// -------------------------------------------------------------------------------------------
// T015 — error.schema.json
// -------------------------------------------------------------------------------------------

/// `ErrorObject.code` (`type: integer`, sem `minimum`/`maximum`, `required`) — `NoMinimumNegative`
/// e `MissingRequired`; `ErrorObject.message` (`required`, string) — `MissingRequired`.
#[test]
fn error_object_code_and_message_boundaries() {
    let schemas = load_schema_set();
    let validator = schemas.def_validator(ERROR_ID, "ErrorObject");
    let parent = schemas.at(&schemas.error, "/$defs/ErrorObject");
    let base = json!({"code": -32000, "message": "versao incompativel"});

    let code_pointer = "/$defs/ErrorObject/properties/code";
    let code_schema = schemas.at(&schemas.error, code_pointer);
    let code_cases = numeric_and_null_boundary_cases(code_schema);
    let no_min_negative = code_cases
        .iter()
        .find(|c| c.kind == BoundaryKind::NoMinimumNegative)
        .expect("gerador deveria produzir NoMinimumNegative para ErrorObject.code (sem minimum, type integer)");
    let instance = apply_case(&base, "code", no_min_negative);
    assert_case_matches_schema(
        &validator,
        "error.schema.json",
        code_pointer,
        no_min_negative,
        &instance,
    );
    assert_case_deserializes::<ErrorObject>(
        "error.schema.json",
        code_pointer,
        no_min_negative,
        &instance,
        "ErrorObject::code: i64",
    );

    let code_missing = missing_required_case(parent, "code")
        .expect("ErrorObject.code deveria estar em `required`");
    let code_missing_instance = apply_case(&base, "code", &code_missing);
    assert_case_matches_schema(
        &validator,
        "error.schema.json",
        "/$defs/ErrorObject",
        &code_missing,
        &code_missing_instance,
    );

    let message_missing = missing_required_case(parent, "message")
        .expect("ErrorObject.message deveria estar em `required`");
    let message_missing_instance = apply_case(&base, "message", &message_missing);
    assert_case_matches_schema(
        &validator,
        "error.schema.json",
        "/$defs/ErrorObject",
        &message_missing,
        &message_missing_instance,
    );
}

/// `ErrorData.reason` (dentro de `ErrorObject.data`) — string livre, não-`required`, sem
/// `minimum`/`maximum`. Confirmação executável (não só comentário) de que nenhum caso de borda
/// numérico/nulo do gerador se aplica a este campo, per T015: "se nenhum existir, documentar essa
/// constatação em comentário em vez de forçar um caso artificial".
#[test]
fn error_data_reason_has_no_applicable_numeric_or_null_boundary_case() {
    let schemas = load_schema_set();
    let reason_schema = schemas.at(
        &schemas.error,
        "/$defs/ErrorObject/properties/data/properties/reason",
    );
    assert!(
        numeric_and_null_boundary_cases(reason_schema).is_empty(),
        "T015: data.reason é string livre — nenhum boundary_kind numérico/nulo esperado; se isto \
         falhar, o schema mudou o tipo do campo e T015 precisa ser revisitada"
    );

    let data_schema = schemas.at(&schemas.error, "/$defs/ErrorObject/properties/data");
    assert!(
        data_schema.get("required").is_none(),
        "T015: `data.reason` não é `required` pelo JSON Schema (só pela prosa normativa quando \
         `code` é um código de domínio Farol) — se isto falhar, `required` foi adicionado e um \
         caso MissingRequired passa a se aplicar"
    );
}
