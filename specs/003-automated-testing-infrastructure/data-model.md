# Data Model: Infraestrutura de Testes Automatizada

**Feature**: `003-automated-testing-infrastructure` | **Data**: 2026-09-01

Esta feature não introduz nem muda nenhuma entidade do **protocolo de wire** (`farol-protocol`) —
os 4 schemas `v0.2` permanecem como estão (`research.md` D3 os consome, não os altera). As
"entidades" relevantes aqui são artefatos de teste/infraestrutura: fixtures, o gerador de casos de
borda, o snapshot visual e o resultado do harness. Refinado a partir de `## Key Entities` do
`spec.md` com as decisões `D1`–`D6` de `research.md`.

## 1. Fixture de execução (US1/FR-001, `research.md` D2)

Configuração determinística usada pelo harness (Camada 1 e Camada 2 de `research.md` D5) para
alcançar um cenário reprodutível sem depender de serviço externo real.

| Campo | Tipo | Descrição |
|---|---|---|
| `plugin_name` | `string` | `"git-local"` ou `"uptime-kuma"` — chave que já existe hoje em `plugin_worker::known_plugins()`, reaproveitada sem mudança. |
| `synthetic_config` | `map<string, string>` | Valores de `required_config`/config não-secreta injetados como variável de ambiente no spawn do processo filho de teste — mesmo mecanismo de produção (`config_store`/`secrets_store`), nunca lido de um segredo real. |
| `expected_terminal_state` | enum `Ready \| Unavailable(reason)` | O estado que o harness aguarda antes de declarar sucesso para este plugin neste cenário (Acceptance Scenario 1/2 de US1). |
| `fixture_backend` | enum `LocalGitRepo \| SyntheticHttpServer` | Como a fixture é materializada: um repositório git real criado em `TempDir` (git-local) ou um servidor HTTP local efêmero servindo `/metrics` sintético (uptime-kuma) — `research.md` D2. |

Não persistido em disco fora do ciclo de vida de um único run de teste (`TempDir`/porta efêmera,
descartados ao final) — nunca um arquivo versionado no repositório.

## 2. Caso de borda de contrato (US2/FR-005/FR-006, `research.md` D3)

Um valor derivado programaticamente de uma propriedade de um dos 4 schemas normativos `v0.2`, junto
com a expectativa de validade que o próprio schema implica.

| Campo | Tipo | Descrição |
|---|---|---|
| `schema_file` | enum `handshake \| widget \| action \| error` | Qual dos 4 `protocol/schema/v0.2/*.schema.json` originou o caso. |
| `json_pointer` | `string` | Caminho da propriedade dentro do schema (ex.: `#/definitions/MonitorStatusItem/properties/response_time_ms`). |
| `boundary_kind` | enum `MinimumMinusOne \| Minimum \| MaximumPlusOne \| Maximum \| NoMinimumNegative \| Null \| MissingRequired` | Qual regra de derivação gerou o valor (`research.md` D3, lista completa). |
| `value` | `serde_json::Value` | O valor de borda em si (pode ser ausência do campo, no caso `MissingRequired`). |
| `expected_schema_valid` | `bool` | Se o schema, por construção da regra de derivação, considera este valor válido — a "verdade fundamental" contra a qual (1) o `Validator` e (2) a desserialização Rust são comparados. |

Um `Caso de borda de contrato` com `expected_schema_valid = true` cuja desserialização Rust
correspondente falha é a condição de falha de FR-006 — o achado concreto de `research.md` D3
(`response_time_ms: -1` contra `Option<u32>`) é uma instância real e presente deste caso, não
hipotética.

## 3. Captura visual de referência (US4/FR-009/FR-010, `research.md` D4)

Um snapshot declarativo (texto, via `insta`) de uma tela conhecida do Farol, extraído com a
`Selector` API de `iced_test` a partir de um `Farol` construído em um estado conhecido.

| Campo | Tipo | Descrição |
|---|---|---|
| `screen_id` | enum `DashboardReady \| SetupForm \| VersionIncompatible` | Qual das três telas cobertas (`research.md` D4) — extensível (FR-013) a novas telas conforme `view.rs` ganhar novos estados renderizados. |
| `farol_state` | (construção de teste) | O `Farol`/`PluginConnection` usado para produzir a tela — mesma técnica de construção de estado já usada pelos testes existentes de `update.rs` (ex. `farol_with_widget`). |
| `snapshot_text` | `string` | A representação textual extraída via `Selector` (textos visíveis + estrutura), o que `insta` de fato compara e versiona em `crates/farol-core/tests/snapshots/`. |

Não é uma imagem/bitmap — nenhum campo de pixel nesta versão da feature (`research.md` D4, decisão
explícita e documentada conforme FR-009 exige).

## 4. Verificação automática de mudança / CI (US3/FR-007/FR-008, `research.md` D6)

Não é uma entidade de dados manipulada pelo código Rust/Python do projeto — é a configuração
declarativa do workflow GitHub Actions (`.github/workflows/ci.yml`). Modelada aqui só para
completude do vocabulário do `spec.md`:

| Campo | Tipo | Descrição |
|---|---|---|
| `job_id` | enum `rust-test \| rust-lint \| rust-smoke \| python-lint \| python-test` | Um dos 5 jobs paralelos (`research.md` D6). |
| `trigger` | enum `push \| pull_request` | FR-007 — os dois gatilhos que disparam o workflow inteiro. |
| `status` | enum `success \| failure \| in_progress` | Exposto nativamente pelo GitHub Actions como *check* da mudança (FR-008) — sem tabela/armazenamento próprio do lado do Farol. |

## Relações entre as entidades

```text
Fixture de execução ──consumida por──> Harness (research.md D5, Camada 1 e 2)
Caso de borda de contrato ──gerado a partir de──> protocol/schema/v0.2/*.schema.json
Captura visual de referência ──extraída via iced_test::Selector de──> Farol (estado real)
Verificação automática de mudança ──executa──> {testes existentes, harness, contrato, lint, visual}
```

Nenhuma dessas entidades é persistida como estado de produção do Farol (`~/.config/farol/...`) —
todas vivem dentro do ciclo de vida de `cargo test`/CI, exceto os arquivos versionados
`crates/farol-core/tests/snapshots/*.snap` (referência de comparação, análogo a qualquer teste de
regressão com fixture versionada) e `.github/workflows/ci.yml` (configuração, não dado de execução).
