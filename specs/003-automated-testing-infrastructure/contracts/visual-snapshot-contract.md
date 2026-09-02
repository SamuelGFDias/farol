# Contrato: Verificação Visual Declarativa (US4)

**Feature**: `003-automated-testing-infrastructure` | Decisão de origem: `research.md` D4

**Local**: `crates/farol-core/tests/` (junto da Camada 1 do harness, ou arquivo próprio, ex.
`visual_snapshot.rs`), com referências versionadas em `crates/farol-core/tests/snapshots/*.snap`
(convenção do crate `insta`).

## Entrada

Um `screen_id` (`data-model.md` §3: `DashboardReady`, `SetupForm`, `VersionIncompatible`) e o estado
`Farol` correspondente, construído com a mesma técnica já usada pelos testes existentes de
`update.rs` (reaproveitar, não duplicar, os construtores de fixture de estado já presentes no crate).

## Execução

1. Constrói o `Farol` no estado do `screen_id`.
2. Chama `view()` sobre esse estado — o mesmo `Element<Message>` que `main.rs` renderizaria de
   verdade.
3. Extrai uma representação textual determinística via a `Selector` API de `iced_test` — no mínimo:
   todo texto visível (`text!`/`button`/`text_input` com seu `value`/placeholder), em ordem de
   composição estável (a mesma ordem que `view.rs` já monta, sem reordenação artificial).
4. Compara essa string contra o snapshot versionado via `insta::assert_snapshot!` (ou macro
   equivalente do crate).

## Saída esperada (sucesso)

Snapshot inalterado entre duas execuções sem mudança relevante em `view.rs`/`update.rs`/`model.rs`
para aquele `screen_id` — nenhum alarme falso (Acceptance Scenario 2 de US4).

## Saída esperada (falha / mudança detectada)

`insta` reporta um diff textual linha a linha entre o snapshot versionado e o novo texto extraído —
uma pessoa revisando a mudança vê exatamente o que mudou (texto novo, campo removido, ordem
diferente), sem precisar abrir o aplicativo manualmente (Acceptance Scenario 3 de US4). Em modo
interativo local, `insta` permite aceitar o novo snapshot como referência (`cargo insta review`); em
CI, a suíte falha até que a mudança seja revisada e o `.snap` correspondente seja atualizado e
commitado por quem propôs a mudança de tela.

## Extensão não-bloqueante: captura de pixels

`iced_test::screenshot()` existe e pode ser adotada no futuro para os mesmos `screen_id`s, gerando
um artefato de imagem em vez de (ou além de) texto — **não faz parte desta feature** (`research.md`
D4: viabilidade sob os runners de CI hospedados não verificada nesta sessão). Se adotada depois,
MUST seguir FR-011: marcada como verificação local/manual-assistida até que a viabilidade em CI seja
confirmada, nunca bloqueando os jobs já obrigatórios (`research.md` D6).

## Extensibilidade (FR-013)

Um novo `screen_id` (nova tela renderizada por `view.rs` no futuro) é adicionado como mais uma
entrada na função que constrói o estado + mais um `assert_snapshot!` — não exige nenhuma mudança no
mecanismo (`Selector`/`insta`) em si.
