# Contrato: Gerador de Casos de Borda de Contrato (US2)

**Feature**: `003-automated-testing-infrastructure` | Decisão de origem: `research.md` D3

**Local**: `crates/farol-protocol/tests/` — reaproveita `load_schemas()` de
`contract_schema_validation.rs` (ou um módulo irmão que o importa), roda sob `cargo test -p
farol-protocol`.

## Entrada

Um schema JSON já carregado (`serde_json::Value`), navegado por `json_pointer` até uma propriedade
(`data-model.md` §2). O gerador **MUST** derivar os casos a partir do conteúdo real do schema em
runtime de teste — nunca de uma constante Rust paralela que apenas *descreve* o schema (isso
reintroduziria exatamente o risco de divergência silenciosa que US2 existe para eliminar: o teste
precisa quebrar se o schema mudar e ninguém atualizar o gerador em conjunto, não continuar "verde"
porque o valor esperado foi hardcoded a parte).

## Regras de derivação (`boundary_kind`, `data-model.md` §2)

| `boundary_kind` | Pré-condição no schema | Valor gerado | `expected_schema_valid` |
|---|---|---|---|
| `MinimumMinusOne` | propriedade declara `minimum` | `minimum - 1` | `false` |
| `Minimum` | propriedade declara `minimum` | `minimum` | `true` |
| `MaximumPlusOne` | propriedade declara `maximum` | `maximum + 1` | `false` |
| `Maximum` | propriedade declara `maximum` | `maximum` | `true` |
| `NoMinimumNegative` | propriedade NÃO declara `minimum`, `type` inclui `"integer"`/`"number"` | `-1` | `true` |
| `Null` | `type` inclui `"null"` | `null` | `true` |
| `MissingRequired` | propriedade está em `required` do objeto pai | (campo omitido da instância) | `false` |

Uma propriedade pode gerar múltiplos casos (ex. `response_time_ms`: `Null` + `NoMinimumNegative` +
`MissingRequired`, já que é obrigatório e nullable ao mesmo tempo — `protocol/schema/v0.2/
widget.schema.json`).

## Execução

Para cada caso gerado, o gerador:
1. Constrói uma instância completa e válida do schema-alvo (reaproveitando um construtor de mensagem
   já existente em `contract_schema_validation.rs`), com a propriedade sob teste substituída (ou
   omitida, no caso `MissingRequired`) pelo `value` do caso.
2. Valida essa instância contra o `Validator` do schema (`jsonschema` crate, registry já montado por
   `load_schemas()`); confirma que o resultado bate com `expected_schema_valid` — uma discrepância
   aqui é um bug no **gerador** (a regra de derivação não corresponde ao schema real), não no core, e
   deve falhar de forma diferenciada de (3).
3. **Somente quando `expected_schema_valid == true`**: tenta desserializar a mesma instância no tipo
   Rust correspondente (`serde_json::from_value::<T>`). **MUST** suceder. Falhar aqui é a condição de
   FR-006 — reportado com `schema_file`, `json_pointer`, `boundary_kind` e o valor exato, sem exigir
   que quem investiga reproduza manualmente.

## Saída esperada (falha, formato mínimo da mensagem de asserção)

```text
FR-006: valor de borda permitido pelo schema widget.schema.json
  (#/definitions/MonitorStatusItem/properties/response_time_ms, boundary_kind=NoMinimumNegative,
  value=-1) foi REJEITADO pela desserialização Rust (MonitorStatusItem::response_time_ms: Option<u32>).
```

## Caso conhecido no momento desta sessão de planejamento

`research.md` D3 documenta que o caso `widget.schema.json` /
`MonitorStatusItem.response_time_ms` / `NoMinimumNegative` (`value = -1`) falha **hoje**, contra o
tipo `Option<u32>` de `crates/farol-protocol/src/messages.rs` — este caso específico MUST fazer parte
do conjunto gerado desde a primeira execução da suíte (não é opcional nem um exemplo ilustrativo),
servindo simultaneamente de prova de que o mecanismo funciona (SC-002) e de constatação de um
gap real de contrato já existente, cuja correção fica fora do escopo desta feature (`## Out of
Scope` de `spec.md`).

## Escopo de versão (Edge Case do `spec.md`)

O gerador roda **apenas** contra `protocol/schema/v0.2/*.schema.json` — a versão corrente com
implementação ativa (`## Assumptions` de `spec.md`). `protocol/schema/v0.1/` permanece fora do
escopo deste gerador; qualquer teste que já valide `v0.1` (nenhum hoje, além do histórico substituído
por H3 da feature 002) não ganha a mesma cobertura de borda por esta feature.
