# Phase 0 Research: Infraestrutura de Testes Automatizada

**Feature**: `003-automated-testing-infrastructure` | **Data**: 2026-09-01

Este documento resolve as decisões técnicas necessárias antes do design (Fase 1), no mesmo padrão
de `specs/001-walking-skeleton-git-plugin/research.md` e `specs/002-uptime-kuma-plugin/research.md`
(numeração `D1`–`D6` própria desta feature). Contexto obrigatório já lido: `spec.md` (14 FRs + 3
decisões de `## Clarifications`), `AGENTS.md` (arquitetura do core, a armadilha de
`Subscription::map`, o estado de `tests/integration/`), `.specify/memory/constitution.md` v1.0.0,
`specs/002-uptime-kuma-plugin/{plan,research,tasks}.md` (os três bugs reais que motivam esta
feature), o código-fonte de `crates/farol-core/src/{main,update,plugin_worker}.rs` e
`crates/farol-protocol/tests/contract_schema_validation.rs`, e uma pesquisa externa dedicada sobre
`iced` 0.14/`iced_test` (ver § Fontes).

---

## D1 — Harness de execução real (US1): upgrade `iced` 0.13 → 0.14 + `iced_test`, arquitetura em duas camadas

Esta é a decisão técnica central desta feature — determina se US1 é resolvida reaproveitando um
framework de teste de primeira classe do próprio `iced`, ou se precisa ser construída do zero por
cima da infraestrutura já existente (spawn manual + `eprintln!` de diagnóstico, o processo lento e
não-repetível descrito no `spec.md`).

### O achado e sua verificação

O arquiteto levantou que `iced` 0.14 (crates.io, publicado **2026-12-07** — confirmado via
`https://crates.io/api/v1/crates/iced`, corrige a data aproximada "dezembro/2025" da hipótese
original) introduziu um crate `iced_test` com suporte de primeira classe a testes E2E headless:
`Simulator` (simula interação sem efeitos colaterais reais) e `Emulator` (roda a aplicação em
runtime headless, executando `Task`/`Subscription` de verdade), uma `Selector` API (busca na árvore
de widgets por texto/`Id`, sem captura de tela) e um DSL `.ice` para gravar/reproduzir interações —
tudo isso sem exigir GPU, X11, Wayland nem Xvfb (`docs.iced.rs/iced_test`, confirmado por pesquisa
externa nesta sessão). O achado também apontava, corretamente, que isso pode resolver US1 e US4
simultaneamente e pedia validação própria antes de comprometer o plano — essa validação é o
conteúdo desta decisão.

**O que a pesquisa externa confirmou** (`github.com/iced-rs/iced` release `0.14.0`, `CHANGELOG.md`,
`docs.iced.rs/iced_test`):

- `iced_test` existe, é headless por design, e o `Emulator` de fato roda `Task`/`Subscription`
  reais — não apenas os simula. Isso é decisivo: os dois panics históricos (`AGENTS.md`) acontecem
  dentro de `Farol::subscription()`, no ponto em que `Subscription::map`/`Subscription::batch` são
  chamados para montar a `Subscription` — **a montagem em si já dispara o `debug_assert!` de
  closure não-capturante**, sem exigir sequer que a `Subscription` seja de fato *polled* por um
  runtime `iced` completo (ver achado abaixo, "descoberta lateral"). O `Emulator` roda o loop
  `update`/`subscription` real da aplicação, então qualquer regressão equivalente aos dois bugs
  históricos aconteceria organicamente, sem exigir que quem escreve o teste adivinhe de antemão qual
  estado (`PluginState`, `setup_attempt`) precisa ser manualmente construído para alcançar o branch
  defeituoso — essa é a diferença real de valor frente a um teste unitário chamando
  `Farol::subscription()` diretamente (ver descoberta lateral).
- O `Program` trait (com `boot`/`update`/`view`/`subscription`/`presets()`) é o que
  `iced::application(...)` já constrói por baixo — `farol-core/src/main.rs` usa exatamente essa
  forma de builder (`iced::application("Farol", Farol::update, Farol::view).subscription(Farol::
  subscription).run()`), sem nenhum tipo próprio nomeado implementando `Program` manualmente. A
  documentação pública não confirma de forma explícita, byte a byte, que o valor devolvido por esse
  builder é diretamente aceito por `iced_test::run`/`Simulator::new`/`Emulator::new` sem nenhum
  ajuste — permanece uma lacuna de certeza (ver § Risco).
- O changelog de 0.14.0 não registra nenhuma mudança em `Subscription::map`/`Subscription::
  run_with_id`/`iced::stream::channel` em si (só adições aditivas: `Subscription::filter_map`,
  `time::repeat`, unificação de subscriptions de teclado) — o padrão já usado e documentado em
  `AGENTS.md` (stream carrega o valor final, `.map()` só com closure zero-sized) continua válido sob
  0.14 pela evidência disponível.
- As mudanças de fato *breaking* confirmadas no changelog (`Task::perform` de `Fn` para `FnOnce` —
  afrouxamento, compatível com closures `Fn` já existentes; `Widget::update` passando `Event` por
  referência — só afeta implementações próprias de `Widget`) **não atingem `farol-core`**:
  `view.rs` usa só widgets de estoque (`button`, `column`, `container`, `row`, `text`,
  `text_input`), nenhum `impl Widget` próprio existe no crate. Isso reduz materialmente o risco de
  quebra de compilação da migração além do necessário para adotar `iced_test`.

