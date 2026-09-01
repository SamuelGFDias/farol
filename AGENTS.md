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

`.specify/memory/constitution.md` é normativo e versionado (SemVer próprio, atualmente v1.0.0).
Mudança de princípio exige emenda formal (skill `speckit-constitution`) — não editar a constitution
diretamente fora desse processo.

## Arquitetura do core (`crates/farol-core`, binário `farol`)

Padrão Elm/Model-Update-View via `iced 0.13`. Um `cargo run --bin farol` (ou o binário
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

`Subscription::map` faz `debug_assert!(size_of::<F>() == 0, ...)` — um closure `move |x| ...` que
captura qualquer variável do ambiente **panica em runtime** assim que a `Subscription` é montada
(mensagem: `"the closure ... is capturing"`), e isso **não aparece em nenhum teste unitário** (eles
testam `update.rs` chamando `handle_worker_event` diretamente, nunca `Farol::subscription()` de
verdade rodando sob o runtime `iced`). Só um `cargo run` real revela o panic — motivo pelo qual
`tasks.md` da feature 002 tem uma task dedicada (T023) só para rodar o binário de verdade antes de
seguir para as próximas fases.

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

- `cargo test --workspace` — 71 testes (unit `farol-core` + unit/contract `farol-protocol`, este
  último validando (de)serialização contra os JSON Schemas via `jsonschema` crate).
- `cargo clippy --workspace --all-targets` — deve ficar limpo, sem warning nenhum.
- `tests/integration/` e `tests/contract/` (raiz do repo, fora de qualquer crate) são **só
  documentação** — `cargo test` nunca os descobre (Cargo só compila `tests/*.rs` dentro de cada
  crate). `tests/integration/README.md` referencia um `harness.sh` que nunca chegou a ser
  construído — cenários de `quickstart.md` são validados manualmente rodando o binário de verdade
  (ver task T023 da feature 002 para um exemplo de execução real + achado).
- Rodar o binário `farol` manualmente para validar um cenário de `quickstart.md`: precisa de um
  `DISPLAY` X11 funcional. Sob Xvfb sem WM/GPU real a janela não renderiza visualmente (fica preta),
  mas o ciclo `update`/handshake/transição de estado roda normalmente e pode ser observado via
  `eprintln!` de diagnóstico temporário (remover depois) — suficiente para validar comportamento de
  protocolo sem depender de captura de tela.

## Python (plugins)

`plugins/*/pyproject.toml` configura `ruff` (lint). Rodar `ruff check` dentro do diretório do
plugin antes de considerar uma mudança Python pronta.
