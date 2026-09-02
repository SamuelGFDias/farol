# Quickstart: Validação da Infraestrutura de Testes Automatizada

**Feature**: `003-automated-testing-infrastructure` | **Data**: 2026-09-01

Guia para validar manualmente, ponta a ponta, as quatro User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de `specs/001-*`/`specs/002-*`.

## Pré-requisitos

- Rust estável ≥ 1.75, `cargo`, `iced` já em `0.14` (`research.md` D1 — pré-requisito da própria
  feature, não algo que o usuário precisa instalar à parte).
- Python 3.11+ no `PATH` (plugins de referência).
- `xvfb` instalado (`sudo apt-get install -y xvfb` ou equivalente da distro) — só necessário para o
  Cenário 1 (Camada 2 do harness, `contracts/e2e-harness-contract.md`); os demais cenários não
  precisam de display.
- `cargo insta` instalado (`cargo install cargo-insta`) — só necessário para revisar/aceitar um novo
  snapshot visual localmente (Cenário 4); não é pré-requisito para rodar a suíte.

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

## Cenário 1 — Harness de execução real detecta sucesso e falha (User Story 1, P1)

> **Comandos revisados na execução de 2026-09-01 (T010)**: não existe um target de teste de
> integração `e2e_harness` — `farol-core` é um crate só-`bin`, e a Camada 1 vive como módulo
> `#[cfg(test)]` (achado N3, ver § Path Conventions de `tasks.md`).

```bash
# Camada 1 (in-process, sem display): o Program real sob o iced_test::Emulator
cargo test --package farol-core e2e_tests

# Camada 2 (smoke do binário real, precisa de xvfb)
./tests/integration/harness.sh
```

