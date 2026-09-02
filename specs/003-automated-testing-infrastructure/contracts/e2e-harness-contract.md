# Contrato: Harness de Execução Real (US1)

**Feature**: `003-automated-testing-infrastructure` | Decisão de origem: `research.md` D1/D2/D5

Duas camadas, cada uma com seu próprio contrato de entrada/saída. Nenhuma delas expõe uma API
pública fora do próprio processo de teste/CI — o "contrato" aqui é o comportamento observável que
`/speckit-tasks` e a implementação MUST respeitar.

## Camada 1 — in-process (`iced_test`, primária)

**Local**: `crates/farol-core/tests/e2e_harness.rs` (ou submódulo equivalente — decisão de
`/speckit-tasks`), como teste de integração do crate, rodando sob `cargo test --workspace`.

**Entrada**: uma `Fixture de execução` (`data-model.md` §1) por cenário — no mínimo um cenário por
plugin conhecido (`git-local`, `uptime-kuma`) alcançando `PluginState::Ready`, e um cenário
reproduzindo cada um dos dois bugs históricos de `Subscription::map` (como teste de regressão
nomeado, referenciando o commit/task original em comentário — `AGENTS.md` § Armadilha).

**Execução**: constrói o `Program` real de `Farol` (mesma função que `main.rs` usa — extraída para
ser reutilizável, `research.md` D5), injeta a fixture como variável de ambiente do processo filho, e
dirige via `iced_test::Emulator` até que:
- o(s) plugin(s) da fixture alcancem `expected_terminal_state`, **ou**
- o timeout por verificação (30s, `## Clarifications` de `spec.md`) expire.

**Saída esperada (sucesso)**: o teste retorna `Ok`/não panica; nenhum processo filho remanescente
(garantido por `kill_on_drop` já existente em `plugin_worker.rs`, reafirmado, não uma capacidade
nova desta feature).

**Saída esperada (falha)**: o teste falha com uma mensagem que identifica — sem exigir leitura de
log bruto (FR-004) — (a) qual `plugin_name` não alcançou `expected_terminal_state`, (b) qual estado
foi de fato observado (ou "nenhum, timeout excedido"), (c) se a causa foi um `panic!`/`debug_assert`
do próprio `Farol::subscription()`/`update()` (nesse caso, o panic do Rust já carrega a stack trace
— o harness não precisa reformatá-la, só não deve escondê-la atrás de um `catch_unwind` silencioso).

**Timeout total do cenário**: 120s (`## Clarifications`), via `tokio::time::timeout` envolvendo o
cenário inteiro — excedido, o teste falha explicitamente como timeout, distinto de uma falha de
asserção (FR-003).

## Camada 2 — smoke de processo real (secundária)

**Local**: `tests/integration/harness.sh` (substitui o stub nunca escrito, referenciado por
`tests/integration/README.md`).

**Entrada**: nenhuma — roda contra o binário já compilado (`cargo build --bin farol` como
pré-requisito documentado, não parte do script).

**Execução**:
```bash
xvfb-run -a target/debug/farol &
PID=$!
sleep <janela curta, ex. 5s>
if ! kill -0 "$PID" 2>/dev/null; then
  echo "farol saiu sozinho antes da janela de observação" >&2
  exit 1
fi
kill -TERM "$PID"
wait "$PID"
EXIT_CODE=$?
# EXIT_CODE esperado: 0 (SIGTERM tratado) ou o código correspondente a
# encerramento por sinal — decisão exata de aceitação é tarefa de
# /speckit-tasks, documentada aqui como contrato de intenção, não de bytes.
```

**Saída esperada (sucesso)**: processo sobe, sobrevive à janela de observação, encerra limpo ao
receber `SIGTERM`.

**Saída esperada (falha)**: código de saída não-zero do script, com a linha de diagnóstico
identificando qual das três condições falhou (não subiu / morreu sozinho / não encerrou limpo) —
FR-004 aplicado à Camada 2.

**Pré-requisito de ambiente**: `xvfb` instalado (`apt-get install -y xvfb` no job de CI,
`research.md` D6) — mesma técnica já usada manualmente e documentada em `AGENTS.md`.

## Distinção dependência-externa vs. falha do Farol (FR-012)

Como ambas as camadas usam exclusivamente fixtures sintéticas locais (`research.md` D2), nenhum
cenário do harness em CI depende de fato de um serviço externo real — a distinção de FR-012, embora
normativa, não tem caminho de código que a exercite nesta versão da feature (nenhuma dependência
externa real é usada). Se um cenário futuro contra uma instância Uptime Kuma real for adicionado
(opcional, local/manual — `## Clarifications` de `spec.md`), ele MUST reportar falha de conectividade
com uma mensagem/categoria distinta (ex. prefixo `[dependência externa]`) de qualquer falha de
asserção de estado do Farol — este contrato fica registrado aqui para orientar essa extensão futura,
sem implementá-la agora.
