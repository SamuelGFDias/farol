---
description: "Task list for feature 010 - discriminador WidgetItems + painel de detalhe de item"
---

# Tasks: Protocolo e UI — Discriminador Explícito de WidgetItems e Superfície de Detalhe de Item

**Input**: Design documents from `/specs/010-widget-detail-surface/`

**Prerequisites**: plan.md, spec.md

**Tests**: incluídas — mesma disciplina das features 001-009 deste projeto.

**Organization**: US1 (discriminador de protocolo, bump v0.5) primeiro e totalmente verificado,
depois US2 (painel de detalhe na UI), que parte de um `WidgetItems` já estável. Ver `plan.md` §
Complexity Tracking.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Criar `protocol/schema/v0.5/` como cópia de `protocol/schema/v0.4/`

## Phase 2: User Story 1 - Discriminador explícito de WidgetItems (P1, issue #9)

**Goal**: `WidgetGetResult` ganha campo `kind` no envelope; a desserialização de `items` passa a ser
dirigida por `kind`, eliminando a dependência de disjunção incidental de campos.

**Independent Test**: enviar um `WidgetGetResult` com `kind="Container"` e itens cujo formato seja
estruturalmente ambíguo com outro tipo — confirmar que a variante correta é escolhida mesmo assim.

- [X] T002 [US1] Definir `WidgetItemKind` (enum `Git`/`Monitor`/`Vpn`/`Container`, serializado como
      string) em `crates/farol-protocol/src/messages.rs`, espelhando os nomes de variante de
      `WidgetItems` (`messages.rs:488-497`)
- [X] T003 [US1] Adicionar campo `kind: WidgetItemKind` a `WidgetGetResult` em `messages.rs`;
      implementar `Deserialize` customizado (parsing em duas etapas via `serde_json::Value`: lê
      `kind` primeiro, depois desserializa `items` só para a variante de `WidgetItems`
      correspondente), substituindo a dependência do `#[serde(untagged)]` puro para escolha de
      variante
- [X] T004 [US1] Bump de `PROTOCOL_VERSION`/`ProtocolVersion` (`crates/farol-protocol/src/
      version.rs`) de `0.4` para `0.5`
- [X] T005 [US1] Atualizar `protocol/schema/v0.5/widget.schema.json`: substituir o `anyOf` puro de
      `v0.4` por `if`/`then` sobre o valor de `kind` selecionando o `$defs` de item aplicável,
      tornando `kind` campo obrigatório do envelope
- [X] T006 [US1] [P] Atualizar os 5 testes de `widget_get_result` em
      `crates/farol-protocol/tests/contract_schema_validation.rs` (linhas 408-699+) para incluir o
      campo `kind`, incluindo o caso de `items: []`
- [X] T007 [US1] [P] Atualizar os testes de `additionalProperties:false` em
      `crates/farol-protocol/tests/schema_boundaries.rs` (linhas 876-891) conforme o novo campo
      `kind`
- [X] T008 [US1] Adicionar teste novo em `contract_schema_validation.rs`: uma ambiguidade estrutural
      proposital entre dois tipos de item (campos idênticos) confirmando que `kind` desambigua
      corretamente a desserialização mesmo assim
- [X] T009 [US1] [P] Migrar `plugins/git-local/main.py`: emitir campo `kind="Git"` em
      `WidgetGetResult` e declarar `protocol_version = "0.5"` no handshake
- [X] T010 [US1] [P] Migrar `plugins/uptime-kuma/main.py`: emitir campo `kind="Monitor"` e
      `protocol_version = "0.5"`
- [X] T011 [US1] [P] Migrar `plugins/openfortivpn-vpn/main.py`: emitir campo `kind="Vpn"` e
      `protocol_version = "0.5"`
- [X] T012 [US1] [P] Migrar `plugins/docker-containers/main.py` (ou caminho equivalente do plugin de
      containers): emitir campo `kind="Container"` e `protocol_version = "0.5"`
- [X] T013 [US1] Atualizar os testes Python de cada um dos 4 plugins de referência (`tests/unit/`)
      para validar que o `kind` correto é emitido
- [X] T014 [US1] Rodar `cargo test --workspace` (foco em `farol-protocol`) e a suíte Python
      (`pytest`/`ruff` conforme já configurado) — confirmar que os 4 plugins de referência e os
      testes de contrato passam com o novo campo, sem regressão

**Checkpoint**: US1 entregue e verificado (issue #9 fechada) — `WidgetItems` está estável para US2
construir em cima.

## Phase 3: User Story 2 - Painel de detalhe genérico sobre a view atual (P2, issue #10)

**Goal**: clicar num item de widget (demonstrado com item de container) abre um painel/overlay
genérico sobre a view atual, sem introduzir um sistema de rotas/telas novo no `iced`.

**Independent Test**: clicar num item de container na UI do Farol abre um painel mostrando os
detalhes desse item; fechar o painel volta à view normal, sem perder o estado da lista.

- [ ] T015 [US2] Estender `crates/farol-core/src/model.rs`: novo campo `detail_panel:
      Option<DetailPanelState>` no `Model`, guardando o item selecionado (ou uma referência/índice
      suficiente para renderizar o painel)
- [ ] T016 [US2] Estender `crates/farol-core/src/main.rs`: novas variantes de `Message` (ex.
      `ItemDetailRequested{...}`, `ItemDetailClosed`); em `update.rs`, roteamento que abre/fecha
      `detail_panel` no `Model`
- [ ] T017 [US2] Implementar `format_item_detail(&WidgetItems) -> Vec<(String, String)>` (par
      label/valor) em `crates/farol-core/src/view.rs` (ou `model.rs`), genérico o suficiente para
      qualquer tipo de item — evita duplicar a lógica de abertura do painel por tipo de widget
- [ ] T018 [US2] Implementar `view_detail_panel` em `view.rs`, consumindo T017; a `view()` principal
      passa a usar `iced::widget::stack![view_main(...), view_detail_panel(...)]` quando
      `detail_panel.is_some()` (confirmar a API exata de `stack` na versão 0.14 do `iced` antes de
      implementar, ver `plan.md` § Complexity Tracking)
- [ ] T019 [US2] Adicionar o gatilho de abertura do painel em `view_container_row`
      (`view.rs:490-533`), reaproveitando o padrão visual de
      `view_container_action_control`(`view.rs:539-555`) mas disparando
      `Message::ItemDetailRequested` em vez de `Message::ActionInvokeRequested`
- [ ] T020 [US2] Teste de integração real (`iced_test::Emulator`, mesmo padrão de `e2e_tests.rs`):
      clicar num item de container abre o painel de detalhe com o conteúdo esperado; fechar o
      painel restaura a view normal sem perder o estado da lista de containers

**Checkpoint**: US2 entregue (issue #10 fechada). Com T001-T020 completas, as duas issues desta
feature estão fechadas.