**Esperado (Camada 1)**: os três cenários de `crates/farol-core/src/e2e_tests.rs` passam —
`uptime-kuma` alcança `Ready` (gate T004), `uptime-kuma` popula o widget `monitor-status-grid` com
os monitores da fixture HTTP determinística (T006) e `git-local` percorre um handshake real até
`Unavailable{VersionIncompatible}` (débito técnico #4, resultado deliberado). Nenhum processo filho
remanescente; cada cenário conclui bem abaixo dos tetos de 30s/120s (`## Clarifications`).

**Esperado (Camada 2)**: `harness.sh` confirma as cinco condições do
`contracts/e2e-harness-contract.md` e imprime `SUCESSO — 5/5 condições confirmadas em Ns`, com
saída `0`. Falhando, a linha `[FALHA] ...` diz qual condição caiu, sem exigir leitura de log bruto.

### Regressão deliberada — o que mudou (achado N1)

O desenho original deste cenário mandava reintroduzir um closure capturante em `Subscription::map`
e observar o `debug_assert!` panicar em runtime. **Isso não é mais possível**: em `iced` 0.14 aquele
`debug_assert!` virou `const { check_zero_sized::<F>() }`, ou seja, a verificação subiu de runtime
para tempo de compilação. Não há execução de harness a observar — o defeito deixa de existir num
binário compilado.

O "cenário de falha clara" continua existindo; só mudou de superfície: **o build falha, com uma
mensagem apontando exatamente o problema**. Reintroduzindo o padrão histórico
(`.map(move |(_, event)| Message::Worker { plugin_name: plugin_name.clone(), event })`) e rodando
`cargo test --package farol-core --no-run`:

```text
error[E0080]: evaluation panicked: The Subscription closure provided is not non-capturing.
Closures given to Subscription::map or filter_map cannot capture external variables.
If you need to capture state, consider using Subscription::with.
...
note: the above error was encountered while instantiating
      `fn Subscription::<(String, WorkerEvent)>::map::<{closure@...}, ...>`
  --> crates/farol-core/src/e2e_tests.rs:1193:25
```

**Esperado**: erro `E0080` nomeando o arquivo, a linha e a coluna do closure ofensor, mais a
correção a aplicar (`Subscription::with`) — diagnóstico estritamente melhor que o panic de runtime
que ele substituiu (Acceptance Scenario 3 de US1, SC-001, satisfeito de forma ainda mais forte).
Reverter a regressão antes de continuar.

**Detalhe observado em T010, relevante para o CI (US3)**: uma regressão confinada a código de
**teste** (`#[cfg(test)]`) não quebra `cargo build --bin farol` — logo, não quebra `harness.sh`, que
compila só o binário. Quem pega esse caso é `cargo test`/`cargo clippy --all-targets`. Reintroduzido
no código de produção (`update.rs`/`plugin_worker.rs`), o mesmo `E0080` derruba o build do binário e,
por consequência, o `harness.sh` já no primeiro passo. Os dois jobs de `ci.yml` (`rust-test`/
`rust-lint` e `rust-smoke`) são, portanto, complementares também para esta classe de defeito.

### Regressão deliberada observável em runtime

Como a classe acima virou erro de compilação, o cenário "harness pega um defeito **rodando**"
precisa de outro gatilho. Dois foram verificados nesta sessão, ambos com falha clara:

- **Camada 1** — alterar um monitor esperado em `expected_monitors()`: falha em 30s com
  `o widget monitor-status-grid não foi populado em 30s (requisições autenticadas servidas pela
  fixture: 2, não autenticadas: 0) — estado observado: Ready`.
- **Camada 2** — remover o `api_key` do `secrets.toml` da fixture: falha com
  `[FALHA] uptime-kuma não alcançou Ready em 30s (nenhum widget/get na transcrição core→plugin)`,
  saída `1`.

## Cenário 2 — Verificação de contrato pega valor de borda que a implementação rejeita (User Story 2, P2)

```bash
cargo test -p farol-protocol
```

**Esperado**: o caso já conhecido nesta sessão de planejamento (`research.md` D3) **falha** na
primeira execução após esta feature ser implementada — `response_time_ms: -1` (permitido pelo
schema, `MonitorStatusItem.response_time_ms` sem `minimum`) rejeitado por `Option<u32>`. Este é o
resultado esperado e correto: a suíte prova que o mecanismo funciona (SC-002) revelando um gap real
já existente, cuja correção é um item separado (`## Out of Scope` de `spec.md`). Depois que esse
item for corrigido (fora desta feature), o mesmo teste passa a ficar verde sem nenhuma mudança no
gerador.

**Regressão deliberada** (depois que o gap acima já estiver corrigido em uma feature futura):
apertar deliberadamente um tipo Rust além do que o schema permite (ex.: trocar um campo `Option<T>`
nullable por `T` não-nullable, contrariando `"type": [..., "null"]` do schema) e confirmar que
`cargo test -p farol-protocol` falha, apontando `schema_file`/`json_pointer`/`boundary_kind`/`value`
exatos (`contracts/contract-boundary-testing.md`). Reverter antes de continuar.

**Resultado real (T016, 2026-09-01)**: confirmado antes de aplicar `#[ignore]` — o teste
`widget_monitor_status_item_response_time_ms_negative_value_is_a_known_protocol_gap`
(`crates/farol-protocol/tests/schema_boundaries.rs`) falhou na primeira execução com exatamente a
mensagem FR-006 esperada (`schema_file=widget.schema.json`,
`json_pointer=/$defs/MonitorStatusItem/properties/response_time_ms`,
`boundary_kind=NoMinimumNegative`, `value=-1`, erro serde `invalid value: integer -1, expected
u32`) — prova do mecanismo (SC-002). Per a decisão já registrada em `tasks.md` T013, esse teste
específico MUST ficar `#[ignore]`d até a issue de débito T029 ser resolvida, para não deixar CI
permanentemente vermelho por um gap pré-existente não relacionado a cada PR futura. Com o
`#[ignore]` aplicado, `cargo test -p farol-protocol` fica verde (12 passed, 1 ignored no arquivo
`schema_boundaries.rs`, além dos 21 de `contract_schema_validation.rs` e 18 unitários); rodar
`cargo test -p farol-protocol --test schema_boundaries -- --ignored` continua reproduzindo a falha
acima sob demanda, sem depender de reverter o `#[ignore]`. Regressão adicional simulada (T016):
suprimir manualmente o caso `NoMinimumNegative` da lista retornada por
`numeric_and_null_boundary_cases` para uma propriedade sem `minimum` fez os testes do gerador
(`generator_no_minimum_negative_only_when_schema_truly_has_no_lower_bound`) falharem
imediatamente, confirmando que o próprio gerador é exercitado por teste, não só as aplicações
por schema; alteração revertida antes de prosseguir.

## Cenário 3 — CI dispara automaticamente e sinaliza quebra (User Story 3, P3)

```bash
git checkout -b demo/quebra-deliberada
# introduzir uma quebra trivial, ex.: um `assert_eq!(1, 2)` num teste existente
git commit -am "demo: quebra deliberada para validar CI"
git push -u origin demo/quebra-deliberada
gh pr create --fill
```

**Esperado**: os 5 jobs de `.github/workflows/ci.yml` (`research.md` D6) disparam automaticamente
sem nenhuma ação manual além de abrir a mudança; o job `rust-test` fica vermelho, visível como check
da PR antes de qualquer revisão humana (Acceptance Scenario 2 de US3). Reverter a quebra, `git push`
de novo, confirmar que todos os jobs ficam verdes (Acceptance Scenario 3) — SC-003.

**Limitação de validação conhecida (T022, 2026-09-01)**: este cenário não pôde ser executado por
uma subtarefa de implementação isolada (sem `gh`/rede autenticada nem escopo para abrir PR real no
repositório remoto; `act` não disponível no sistema usado). Validação alternativa realizada: cada
comando exato de cada job de `ci.yml` rodado manualmente na worktree, incluindo uma sabotagem
temporária (um `assert_eq!` de teste Rust e uma asserção de teste Python alterados, cada um
confirmado como causador de saída não-zero visível, depois revertido sem deixar diff) — ver nota em
`tasks.md` T022 para o detalhe completo. O disparo automático via `push`/`pull_request` do GitHub
Actions em si (o mecanismo, não os comandos) segue não verificado nesta sessão.

## Cenário 4 — Regressão visual é detectada sem inspeção manual (User Story 4, P4)

> **Comando revisado na execução de 2026-09-01 (T025)**: mesmo achado N3 do Cenário 1 — `farol-core`
> é um crate só-`bin`, sem target `lib`, então não existe (nem pode existir sem criar `src/lib.rs`) um
> target de teste de integração `--test visual_snapshot`. A Camada de verificação visual vive como
> módulo `#[cfg(test)]` dentro do próprio bin, igual a `e2e_tests.rs`.

```bash
cargo test --package farol-core visual_snapshot_tests
```

**Esperado (primeira vez / sem mudança)**: passa, snapshot estável (Acceptance Scenario 2 de US4).

**Regressão deliberada**: alterar um texto visível em `view.rs` para um dos três `screen_id`s
cobertos (`data-model.md` §3 — ex. o título de `view_setup_form`) e rodar de novo:

**Esperado**: o teste falha, `insta` mostra o diff textual exato do que mudou (Acceptance Scenario 3
de US4, SC-005) — sem abrir o Farol. Reverter a mudança, ou (se a mudança fosse intencional) rodar
`cargo insta review` e commitar o `.snap` atualizado.

**Resultado real (T025, 2026-09-01)**:

Estabilidade (Acceptance Scenario 2) — `cargo test --package farol-core visual_snapshot_tests` rodado
duas vezes seguidas sem nenhuma mudança em `view.rs`/`update.rs`/`model.rs`:

```text
running 3 tests
test visual_snapshot_tests::version_incompatible_screen_matches_snapshot ... ok
test visual_snapshot_tests::dashboard_ready_screen_matches_snapshot ... ok
test visual_snapshot_tests::setup_form_screen_matches_snapshot ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 35 filtered out; finished in 0.39s
```

3/3 verdes nas duas execuções, nenhum `crates/farol-core/src/snapshots/*.snap.new` gerado — nenhum
alarme falso.

Regressão deliberada — o headline do braço `UnavailableReason::VersionIncompatible` em
`view.rs::unavailable_message` alterado de `"Plugin indisponível — versão de protocolo
incompatível"` para `"Plugin indisponível — versão de protocolo INCOMPATÍVEL (regressão deliberada
T025)"`:

```text
Snapshot file: crates/farol-core/src/snapshots/farol__visual_snapshot_tests__VersionIncompatible.snap
Snapshot: VersionIncompatible
Source: crates/farol-core/src/visual_snapshot_tests.rs:256
────────────────────────────────────────────────────────────────────────────────
Expression: extract_visible_text(app.view())
────────────────────────────────────────────────────────────────────────────────
-old snapshot
+new results
────────────┬───────────────────────────────────────────────────────────────────
    1     1 │ git-local
    2       │-Plugin indisponível — versão de protocolo incompatível
          2 │+Plugin indisponível — versão de protocolo INCOMPATÍVEL (regressão deliberada T025)
    3     3 │ plugin fala protocolo 0.1, core fala 0.2 (débito técnico #4)
    4     4 │ uptime-kuma
    5     5 │ Iniciando plugin...
────────────┴───────────────────────────────────────────────────────────────────

thread 'visual_snapshot_tests::version_incompatible_screen_matches_snapshot' panicked at .../insta-1.48.0/src/runtime.rs:719:13:
snapshot assertion for 'VersionIncompatible' failed in line 256
test visual_snapshot_tests::version_incompatible_screen_matches_snapshot ... FAILED

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 35 filtered out; finished in 0.43s
```

Diff textual exato, linha a linha, nomeando arquivo/linha do teste (`visual_snapshot_tests.rs:256`) —
Acceptance Scenario 3 e SC-005 satisfeitos. Note que **só** o snapshot `VersionIncompatible` falhou:
`DashboardReady` e `SetupForm` continuaram verdes na mesma execução, confirmando que a mudança em um
`screen_id` não produz alarme falso cruzado nos outros dois. Regressão revertida em seguida
(`view.rs` volta ao texto original) e o arquivo `.snap.new` pendente removido; `cargo test --package
farol-core visual_snapshot_tests` volta a 3/3 verde, `git status`/`git diff` confirmam `view.rs` sem
diff residual.

## Cenário 5 — Confirmação de que nada trava indefinidamente (Edge Case do `spec.md`)

```bash
time cargo test --workspace --test e2e_harness
```

**Esperado**: conclui bem abaixo de 120s por cenário (timeout de `## Clarifications`) mesmo no pior
caso local; nenhum processo Python remanescente depois (`ps aux | grep plugins/`).