**Descoberta lateral, relevante para calibrar o valor real desta decisão**: `update.rs` já contém
dois testes de unidade (`subscription_does_not_panic_after_setup_attempt_increments`,
`subscription_does_not_panic_with_a_ready_plugin`) adicionados **depois** da correção dos dois
panics históricos, que constroem manualmente o estado (`setup_attempt = 1`, um plugin em `Ready`) e
chamam `Farol::subscription()` diretamente sob `cargo test` puro — sem `iced_test`, sem runtime
`iced` real — e já bastam para pegar a classe de regressão de `Subscription::map` capturante, *desde
que* quem escreve o teste already saiba qual estado ativa o branch. A lição documentada em
`AGENTS.md` ("`Farol::default()` não ativa nenhum dos dois casos acima") é exatamente isso: o
`Emulator` não substitui a necessidade de testes unitários dirigidos por estado — ele resolve um
problema diferente e complementar, o de alcançar organicamente estados não antecipados por quem
escreve o teste, através de uma execução real do app (handshake real, ciclo de refresh real,
processo filho real), sem exigir onisciência prévia de qual condição de runtime precisa ser
reproduzida manualmente.

### Cobertura de US1 e US4 pelo achado — avaliação própria, não aceita cegamente

- **US1 (harness de execução real)**: coberto **parcialmente, não integralmente**, pelo `Emulator`.
  Ele roda o `Program` real (`update`/`subscription`/`view` de `Farol`) dentro do **mesmo processo
  do binário de teste** (`cargo test`), não como um processo OS separado do binário compilado
  `target/debug/farol`. Como a `Subscription` de cada plugin (`plugin_worker::subscription`) spawna
  de verdade um `tokio::process::Command` (o interpretador Python do plugin), o `Emulator` **exercita
  genuinamente comunicação entre processos reais** (core in-process ↔ plugin como processo filho de
  verdade) — a metade "plugin" de FR-001/FR-002 é coberta literalmente. A metade "core" é coberta
  **funcionalmente, não literalmente**: o mesmo código de `update.rs`/`plugin_worker.rs` roda, mas
  não dentro do binário `farol` como processo OS separado, e não passa pelo backend real de janela
  (winit/wgpu) que `cargo run --bin farol` usa. Ver D5 para a resposta: um harness de duas camadas,
  não uma substituição integral por `iced_test` sozinho.
- **US4 (testes de UI/visual)**: coberto **bem, como abordagem declarativa** — exatamente o tipo de
  "verificação declarativa... sem depender de captura de tela" que a `## Assumptions` do `spec.md`
  já antecipa como aceitável (FR-009/FR-010). Ver D4 para o desenho concreto.

### Decisão

Adotar `iced` **0.14** (bump de `~0.13`, `crates/farol-core/Cargo.toml`) e o crate `iced_test` como
`dev-dependency` de `farol-core`, para a camada in-process do harness de execução real (D5) e para a
verificação visual declarativa (D4). **Gate obrigatório antes de investir no restante do harness**:
uma task própria e cedo em `tasks.md` — spike de migração — que (a) faz só o bump de versão e
confirma `cargo build --workspace` sem quebra de compilação (dado o risco já reduzido pela ausência
de `impl Widget` próprio), (b) escreve exatamente um teste `iced_test`-based que reproduz o padrão
do primeiro bug histórico (`Subscription::map` capturante) contra um estado que deliberadamente o
reintroduz, confirma que esse teste falha (prova que o mecanismo pega o defeito), reverte a
reintrodução deliberada, confirma que o teste passa. Só depois desse spike o restante do harness
(D5) é construído em cima da confirmação real, não da hipótese.

### Alternativas consideradas

- **Não fazer upgrade, construir o harness só com spawn manual de `target/debug/farol` + parsing de
  stdout/stderr instrumentado** (a extensão natural do que já foi feito manualmente em T023 da
  feature 002): viável, mas reproduz exatamente o processo lento e frágil que esta feature existe
  para eliminar — exigiria adicionar logging estruturado permanente só para tornar o processo
  observável programaticamente, sem ganhar nenhuma das garantias que o `Emulator` já dá de graça
  (execução real de `Task`/`Subscription` com introspecção de estado Rust nativa, não parsing de
  texto). Rejeitada como única camada, mas sobrevive como a camada 2 (D5) — narrow, específica para
  o que só um processo OS separado prova.
- **Ficar em `iced` 0.13 e construir uma versão própria e simplificada de "rodar `Farol::update`/
  `subscription` em um runtime de teste"** (reimplementar em miniatura o que `iced_test` já
  resolve): rejeitada — complexidade permanente e manutenção própria por um benefício que o upstream
  já entrega, testado pela própria comunidade `iced`; contradiz FR-013 (extensibilidade sem
  reconstrução) ao criar um mecanismo paralelo e não-padrão.

---

## D2 — Fixtures do harness: fixture HTTP sintética para `uptime-kuma`, repositório git sintético para `git-local`

Decorrente da terceira clarificação de `spec.md`: nenhuma credencial real, nenhum serviço externo
real, no cenário de harness que roda em CI.

### Decisão

- **`git-local`**: fixture já trivial — um diretório git real, criado no próprio processo de setup
  do teste (`git init` + um commit em um `tempfile::TempDir` ou equivalente), servindo de
  `scan_root`. Não depende de rede nem de segredo — nenhuma decisão nova além de "criar
  deterministicamente", já implícito no design do plugin (feature 001).
