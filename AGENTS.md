# AGENTS.md — contexto do projeto Farol

Referência para trabalho futuro nesta base — arquitetura, convenções e decisões que não são óbvias
só lendo o código. Mantido atualizado a cada mudança relevante (ver `CLAUDE.md` global do usuário).

## O que é

Farol é um "plano de controle pessoal" nativo Linux (GUI, sem navegador) que agrega **Ver / Agir /
Lembrar** sobre o estado técnico do dev (serviços monitorados, repos git, VPN, issues...) através de
plugins como processos separados. Visão completa em `README.md`; princípios formais e imutáveis em
`.specify/memory/constitution.md` (7 princípios: Nativo sem navegador, Plugins isolados via
JSON-RPC, Widgets declarativos, Permissões explícitas por manifesto, Espaços por contexto, Paleta
de comandos, Registry federado).

## Metodologia: spec-driven development (spec-kit)

O projeto usa o workflow **speckit** (skills `speckit-*`, diretório `.specify/`). Cada feature vive
em `specs/NNN-nome-da-feature/` com `spec.md` (requisitos), `plan.md` (design), `research.md`
(decisões `D1`, `D2`...), `data-model.md`, `contracts/`, `quickstart.md` (cenários de validação
manual) e `tasks.md` (lista de tasks `TNNN` com checkbox `[ ]`/`[X]`, dependências explícitas e
notas de execução). **Antes de implementar qualquer coisa numa feature, ler o `tasks.md`
correspondente** — ele é a fonte de verdade de "o que falta" e de por que cada task existe.

