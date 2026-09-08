# Implementation Plan: Protocolo e UI — Discriminador Explícito de WidgetItems e Superfície de Detalhe de Item

**Branch**: `010-widget-detail-surface` | **Date**: 2026-09-07 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/010-widget-detail-surface/spec.md`

## Summary

Fecha o débito técnico das issues #9 e #10 (feature 005). `WidgetGetResult` ganha um campo `kind`
explícito no envelope (bump de `PROTOCOL_VERSION` de `0.4` para `0.5`, migração dos 4 plugins de
referência), eliminando a dependência de disjunção incidental de campos para desserializar
`WidgetItems`. O core ganha um mecanismo genérico de painel de detalhe (overlay sobre a view atual,
via `iced::widget::stack`), demonstrado com o item de container — preparando o terreno para uma
futura feature de logs de container, sem implementá-la aqui.

## Technical Context

**Language/Version**: Rust (workspace já em uso); JSON Schema (novo diretório de versão
`protocol/schema/v0.5/`).

**Primary Dependencies**: nenhuma dependência Rust nova. `iced::widget::stack` (já disponível na
versão 0.14 do `iced` já em uso, `Cargo.toml:13`) para o overlay de detalhe.

**Storage**: nenhuma mudança.

**Testing**: `crates/farol-protocol/tests/contract_schema_validation.rs` e `schema_boundaries.rs`
MUST ser atualizados para incluir o novo campo `kind` em cada teste de `widget_get_result` (5 testes
identificados na investigação, incluindo o caso de `items: []`). Novo teste garantindo que a
desserialização de `WidgetGetResult` usa o campo `kind` para escolher a variante de `WidgetItems`
correta, não mais a disjunção de campos (incluindo um teste que force uma ambiguidade estrutural
proposital entre dois tipos de item e confirme que `kind` desambigua corretamente mesmo assim).
Testes Python dos 4 plugins de referência (`tests/unit/`) MUST validar que cada plugin agora emite o
campo `kind` correto.

**Target Platform**: Linux + aplicação desktop `iced`.

**Constraints**: FR-002 exige que a introdução do discriminador siga o padrão de bump de versão +
migração coordenada já praticado (não compatibilidade retroativa automática) — os 4 plugins de
referência MUST ser migrados dentro desta mesma feature, nunca deixados no formato antigo.

**Scale/Scope**: User Story 1 (discriminador de protocolo) e User Story 2 (painel de detalhe) são
razoavelmente independentes — US2 não depende tecnicamente de US1 estar pronta (o painel de detalhe
funciona sobre o `WidgetItems` já desserializado, independente de como ele foi discriminado), mas
ambas tocam `farol-protocol`/`farol-core` e MUST ser sequenciadas para evitar conflito de arquivo
(ver Complexity Tracking).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- Mudança de protocolo NÃO aditiva (bump de `PROTOCOL_VERSION`) — MUST seguir a disciplina de
  compatibilidade já estabelecida pelo protocolo Farol (rejeitar handshake com versão incompatível,
  `messages.rs:239`) e migrar coordenadamente os 4 plugins de referência, sem deixar nenhum no
  formato antigo. PASS, com atenção: esta é a primeira vez que um bump de protocolo é motivado por
  débito técnico interno (não por uma capability nova) — documentar isso no `CHANGELOG`/`AGENTS.md`
  ao final.
- Painel de detalhe (US2) é aditivo à UI existente, sem remover nenhuma interação hoje disponível
  (start/stop/restart de container continuam funcionando como hoje). PASS.

## Project Structure

### Documentação desta feature

```
specs/010-widget-detail-surface/
├── spec.md
├── plan.md              # este arquivo
├── checklists/requirements.md
└── tasks.md             # gerado a seguir
```

### Código afetado

```
protocol/schema/v0.5/                         # NOVO diretório, cópia de v0.4 + campo `kind`
                                               # obrigatório em WidgetGetResult