- **`uptime-kuma`**: precisa de um duplo determinístico do endpoint `/metrics` — um servidor HTTP
  minimalista, subido no mesmo processo de teste (Rust: `tokio::net::TcpListener` +
  `hyper`/handler mínimo, ou reaproveitando um crate leve já comum no ecossistema `tokio` — decisão
  de implementação, não normativa aqui), servindo um corpo Prometheus sintético fixo (2–3 monitores
  com status/tempo de resposta conhecidos) na rota `/metrics` sob HTTP Basic Auth com uma API key
  sintética fixa (`farol-e2e-fixture-key`, nunca usada contra nenhuma instância real). O plugin
  `uptime-kuma` é configurado, via `required_config` injetado como variável de ambiente pelo próprio
  harness (mesmo mecanismo de produção, `secrets_store`/`config_store`), para apontar para
  `http://127.0.0.1:<porta-efêmera>` — nunca um host de rede real, resolvendo também a exigência de
  FR-012 (dependência externa indisponível vs. falha do Farol: com fixture local, essa distinção
  simplesmente não se aplica ao cenário-padrão de CI; um cenário opcional contra Uptime Kuma real,
  se existir, é local/manual, nunca obrigatório).

### Alternativas consideradas

- **Plugin de teste dedicado, escrito só para o harness** (um terceiro plugin Python/Rust mínimo,
  sibling de `git-local`/`uptime-kuma`, que sempre alcança `Ready` sem I/O real nenhum): considerado
  para o cenário "genérico" de FR-013 (cobrir plugin futuro sem reconstrução) — não descartado, mas
  não necessário como fixture *primária*, porque os dois plugins de referência já existentes já
  produzem cobertura mais realista (exercitam o handshake/`required_config`/parsing real) pelo mesmo
  custo. Registrado como extensão natural (não tarefa desta feature) caso um cenário futuro precise
  de um plugin que falha de propósito de forma controlada (ex.: simular timeout de handshake) sem
  depender de sabotar um dos dois plugins reais.

---

## D3 — Cobertura de contrato mais rigorosa (US2): gerador determinístico de valores de borda a partir dos schemas, sem dependência nova de teste por propriedade

### Achado concreto durante esta pesquisa — o próprio caso que motivou US2 ainda está latente