Features existentes:
- `specs/001-walking-skeleton-git-plugin/` — protocolo v0.1, plugin `git-local` de referência.
- `specs/002-uptime-kuma-plugin/` — protocolo v0.2 (bump deliberado, quebra `git-local` de
  propósito — débito técnico #4 rastreado como issue, migração fora de escopo desta feature),
  arquitetura multi-plugin, config/secrets geridos pelo core, plugin `uptime-kuma`.
- `specs/003-automated-testing-infrastructure/` — harness de testes em duas camadas (ver § Testes).
- `specs/004-vpn-status-plugin/` — em planejamento (`plan.md`/`research.md`/`data-model.md`/
  `contracts/`/`quickstart.md` prontos, `tasks.md` ainda não gerado). Plugin `openfortivpn-vpn`,
  envolvendo a CLI `openfortivpn-gui status|connect|disconnect --json` (contrato em
  `../openfortivpn-gui/specs/001-add-cli-interface/contracts/`, projeto irmão fora deste repo).
  Bump de protocolo `0.2` → `0.3` **aditivo** (novo `kind` de widget `"vpn-status"`,
  `ActionInvokeResult` generalizado para `oneOf`) — mas como a série `0.x` exige igualdade exata de
  versão (`ProtocolVersion::is_compatible_with`), `git-local`/`uptime-kuma` também precisam migrar
  a constante `PROTOCOL_VERSION` para `"0.3"` dentro desta mesma feature (decisão D2 de
  `research.md`, para não repetir o padrão de dívida técnica da migração `0.1→0.2`, débito #4).

`.specify/memory/constitution.md` é normativo e versionado (SemVer próprio, atualmente v1.0.0).
Mudança de princípio exige emenda formal (skill `speckit-constitution`) — não editar a constitution
diretamente fora desse processo.

## Arquitetura do core (`crates/farol-core`, binário `farol`)

Padrão Elm/Model-Update-View via `iced 0.14`. Um `cargo run --bin farol` (ou o binário
`target/debug/farol`) **precisa rodar com cwd = raiz do repo** — `plugin_worker::known_plugins()`
usa caminhos relativos (`plugins/git-local/main.py`, `plugins/uptime-kuma/main.py`) para o
`command`/`args` de cada plugin spawnado.

- `main.rs` — entrypoint `iced::application`; `Farol { plugins: Vec<PluginSlot> }`, uma entrada por
  plugin conhecido (registro fixo em `plugin_worker::known_plugins()`), cada uma com seu próprio
  `PluginConnection` e canal de worker.
- `model.rs` — tipos de estado puro: `PluginState` (`Starting → Handshaking → Ready`, ou
  `Unavailable{reason}` — `FailedToStart`/`VersionIncompatible`/`Crashed`/`Unresponsive` são
  terminais; `NotConfigured` é a **única exceção não-terminal**, com caminho de volta via
  `SetupForm` + `Message::SetupSubmitted`, que incrementa `PluginConnection::setup_attempt` e força
  a reconexão do worker — feature 002, T029-T035, completo).
- `plugin_worker.rs` — spawn do processo filho, I/O assíncrona via `iced::stream::channel` +
  `tokio::process`, handshake, ciclo `widget/get`/`action/invoke`. Uma `Subscription` por plugin via
  `Subscription::run_with_id("{plugin_name}-{setup_attempt}", stream)` — o `setup_attempt` no `id`
  é o que faz o `iced` encerrar o processo filho antigo (`kill_on_drop`) e iniciar um novo quando a
  tela de setup é submetida (`Subscription::run` simples não aceita capturar `config` nem esse
  contador).
- `update.rs` — transições `Message → Farol`. `subscription()` monta uma `Subscription` por plugin
  (worker) + uma `Subscription` de timer de refresh por conexão `Ready` (`refresh_tick_stream`, ver
  armadilha abaixo — **não** usa `iced::time::every` diretamente).
- `view.rs` — renderização condicionada a `PluginState`, incluindo `view_setup_form` (tela de setup,
  `TextInput`/botão ligados a `Message::SetupFieldChanged`/`SetupSubmitted`) quando
  `Unavailable{NotConfigured}`.
- `config_store.rs`/`secrets_store.rs` — leitura/escrita de
  `~/.config/farol/plugins/<nome>/config.toml` e `~/.config/farol/secrets.toml`, injetados no spawn
  do plugin como variável de ambiente `FAROL_PLUGIN_<NOME>_<CAMPO>` (maiúsculo, `-`→`_`) — **não**
  system keyring (ver nota de decisão abaixo). Escrita (`save_plugin_config`/`save_plugin_secrets`)
  chamada por `Farol::handle_setup_submitted` (update.rs) ao confirmar a tela de setup.

### Armadilha real já corrigida: `iced::Subscription::map` exige closure não-capturante

Em `iced 0.13`, `Subscription::map` fazia `debug_assert!(size_of::<F>() == 0, ...)` — um closure
`move |x| ...` que captura qualquer variável do ambiente **panicava em runtime** assim que a
`Subscription` era montada (mensagem: `"the closure ... is capturing"`), e isso não aparecia em
nenhum teste unitário (eles testavam `update.rs` chamando `handle_worker_event` diretamente, nunca
`Farol::subscription()` de verdade rodando sob o runtime `iced`). Só um `cargo run` real revelava o
panic — motivo pelo qual `tasks.md` da feature 002 tem uma task dedicada (T023) só para rodar o
binário de verdade antes de seguir para as próximas fases.

**Sob `iced 0.14` (feature 003) essa classe de bug virou erro de compilação**: o mesmo check hoje é
`const { check_zero_sized::<F>() }` dentro de `Subscription::map`, avaliado em tempo de compilação
— não existe mais binário compilável com um closure capturante nesse ponto, então não há mais nada
a "revelar em runtime" para esse caso específico. O relato histórico abaixo (dois pontos distintos
de `Farol::subscription()`, cada um só alcançável sob uma condição de runtime diferente) continua
valendo como lição sobre cobertura de teste — só o mecanismo de detecção mudou de `debug_assert!`
de runtime para erro `E0080` de compilação. Achado completo (N1/N2/N3) e a regressão deliberada que
comprova o `E0080` atual: `specs/003-automated-testing-infrastructure/research.md` D1 (entrada de
2026-09-01) e `specs/003-automated-testing-infrastructure/quickstart.md` § Cenário 1.

Padrão correto quando um valor por-plugin (como `plugin_name`) precisa ir dentro da `Message`
produzida por uma `Subscription`: embuti-lo no **stream** via `futures::StreamExt::map`/dentro de um
`iced::stream::channel` (sem essa restrição, por não passar pelo `Subscription::map` do `iced`)
antes de envolver em `Subscription`, e só então usar `Subscription::map` com um closure que só usa
seu próprio parâmetro (zero-sized), ou nem usar `.map()` nenhum se o stream já produz o tipo final
diretamente. Ver `plugin_worker::subscription` (devolve `Subscription<(String, WorkerEvent)>`) e
`update.rs::subscription` (`.map(|(plugin_name, event)| Message::Worker { plugin_name, event })`)
para o padrão de referência.

**Esse bug apareceu duas vezes na mesma feature (002), em dois pontos diferentes de
`Farol::subscription()`** — cada um só alcançável sob uma condição de runtime distinta que nenhum
teste unitário nem execução manual anterior tinha exercitado:
1. A `Subscription` do worker de cada plugin (`.map(move |event| Message::Worker { plugin_name:
   worker_plugin_name.clone(), event })`) — pega em **qualquer** execução real, corrigido durante a
   task T023.
2. O timer de refresh periódico (`iced::time::every(interval).map(move |_instant| Message::
   RefreshTick { plugin_name: tick_plugin_name.clone() })`), dentro de `if slot.connection.state ==
   PluginState::Ready { ... }` — só é alcançado quando **algum plugin de fato chega a `Ready`**, o
   que só passou a acontecer depois que a feature 002 implementou o handshake/widget real do
   `uptime-kuma` (T024-T035); corrigido substituindo `iced::time::every(...).map(...)` por um stream
   próprio (`refresh_tick_stream`, via `iced::stream::channel` + `tokio::time::interval`, primeiro
   tick descartado para não duplicar o "fetch imediato ao ficar Ready" já existente).

**Lição para revisão de código nesta base**: `grep -n "Subscription::map\|\.map(move \|"` em
`update.rs`/`plugin_worker.rs` não basta como checklist estático — qualquer `Subscription` nova
precisa ser exercitada de verdade (`cargo run`, não só `cargo test`) sob a condição de runtime que a
constrói, não só revisada por leitura. Um teste unitário que chama `Farol::subscription()`
diretamente só prova algo se o `Farol` de teste estiver no estado (`PluginState`, contadores, etc.)
que ativa o branch em questão — `Farol::default()` não ativa nenhum dos dois casos acima.

## Protocolo (`crates/farol-protocol`, `protocol/`)

JSON-RPC 2.0 sobre NDJSON em stdin/stdout (mesmo modelo do LSP/MCP) — core sempre inicia a
requisição, plugin nunca escreve nada antes de responder `handshake/hello`. `protocol/SPEC.md` é a
especificação normativa; `protocol/schema/v0.{1,2}/*.schema.json` são os JSON Schemas
correspondentes, versionados lado a lado (v0.1 histórico, mantido para o cenário deliberado de
incompatibilidade). Versão atual: `"0.2"`. Comparação de versão é `MAJOR.MINOR` (D7 de
`research.md` da feature 001) — `crates/farol-protocol/src/version.rs`.

Plugins de referência (`plugins/`) são implementações Python **independentes**, que não importam
`farol-protocol` — leem só a spec/schema/contracts, para provar que o protocolo é genuinamente
agnóstico de linguagem (`git-local`: stdlib só, `uptime-kuma`: idem, ver docstring de
`plugins/uptime-kuma/main.py`).

## Decisão de arquitetura: segredos geridos pelo core, não system keyring

A constitution original (v0.3.0, Princípio IV) exigia keyring do sistema para segredos de plugin.
Emendada para v1.0.0 (2026-09-01) após auditoria da feature 002: segredos agora são geridos pelo
**core** (`secrets_store.rs`, arquivo `~/.config/farol/secrets.toml` com permissão `0600`) e
injetados como variável de ambiente no spawn — não há dependência de keyring do sistema (D8 de
`specs/002-uptime-kuma-plugin/research.md`). Ver `git log` da emenda para o raciocínio completo.

## Testes

- `cargo test --workspace` — 99 testes passando (0 `#[ignore]`d): unit/e2e/snapshot de `farol-core`
  (47) + contrato/unit de `farol-protocol` (21 `contract_schema_validation` + 13 `schema_boundaries`
  + 18 unit).
- `cargo clippy --workspace --all-targets` — deve ficar limpo, sem warning nenhum.
- Harness de execução real em **duas camadas** (feature 003, `research.md` D1/D5,
  `contracts/e2e-harness-contract.md`) — a mesma máquina de estados real (`Program`/`Subscription`/
  handshake), verificada de duas formas complementares:
  - **Camada 1** — in-process, sem display, via `iced_test::Emulator`:
    `crates/farol-core/src/e2e_tests.rs` (módulo `#[cfg(test)]` dentro do bin — `farol-core` não tem
    target `lib`, então não existe `--test e2e_harness`; rodar com `cargo test --package farol-core
    e2e_tests`). `uptime-kuma` alcança `Ready` e popula `monitor-status-grid`
    (`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`); `git-local`, migrado para o
    protocolo `"0.2"` (débito técnico #4, resolvido — commit `9d2fe77`, issue #4 fechada), percorre
    um handshake real até `Ready` como qualquer outro plugin
    (`emulator_takes_git_local_through_a_real_handshake_to_ready`) — deixou de terminar em
    `Unavailable{VersionIncompatible}`. Timeouts de 30s/120s por cenário (`## Clarifications` do
    `spec.md`).
    Mais 7 cenários automatizados na mesma sessão (T036-T042 da feature 002, commit `dbae06c`): tela
    de setup preenchida via UI (`Selector`/click/type do `iced_test`) leva a `Ready` com dados reais
    e persiste após reabertura
    (`setup_form_filled_via_ui_reaches_ready_with_real_data_and_persists_across_restart`); tela de
    setup não preenchida fica `NotConfigured` (`setup_form_left_unfilled_stays_not_configured`);
    `base_url` inválido gera `metrics_unreachable` sem sair de `Ready`
    (`uptime_kuma_reports_metrics_unreachable_for_an_invalid_base_url_but_stays_ready`); recuperação
    após a instância ficar inacessível e voltar, sem reiniciar o Farol
    (`uptime_kuma_recovers_after_instance_becomes_unreachable_without_restarting_farol`); resposta
    `/metrics` não reconhecível como Uptime Kuma vira `metrics_parse_error` sem derrubar o core
    (`uptime_kuma_reports_metrics_parse_error_for_a_non_metrics_response_without_crashing`); processo
    do plugin morto via `kill -9` vira `Crashed` sem derrubar o core
    (`uptime_kuma_process_killed_becomes_crashed_without_taking_down_the_core`); processo travado via
    `kill -STOP` vira `Unresponsive` sem derrubar o core
    (`uptime_kuma_process_frozen_becomes_unresponsive_without_taking_down_the_core`). Um oitavo
    cenário (T039, Cenário 6 de `quickstart.md` — instância acessível sem nenhum monitor cadastrado,
    `uptime_kuma_widget_reports_empty_items_when_instance_has_no_monitors`) não é mais `#[ignore]`d
    (débito técnico #5, issue #7 fechada, `specs/002-uptime-kuma-plugin/tasks.md` T051, 2026-09-02) —
    dois bugs reais corrigidos: (1) `plugins/uptime-kuma/metrics_parser.py::parse_metrics` não
    distinguia "zero monitores" de "resposta inválida" (corrigido reconhecendo a declaração `# HELP`/
    `# TYPE monitor_status` como sinal de instância real, mesmo sem amostras); (2) achado só depois
    de corrigir (1) — `farol_protocol::messages::WidgetItems` (`#[serde(untagged)]`) desserializa um
    `items: []` sempre como a primeira variante (`Git`), mesmo vindo de `monitor-status-grid`,
    fazendo `handle_widget_outcome` (`crates/farol-core/src/update.rs`) rotear o sucesso vazio para o
    campo errado de `PluginConnection` e nunca limpar `monitor_widget.last_error` — corrigido com
    `update::normalize_widget_items`, que usa o `kind` já conhecido do `widget_id` para resolver só o
    caso ambíguo (array vazio).
  - **Camada 2** — smoke do binário `farol` real (`fn main()`, backend de janela winit de verdade),
    via `tests/integration/harness.sh` — precisa de `xvfb` (`Xvfb`). Confirma 5 condições (subir e
    sobreviver, `uptime-kuma` chega a `Ready`, `git-local` chega a `Ready` — mesma migração para
    `"0.2"` acima, débito técnico #4 resolvido —, encerra em `SIGTERM`, nenhum processo remanescente)
    e imprime `SUCESSO — 5/5 condições confirmadas em Ns` ou `[FALHA] ...` apontando a condição que
    caiu, saída `0`/`1`. `tests/integration/README.md` documenta o contrato; o script em si é a
    Camada 2, não mais um stub.
  - Sob `iced 0.14`, um closure capturante em `Subscription::map` (a armadilha histórica acima) não
    chega a rodar — vira erro `E0080` de compilação, apanhado por `cargo test`/`cargo clippy
    --all-targets` (código de teste) ou já no primeiro passo do `harness.sh` (código de produção).
- Gerador de casos de borda de contrato — `crates/farol-protocol/tests/schema_boundaries.rs`,
  `numeric_and_null_boundary_cases()`: deriva de cada JSON Schema (`protocol/schema/v0.2/*`) os
  valores de fronteira que o schema permite mas a implementação Rust pode rejeitar (`Option<T>` vs.
  `"type": [..., "null"]`, ausência de `minimum`/`maximum`, etc.), sem hardcodar caso por caso —
  `contracts/contract-boundary-testing.md` é o contrato normativo. `MonitorStatusItem.response_time_ms`
  (`widget.schema.json`) costumava permitir `-1` (sem `minimum`) enquanto `Option<u32>`
  (`messages.rs`) rejeitava — débito técnico que ficava provado por um teste `#[ignore]`d, rastreado
  como issue #5. Fechado declarando `minimum: 0` no schema (não alterando o tipo Rust — nenhum
  plugin real precisa de valor negativo, e o Uptime Kuma já converte seu sentinela `-1` para `null`
  do lado Python em `metrics_parser.py`); o teste foi reescrito como
  `widget_monitor_status_item_response_time_ms_minimum_boundaries` (sem `#[ignore]`), no mesmo padrão
  de `widget_remote_status_tracked_ahead_and_behind_minimum_boundaries` para um campo com `minimum`
  declarado.
- Verificação visual declarativa — `crates/farol-core/src/visual_snapshot_tests.rs` +
  `crates/farol-core/src/snapshots/*.snap` (via `insta`, `cargo test --package farol-core
  visual_snapshot_tests`): compara `extract_visible_text(app.view())` contra um snapshot textual por
  `screen_id` (`data-model.md` §3, extensível por FR-013 de `visual-snapshot-contract.md`) para os
  quatro estados cobertos — `DashboardReady`, `SetupForm`, `VersionIncompatible`
  (`dashboard_ready_state`/`setup_form_state`/`version_incompatible_state`) e, desde T045
  (`specs/002-uptime-kuma-plugin/tasks.md`, 2026-09-02), `MonitorWidgetError`
  (`monitor_widget_error_state`) — erro pontual do widget `monitor-status-grid`
  (`monitor_widget.last_error`) com uma lista de uma leitura anterior preservada, distinto tanto de
  "0 monitores, sem erro" quanto de "lista populada, sem erro". Revisar/aceitar um snapshot alterado
  intencionalmente: `cargo insta review` (requer `cargo install cargo-insta`, não é pré-requisito pra
  rodar a suíte).
- `tests/contract/` (raiz do repo, fora de qualquer crate) permanece só documentação — `cargo test`
  não o descobre (Cargo só compila `tests/*.rs` dentro de cada crate).
- Validação manual via `eprintln!` de diagnóstico temporário (usada até a feature 002) foi
  substituída pela infraestrutura acima — não é mais o caminho recomendado para validar um cenário
  de `quickstart.md`. Rodar o binário `farol` manualmente (Camada 2 ou fora do harness) ainda precisa
  de um `DISPLAY` X11 funcional; sob Xvfb sem WM/GPU real a janela não renderiza visualmente (fica
  preta), mas o ciclo `update`/handshake/transição de estado roda normalmente — é isso que
  `harness.sh` observa sem instrumentar código de produção (ver cabeçalho do script para o método).

## Python (plugins)

`plugins/*/pyproject.toml` configura `ruff` (lint). Rodar `ruff check` dentro do diretório do
plugin antes de considerar uma mudança Python pronta.

- `git-local` (feature 001): testes em `tests/unit/test_git_local_scan.py` (raiz do repo,
  `unittest`, ver `tests/unit/README.md`) — repositórios git reais em diretórios temporários, não
  mocks de subprocess.
- `uptime-kuma` (feature 002): testes colocados junto do código
  (`plugins/uptime-kuma/test_*.py`, `unittest`, D7 de `research.md`), **não** em `tests/unit/` —
  divergência de layout pré-existente entre as duas features, não corrigida (mover exigiria tocar um
  arquivo fora do escopo de quem a criou; ver a nota de T046 em
  `specs/002-uptime-kuma-plugin/tasks.md`). Rodar tudo junto: `python3 -m unittest discover -p
  "test_*.py"` dentro de `plugins/uptime-kuma/` (21 testes) — `test_metrics_parser.py` (parsing
  Prometheus, mapeamento de status FR-012), `test_poller.py` (cache/erro do poller,
  `metrics_client.fetch_metrics` mockado via `unittest.mock`), `test_config.py`/`test_secrets.py`
  (leitura de `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL`/`_API_KEY` via `unittest.mock.patch.dict(os.environ,
  ...)`, nunca segredo real).