crates/farol-protocol/src/
├── messages.rs           # ESTENDER `WidgetGetResult`: novo campo `kind: WidgetItemKind` (enum
│                          # `Git`/`Monitor`/`Vpn`/`Container`, serializado como string). Custom
│                          # `Deserialize` (ou parsing em duas etapas via `serde_json::Value`) que lê
│                          # `kind` primeiro e desserializa `items` para a variante correspondente de
│                          # `WidgetItems`, em vez de depender do `#[serde(untagged)]` puro
├── version.rs             # Bump da constante de versão corrente usada pelos plugins de referência
                            # (`ProtocolVersion::new(0, 5)`, ajustar o valor central se existir)
crates/farol-protocol/tests/
├── contract_schema_validation.rs   # ATUALIZAR os 5 testes de widget_get_result para incluir `kind`
└── schema_boundaries.rs            # ATUALIZAR testes de `additionalProperties:false` conforme novo
                                     # campo
plugins/{git-local,uptime-kuma,openfortivpn-vpn,docker-containers}/main.py
                            # ATUALIZAR cada plugin para emitir `kind` correto e declarar
                            # `protocol_version = "0.5"` no handshake
crates/farol-core/src/
├── main.rs                # NOVA `Message`: ex. `ItemDetailRequested{...}` / `ItemDetailClosed`
├── model.rs                # NOVO estado: `detail_panel: Option<DetailPanelState>` no `Model`
├── update.rs                # Roteamento das novas mensagens: abre/fecha `detail_panel`
└── view.rs                  # NOVA `view_detail_panel`; `view()` principal passa a usar
                              # `iced::widget::stack![view_main(...), view_detail_panel(...)]`
                              # quando `detail_panel.is_some()`; adicionar o gatilho de abertura
                              # (`on_press`) pelo menos na linha de container (`view_container_row`,
                              # `view.rs:490-533`)
```

## Key Design Decisions (Phase 0+1 consolidado)

- **D1 (US1)**: `kind` fica no envelope `WidgetGetResult` (não em cada item), com um enum fechado
  `WidgetItemKind` espelhando as 4 variantes atuais de `WidgetItems` — nomes de variante idênticos
  (`Git`/`Monitor`/`Vpn`/`Container`) para minimizar a chance de o valor do campo divergir da
  variante real escolhida na desserialização.
- **D2 (US1)**: a validação de consistência entre `kind` e o conteúdo real de `items` é feita no
  próprio parsing (a desserialização de `items` é dirigida por `kind`, não uma checagem posterior) —
  elimina por construção o caso de `kind` mentir sobre o conteúdo.
- **D3 (US1)**: o schema JSON Schema de `v0.5` usa `if`/`then` sobre o valor de `kind` para
  selecionar qual `$defs` de item se aplica a `items`, substituindo o `anyOf` puro de `v0.4`.
- **D4 (US2)**: o painel de detalhe é genérico o suficiente para renderizar qualquer struct de item
  (via uma função de formatação por tipo, ex. `format_item_detail(&WidgetItems) -> Vec<(String,
  String)>` de par label/valor) — evita duplicar a lógica de "abrir painel" por tipo de widget,
  mesmo que o conteúdo exibido varie.
- **D5 (US2)**: o gatilho de abertura reaproveita o padrão visual de botão já usado para ações
  (`view_container_action_control`, `view.rs:539-555`), mas dispara `Message::ItemDetailRequested`
  em vez de `Message::ActionInvokeRequested` — não é uma "ação" que muda estado do plugin, é
  puramente uma interação de UI local.

## Complexity Tracking

| Risco | Mitigação |
|---|---|
| Bump de `PROTOCOL_VERSION` quebra compatibilidade com qualquer plugin de terceiro instalado via feature 007/008 que ainda esteja em `0.4` | Aceito conscientemente — mensagem de erro de incompatibilidade de handshake já existe (`messages.rs:239`) e é clara; documentar no changelog que plugins de terceiro em `0.4` precisam ser atualizados |
| `iced::widget::stack` pode ter uma API ligeiramente diferente da assumida aqui | Executor de US2 MUST confirmar a API exata na versão 0.14 antes de implementar, consultando a documentação/exemplos oficiais do `iced`, não assumir a assinatura de cabeça |
| US1 e US2 tocam arquivos sobrepostos (`view.rs` para o gatilho, mas não para o parsing) | Sequenciar: US1 (protocolo) primeiro e totalmente verificado (testes de contrato passando), depois US2 (UI), para que US2 já parta de um `WidgetItems` estável |