`protocol/schema/v0.2/widget.schema.json`, campo `MonitorStatusItem.response_time_ms`, declara
`"type": ["integer", "null"]` **sem `minimum`** — ou seja, o schema normativo permite explicitamente
qualquer inteiro, incluindo negativo, além de `null`. O tipo Rust correspondente
(`crates/farol-protocol/src/messages.rs`) é `pub response_time_ms: Option<u32>` — `u32` não
consegue representar valor negativo algum; `serde_json::from_value` para `{"response_time_ms": -1,
...}` falha estruturalmente. O commit `2608c03` ("fix: mapeia sentinela -1 de response_time_ms para
null") corrigiu o **plugin** `uptime-kuma` para nunca mais enviar `-1` pela rede (mapeando o
sentinela para `null` antes de serializar) — uma correção real e válida do lado do produtor, mas que
**não estreitou nem alargou** o contrato formal: o schema `v0.2` continua permitindo `-1` como valor
de `response_time_ms`, e a implementação Rust continua incapaz de aceitá-lo caso outro produtor
(um plugin de terceiro futuro, por exemplo) o envie de boa-fé porque o schema autoriza. Este é
exatamente o tipo de divergência silenciosa entre schema e implementação que FR-005/FR-006 pedem
para pegar automaticamente — e ela não é histórica, **está presente no código hoje**. Fica registrada
aqui como constatação desta sessão de planejamento (não corrigida — Out of Scope do `spec.md`: "cada
defeito revelado é tratado como item separado"); a task de contrato desta feature (US2) deve incluir
um caso que a revele deliberadamente, servindo tanto de regressão quanto de prova de que o mecanismo
funciona antes de qualquer correção futura acontecer.

### Decisão

Um gerador determinístico (não randômico) de valores de borda, implementado como módulo de teste
próprio dentro de `crates/farol-protocol/tests/` (ex.: `schema_boundaries.rs`, chamado a partir de
`contract_schema_validation.rs` ou como arquivo de teste irmão reaproveitando `load_schemas()`).
Para cada propriedade relevante de um schema (`type` incluindo `"integer"`/`"number"`, ou incluindo
`"null"`), o gerador deriva, a partir do próprio JSON do schema já carregado em runtime de teste
(sem hardcode paralelo do valor declarado):

- se `minimum` está declarado: `minimum - 1` (esperado **inválido** contra o schema) e `minimum`
  (esperado **válido**);
- se `minimum` **não** está declarado: um valor negativo representativo, ex. `-1` (esperado
  **válido** contra o schema — é exatamente o caso `response_time_ms` acima) — a ausência de
  `minimum` é, por definição de JSON Schema, ausência de piso;
- se `maximum` está declarado: `maximum` e `maximum + 1` (válido/inválido, análogo);
- se `"null"` está no array de `type`: `null` (esperado válido);
- se a propriedade é obrigatória (`required`): omissão do campo (esperado **inválido**).

Cada valor gerado é injetado em uma instância completa e por outro lado válida do schema que o
contém (reaproveitando os construtores de mensagem já existentes em
`contract_schema_validation.rs`, com o campo sob teste substituído), e verificado em **duas frentes
simultâneas**: (1) contra o próprio `Validator` do schema (`assert_valid`/`assert_invalid` já
existentes) — confirma que a expectativa derivada está correta; (2) contra a desserialização Rust
tipada (`serde_json::from_value::<T>`) — sempre que (1) diz "válido pelo schema", (2) **MUST**
também suceder (esse é o teste de FR-006: implementação mais restritiva que o schema permite). Uma
falha em (2) quando (1) é válido é reportada com o nome do campo, o valor de borda exato e o schema
de origem — cumprindo FR-006 e a segunda metade de FR-005 (borda "derivada diretamente do schema",
não escrita à mão).

### Alternativas consideradas

- **`proptest`/geração randômica com estratégias derivadas do schema**: rejeitada como mecanismo
  primário — para o tamanho finito e conhecido de campos numéricos/nuláveis dos 4 schemas v0.2 (um
  punhado de campos: `response_time_ms`, `ahead`/`behind` de `RemoteStatus::Tracked`,
  `suggested_refresh_interval_ms`, `timeout_hint_ms`, `code` de `ErrorObject`), a enumeração
  determinística dos valores de borda relevantes já é exaustiva e não se beneficia de amostragem
  randômica — e randomização introduz não-determinismo em CI (SC-003/FR-007 exigem sinal confiável a
  cada push, um teste ocasionalmente flaky por seed contradiz isso) sem cobrir nada que a enumeração
  determinística já não cubra. Não descartada como ferramenta *futura* se o protocolo crescer
  significativamente em superfície numérica — só não é o desenho desta feature.
- **Um crate dedicado de "JSON Schema → gerador de instância arbitrária"**: pesquisado e não
  encontrado um candidato maduro/mantido no ecossistema Rust que cubra Draft 2020-12 com a
  fidelidade que os 4 schemas já exigem (`$ref` cruzado entre arquivos, já resolvido hoje via
  `jsonschema::Registry`) — construir um seria desproporcional ao problema (FR-013 pede
  extensibilidade, não um motor de geração de propósito geral).

---

## D4 — Verificação visual/de interface (US4): snapshot declarativo via `iced_test::Selector` + `insta`, pixels como extensão não-bloqueante

### Decisão

A captura de referência (FR-009/FR-010) é **declarativa**, não de pixels: para um pequeno conjunto
de telas conhecidas do Farol (o dashboard com um plugin `Ready` exibindo itens, um plugin
`Unavailable{NotConfigured}` mostrando `view_setup_form`, e um plugin `Unavailable{VersionIncompatible}`
— os três estados de tela mais deliberadamente cobertos por `view.rs`), um teste `iced_test`
constrói o `Farol` real no estado correspondente (mesma construção de estado já usada pelos testes
unitários existentes de `update.rs`, ex. `farol_with_widget`) e usa a `Selector` API para extrair uma
representação textual estável do que a tela mostraria (os textos visíveis e sua estrutura de
composição, via os seletores por texto/`Id` que `iced_test` já expõe). Essa representação é
comparada via `insta` (crate de snapshot testing consolidado do ecossistema Rust, adicionado como
`dev-dependency` de `farol-core`), com o snapshot de referência versionado em
`crates/farol-core/tests/snapshots/`. Uma mudança visível na tela (novo texto, item removido, campo
a mais no formulário) muda a representação textual extraída, e `insta` aponta o diff exato — exatamente
o que FR-010/Acceptance Scenario 3 de US4 pedem, sem depender de captura de tela.

`iced_test` também expõe uma função `screenshot()` (pixel-based) — a documentação pública consultada
não permite confirmar se ela funciona sob os runners headless do GitHub Actions (`ubuntu-latest`)
sem Xvfb, e a pesquisa disponível nesta sessão não chegou a esse nível de detalhe. Por isso: captura
de pixels via `screenshot()` fica marcada como extensão **não-bloqueante**, avaliável localmente
(máquina de desenvolvimento com display real), nunca como dependência do job de CI obrigatório —
resolve diretamente FR-011 e o Edge Case correspondente do `spec.md`, e evita comprometer o plano com
uma capacidade cuja viabilidade em CI não foi verificada nesta sessão de planejamento (mesma cautela
que o achado original do arquiteto já pedia).

### Alternativas consideradas

- **Captura de pixels como abordagem primária** (`screenshot()` de `iced_test` direto em CI):
  rejeitada como *primária* nesta fase — risco de viabilidade não verificado (Xvfb/GPU em runner
  hospedado), e a `## Assumptions`/`## Clarifications` do `spec.md` já autorizam explicitamente uma
  alternativa declarativa. Pode ser promovida a primária numa iteração futura se a viabilidade for
  confirmada (task de spike opcional, não bloqueante desta feature).
- **Snapshot do `Debug` de `Farol` inteiro (struct de estado), sem passar pela `Selector`/`view()`**:
  rejeitada — testaria o *modelo*, não a *tela renderizada*; um bug isolado em `view.rs` (Princípio
  III: core renderiza) que não muda `Farol`/`Message` algum passaria despercebido, contrariando o
  próprio propósito de US4 ("regressões visuais").

---

## D5 — Harness de execução real em duas camadas (consolida D1): in-process (`iced_test`, primária) + smoke de processo real (secundária, fecha o débito de `tests/integration/harness.sh`)

### Decisão

1. **Camada 1 — in-process, primária** (a maior parte da cobertura de US1): testes `iced_test`
   dentro de `crates/farol-core` (novo arquivo `tests/e2e_harness.rs`, ou submódulo de teste dedicado
   — decisão de `/speckit-tasks`), construindo o `Program` real que `main.rs` já expõe (extraído para
   uma função reutilizável, ex. `fn program() -> impl iced::application::Program<...>`, chamada tanto
   por `main()` quanto pelos testes, para nunca haver dois caminhos de construção divergentes) e
   dirigindo-o via `Emulator` contra as fixtures de D2. Cobre literalmente FR-001 (plugin alcança
   `Ready`), FR-002 (encerramento anormal — panic dentro do loop real é capturado pelo runner de
   teste padrão do Rust, que já reporta o teste como falho com stack trace), e — via `tokio::time::
   timeout` envolvendo a espera de cada estado — FR-003/FR-004 com os valores de D-clarify (30s por
   verificação, 120s por cenário).
2. **Camada 2 — smoke de processo real, secundária e deliberadamente estreita**: fecha o débito já
   registrado em `tests/integration/README.md` (que referencia um `harness.sh` nunca construído,
   heranca da feature 001) — um script (`tests/integration/harness.sh`, finalmente escrito) que
   compila (`cargo build --bin farol`), spawna `target/debug/farol` como processo OS separado sob
   `xvfb-run -a` (mesma técnica já usada manualmente e documentada em `AGENTS.md`/T023), aguarda um
   tempo fixo curto e confirma que o processo (a) não sai com código de erro, (b) não morre sozinho
   antes do fim da janela de observação, (c) é encerrado de forma limpa via `SIGTERM` ao final —
   sem tentar inspecionar estado interno (isso é papel da Camada 1). Prova exclusivamente o que só um
   processo OS real prova: o binário compilado sobe de fato, com o entrypoint real (`iced::
   application(...).run()`, backend de janela real), a partir do `cwd` correto. Roda em CI (FR-007);
   por ser mais barata e mais rasa que a Camada 1, não precisa ser pulada mesmo sem GPU real —
   `AGENTS.md` já confirma que o ciclo `update`/handshake roda normalmente sob Xvfb sem GPU, mesmo
   com janela preta.

Nenhuma das duas camadas depende de infraestrutura que não já exista ou que esta feature não
construa — D1/D2 constroem a Camada 1, e a Camada 2 é um script simples sem dependência nova. As
duas rodam como parte do mesmo gatilho de CI (US3/FR-007), não como pipelines separados.

### Alternativas consideradas

- **Só a Camada 1** (abandonar de vez a ideia de smoke de processo real): rejeitada — deixaria sem
  cobertura automatizada exatamente a classe de regressão mais específica do entrypoint real
  (`main.rs`, backend de janela), e deixaria o débito de `tests/integration/harness.sh` (citado
  explicitamente no `spec.md` como parte do contexto motivador) formalmente aberto.
- **Só a Camada 2** (não adotar `iced_test`): rejeitada — é exatamente o "spawn manual + observação
  de saída" que motivou esta feature, sem introspecção de estado Rust nativo, sem afirmação
  específica sobre qual `PluginState` foi alcançado (Acceptance Scenario 2 de US1 exige
  especificamente isso), e sem cobertura convincente do padrão exato dos dois bugs históricos de
  `Subscription::map` (que a Camada 1 cobre por construção).

---

## D6 — CI (US3): um único workflow GitHub Actions, jobs paralelos, orçamento de 10 minutos (SC-004)

### Decisão

Um workflow (`.github/workflows/ci.yml` — repositório hoje sem nenhum workflow, confirmado por
`find .github`) disparado em `push` e `pull_request` (FR-007), com jobs paralelos (não seriais, para
caber no orçamento de SC-004):

- **`rust-test`**: `cargo test --workspace` (71 testes existentes + os novos testes de contrato de
  D3 + os testes in-process de D5 Camada 1, todos sob o mesmo comando — `iced_test` como
  `dev-dependency` já corre dentro de `cargo test` normal, sem harness externo, confirmando a
  característica "roda dentro do `cargo test` normal" do achado original).
- **`rust-lint`**: `cargo clippy --workspace --all-targets -- -D warnings` (mesmo padrão "deve ficar
  limpo, sem warning nenhum" já documentado em `AGENTS.md`).
- **`rust-smoke`**: a Camada 2 de D5 (`tests/integration/harness.sh`), sob `xvfb-run -a`
  (dependência de sistema do runner `ubuntu-latest`, instalável via `apt-get install -y xvfb` no
  próprio job).
- **`python-lint`**: `ruff check` em cada diretório de `plugins/*` que tiver `pyproject.toml` —
  hoje só `plugins/uptime-kuma/` tem um (`plugins/git-local/` não tem `pyproject.toml` próprio,
  achado desta sessão, registrado como nota — não é bloqueio desta feature corrigir isso, mas o job
  de CI precisa lidar com a ausência sem falhar silenciosamente: rodar `ruff check` a partir de cada
  diretório de plugin de qualquer forma, aceitando que `git-local` roda sob as configurações-padrão
  do `ruff` na ausência de arquivo de config próprio).
- **`python-test`**: `pytest` onde já existe (`plugins/uptime-kuma/test_metrics_parser.py`); dado que
  `plugins/git-local/` não tem teste Python próprio hoje, o job simplesmente não encontra nada para
  rodar ali — não é uma lacuna introduzida por esta feature.

A verificação visual (D4) roda dentro de `rust-test` (é só mais um `#[test]` via `insta`) — sem job
próprio, porque não depende de nenhuma capacidade de sistema além do que os outros testes já
precisam (headless, sem GPU/Xvfb necessário para a via declarativa). O status de cada job aparece
como *check* da mudança (FR-008) — mecanismo nativo do GitHub Actions, sem configuração adicional.

### Alternativas consideradas

- **Um job monolítico serial** (tudo em sequência): rejeitada — mais simples de escrever, mas soma
  os tempos em vez de paralelizar, arriscando o orçamento de 10 minutos de SC-004 à medida que a
  suíte cresce; jobs paralelos do GitHub Actions já rodam em runners separados sem custo adicional
  de configuração relevante.
- **Plataforma de CI adicional** (ex. um serviço de CI dedicado a testes visuais): descartada —
  `## Assumptions` do `spec.md` já fixa que nenhuma plataforma de CI além da já hospedada (GitHub)
  é assumida; não há necessidade técnica que justifique revisitar isso.

---

## Fontes da pesquisa externa

- `https://github.com/iced-rs/iced/releases/tag/0.14.0` — release notes de `iced` 0.14.0.
- `https://raw.githubusercontent.com/iced-rs/iced/master/CHANGELOG.md` — changelog completo,
  seção `0.14.0`.
- `https://docs.iced.rs/iced_test/index.html` — documentação do crate `iced_test` (`Simulator`,
  `Emulator`, `Selector`, DSL `.ice`, `screenshot()`).
- `https://crates.io/api/v1/crates/iced` — confirmação da data de publicação de `iced` 0.14.0
  (2025-12-07) e de que é a versão mais recente disponível nesta sessão (2026-09-01).
- `https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary` — cobertura de terceiros sobre a
  release, usada só como triangulação, não como fonte normativa.

---

## Nota de execução (2026-09-01) — resultado do gate T004: hipótese de D1 **refutada no mecanismo**, T001–T004 bloqueadas

Nota **acrescentada** durante a execução de T001–T004 (nada acima foi reescrito). Registra só o que
foi verificado com código de verdade contra `iced` 0.14.0 / `iced_test` 0.14.0 já publicados e
baixados nesta máquina (ambos confirmados existentes via `cargo search`).

### N1 — O padrão do 1º bug histórico virou **erro de compilação** em 0.14, não panic de runtime

`iced_futures` 0.13.2 (`src/subscription.rs:249`) checa a closure de `Subscription::map` com
`debug_assert!(size_of::<F>() == 0, "the closure {} provided in `Subscription::map` is capturing")`
— falha **em runtime**, que é a premissa de T004(a) ("confirmar que o teste falha com o
`debug_assert!`"). Em `iced_futures` 0.14.0 (`src/subscription.rs:288-290`) a mesma checagem virou
um bloco `const { check_zero_sized::<F>(); }`, avaliado **em tempo de compilação**.

Verificado empiricamente (crate isolado, `iced = "0.14"`, reproduzindo o padrão exato do ponto 1 da
armadilha de `AGENTS.md`): a compilação falha com `error[E0080]: evaluation panicked: The
Subscription closure provided is not non-capturing. Closures given to Subscription::map or
filter_map cannot capture external variables. If you need to capture state, consider using
Subscription::with.`

Consequência para D1: **T004 é inexecutável como especificado** — não existe execução de teste a
observar, porque o crate não compila. A classe de bug que motivou a feature inteira deixa de ser
detectável por qualquer harness de runtime sob 0.14, porque o `rustc` a rejeita antes. Isso é uma
boa notícia para o produto e uma refutação do *mecanismo* proposto em D1: o `Emulator` continua
podendo ter valor para US1 em geral (confirmado: `iced_test-0.14.0/src/emulator.rs:422-423` chama
`program.subscription(&state)` e roda as recipes de verdade), mas a evidência que o gate existia
para produzir não pode ser produzida. A justificativa de adotar `iced_test` precisa ser reancorada
em outra coisa que não "reproduzir os dois panics históricos".

### N2 — A migração 0.13→0.14 quebra código de produção **fora** do raio de mudança declarado

D1 afirma que as breaking changes "não atingem `farol-core`". `cargo build --package farol-core`
com o bump aplicado produz **5 erros**, 4 deles em arquivos que `tasks.md` § Notes declara
explicitamente intocáveis nesta feature (`update.rs`, `plugin_worker.rs`):

| # | Arquivo | Erro |
|---|---|---|
| 1 | `src/plugin_worker.rs:517` | `E0282` type annotations needed — `iced::stream::channel` mudou para `f: impl AsyncFnOnce(mpsc::Sender<T>)`, o closure perde a inferência do tipo de `output` |
| 2 | `src/update.rs:561` | `E0282` idem (`refresh_tick_stream`) |
| 3 | `src/plugin_worker.rs:344` | `E0599` `Subscription::run_with_id` **não existe mais** |
| 4 | `src/update.rs:97` | `E0599` idem |
| 5 | `src/main.rs:27` | `E0277` `iced::application` mudou de `(title, update, view)` para `(boot: impl BootFn, update, view)` — o título passa a ser builder `.title(...)` |

O item 3/4 **não é rename mecânico**: a substituta é `Subscription::run_with<D, S>(data: D, builder:
fn(&D) -> S) where D: Hash` — o builder é um **ponteiro de função não-capturante**, não um `Stream`
já construído. Hoje `plugin_worker::subscription` faz
`Subscription::run_with_id(format!("{plugin_name}-{setup_attempt}"), worker(config).map(...))`, ou
seja, constrói o stream capturando `config`. Sob 0.14 é preciso redesenhar isso: `PluginSpawnConfig
+ setup_attempt` teriam de virar um `D: Hash` e `worker` teria de ser reconstruído dentro de um
`fn(&D) -> S`. Esse é exatamente o mecanismo de reconexão pós-setup documentado em `AGENTS.md`
(o `setup_attempt` no `id` é o que mata o processo filho antigo) — redesenhá-lo é decisão de
arquitetura, não coberta por nenhuma task escrita.

### N3 — `crates/farol-core` não tem target `lib`: `tests/e2e_harness.rs` não alcança `program()`

`cargo metadata --no-deps` devolve para `farol-core` exatamente um target: `('farol', ['bin'])`.
Não há `src/lib.rs` nem `[lib]` no `Cargo.toml`. Um teste de integração em `crates/farol-core/tests/`
compila como crate separado e só pode importar de um target `lib` — que não existe. Verificado
empiricamente com um crate bin-only mínimo: `error[E0432]: unresolved import` / `use of unresolved
module or unlinked crate`.

Consequência: T002 (`pub(crate) fn program()` em `main.rs`) + T004 (`crates/farol-core/tests/
e2e_harness.rs` usando `program()`) são **estruturalmente impossíveis como especificados**, e o
comando de verificação `cargo test --package farol-core --test e2e_harness` não pode existir. Toda a
§ Path Conventions de `tasks.md` (`tests/e2e_harness.rs`, `tests/visual_snapshot.rs`,
`tests/support/fixtures.rs`) depende disso. Sair do impasse exige uma de duas decisões de
arquitetura, nenhuma coberta pelas tasks atuais: (a) criar `src/lib.rs` expondo os módulos e reduzir
`main.rs` a um binário fino — obriga a alargar visibilidades (`pub(crate)` → `pub`) em `model.rs`/
`update.rs`/`plugin_worker.rs`/`view.rs`, fora do raio declarado; ou (b) mover os testes `iced_test`
para `#[cfg(test)] mod tests` **dentro** do bin target, contrariando a estrutura de arquivos de
`plan.md`/`tasks.md`.

### Recomendação (não executada — decisão do arquiteto)

Revisitar D1 antes de qualquer task de US1/US4. Os três achados são independentes: N1 invalida a
evidência que o gate deveria produzir; N2 e N3 invalidam o raio de mudança e a estrutura de arquivos
planejados. Nenhum deles foi contornado nesta sessão, deliberadamente — inventar uma forma
alternativa de fazer T004 "passar" mascararia a mesma classe de diagnóstico enganoso que motivou
esta feature.

Estado deixado na worktree para inspeção: `crates/farol-core/Cargo.toml` com o bump aplicado (`iced
= "0.14"`, `dev-dependencies` `iced_test`/`insta`) e `Cargo.lock` resolvido — basta rodar `cargo
build --package farol-core` para reproduzir os 5 erros da tabela de N2. Nenhum commit foi feito.

---

## D1 — decisão revisada (2026-09-01, 2ª execução): critério do gate T004 reancorado; N1/N2/N3 resolvidos

Entrada **acrescentada** ao D1 (nada acima foi reescrito), registrando a decisão do arquiteto tomada
em resposta à "Nota de execução (2026-09-01) — resultado do gate T004" e o que a execução dessa
decisão de fato produziu.

### Critério do gate T004: de "reproduzir o bug histórico" para "rodar o mecanismo real"

**Por quê**: N1 mostrou que, em `iced` 0.14, o padrão do 1º bug histórico (`Subscription::map` com
closure capturante) virou `const { check_zero_sized::<F>() }` — **erro de compilação**, não mais
`debug_assert!` de runtime. Não existe execução a observar: o `rustc` rejeita antes. O critério
original de T004 ("confirmar que o teste falha com o `debug_assert!`, reverter, confirmar que
passa") é, portanto, inexecutável — e isso é uma **boa** notícia para o produto: aquela classe de
bug deixou de poder existir num binário compilado.

**Critério revisado (o que T004 passa a provar)**: que o `iced_test::Emulator` roda
`Farol::subscription()` de verdade e leva uma conexão de plugin **real** até um estado terminal,
atravessando `Subscription` real → spawn de processo filho real (`tokio::process`) → handshake
JSON-RPC/NDJSON real por stdin/stdout → transição real em `update.rs`. É esse mecanismo — não o bug
específico de `Subscription::map` — que sustenta US1 daqui em diante.

**Achado N4 desta execução — `git-local` não pode alcançar `Ready`**: `plugins/git-local/main.py`
declara `PROTOCOL_VERSION = "0.1"` e o core fala `"0.2"`
(`plugin_worker::CORE_PROTOCOL_VERSION`). Sob o regime `MAJOR == 0` de
`ProtocolVersion::is_compatible_with` (D7 da feature 001), MINOR diferente é incompatível — logo
`git-local` termina **deterministicamente** em `Unavailable{VersionIncompatible}`. Isso é o débito
técnico #4 já registrado em `AGENTS.md` ("v0.2 ... quebra `git-local` de propósito"), fora do escopo
desta feature corrigir. Consequência para o plano: **T005 ("git-local alcança Ready") é impossível
como escrito** enquanto o débito #4 não for resolvido.

**Como o gate foi fechado mesmo assim**: dois cenários no mesmo gate, ambos passando —

1. `emulator_runs_the_real_subscription_until_a_plugin_reaches_ready` — **`uptime-kuma`** (que fala
   `"0.2"`) alcança `PluginState::Ready` pelo `Emulator`, com `required_config` resolvido pela
   fixture hermética através do mecanismo de produção (`config_store`/`secrets_store` →
   `FAROL_PLUGIN_*` injetada no spawn). É este cenário que prova o gate no sentido forte: chegar a
   `Ready` exige que todo o caminho real tenha rodado.
2. `emulator_takes_git_local_through_a_real_handshake_to_a_terminal_state` — **`git-local`** contra a
   fixture determinística de T003, terminando em `Unavailable{VersionIncompatible}` com o detalhe
   nomeando as duas versões. Percorre o mesmo caminho real e **congela o débito #4 como
   comportamento automatizado**: no dia da migração do plugin para v0.2 este teste falha e obriga a
   atualizá-lo, em vez de o débito seguir invisível.

Não-vacuidade verificada nesta sessão, sabotando cada fixture e observando a falha:
remover `api_key` do `secrets.toml` da fixture ⟹ `uptime-kuma` observado como
`Unavailable{NotConfigured}` (não `Ready`); esperar `Crashed` em vez de `VersionIncompatible` ⟹
falha reportando `Unavailable { reason: VersionIncompatible, detail: "plugin fala protocolo 0.1,
este core só suporta 0.2" }`, string montada a partir da resposta que o processo Python *de fato*
enviou.

### N2 — `Subscription::run_with_id` → `Subscription::run_with` (resolvido)

`PluginSpawnConfig` ganhou `PartialEq, Eq, Hash`; o `id` explícito
(`"{plugin_name}-{setup_attempt}"`) deu lugar a `WorkerSubscriptionKey { config, setup_attempt }`
como o `D: Hash` de `Subscription::run_with`, com `fn worker_stream(&WorkerSubscriptionKey) -> impl
Stream` como `builder`. A propriedade load-bearing de D8 da feature 002 — a identidade da
`Subscription` MUST mudar sempre que `setup_attempt` muda, para o `iced` matar o processo filho
antigo via `kill_on_drop` — passa a ser garantida pelo `#[derive(Hash)]` em vez de por interpolação
de string. Mesmo tratamento para o timer de refresh (`RefreshSubscriptionKey { plugin_name,
interval }`, `update.rs`), com uma diferença deliberada de semântica documentada no código:
`interval` agora participa da identidade (sob 0.13 não participava).
Também ajustados: `iced::stream::channel` (o closure passou a exigir anotação de tipo do `Sender`,
`E0282`) e `iced::application` (agora `(boot, update, view)` + `.title(...)` builder).

### N3 — testes `iced_test` dentro do bin target (resolvido)

`crates/farol-core` continua **sem** `src/lib.rs` (criar um obrigaria a alargar visibilidades em
`model.rs`/`update.rs`/`plugin_worker.rs`/`view.rs`, fora do raio da feature). Os testes `iced_test`
vivem em `crates/farol-core/src/e2e_tests.rs`, declarado como `#[cfg(test)] mod e2e_tests;` em
`main.rs` — exatamente o padrão dos 32 testes que o crate já tinha (`update.rs`, `config_store.rs`,
`secrets_store.rs`). A § Path Conventions de `tasks.md` foi corrigida em conformidade
(`tests/e2e_harness.rs`/`tests/visual_snapshot.rs`/`tests/support/fixtures.rs` deixam de existir
como planejados).

### T002 — `program()` parametrizado pelo `boot`

`crate::program(boot)` (em `main.rs`) é o ponto único de montagem do `Program`; `main()` passa
`Farol::default`, sem nenhuma mudança de comportamento observável de `cargo run --bin farol`. O
parâmetro existe porque `Farol::default()` deriva os plugins de `known_plugins()`, que usa caminhos
**relativos** à raiz do repo e traz os dois plugins conhecidos: sob `cargo test` o `cwd` é o
diretório do crate, e o slot `uptime-kuma` leria o `~/.config/farol` real da máquina. `update`,
`view`, `subscription` e título permanecem idênticos entre binário e teste — que é o ponto de T002.

### T003 — fixture hermética via `$XDG_CONFIG_HOME`

A fixture (`HarnessFixture`, `e2e_tests.rs`) cria um repositório git real (`git init` + um commit,
identidade/datas fixas, `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` neutralizados) e escreve
`config.toml`/`secrets.toml` sintéticos para os dois plugins sob um diretório temporário, que passa
a ser o `$XDG_CONFIG_HOME` do processo de teste — **os dois lados** (core em Rust e processo Python
do plugin, que herda o ambiente no spawn) leem dali, e nenhum teste toca o `~/.config/farol` real.
Os testes E2E são serializados por um `Mutex` porque uma variável de ambiente é global ao processo;
nenhum outro teste do crate lê `XDG_CONFIG_HOME`/`HOME`.

`base_url` da fixture aponta para `http://127.0.0.1:1` (nunca há serviço escutando: "connection
refused" imediato, sem tráfego para fora da máquina). O duplo HTTP determinístico de `/metrics`
previsto em D2 **continua pendente** — ele é necessário para assertivas sobre os *dados* do widget
(T006), não para o gate de `Ready`, que depende só do handshake + `required_config` resolvido.
