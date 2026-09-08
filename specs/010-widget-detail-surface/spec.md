# Feature Specification: Protocolo e UI — Discriminador Explícito de WidgetItems e Superfície de Detalhe de Item

**Feature Branch**: `010-widget-detail-surface`

**Created**: 2026-09-07

**Status**: Draft

**Input**: User description: "Fechar débito técnico das issues #9 (WidgetItems: desambiguação untagged depende de disjunção incidental de campos) e #10 (superfície de UI de detalhe/drill-down no core, pré-requisito para logs de container)"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Discriminador explícito elimina dependência de disjunção incidental (Priority: P1)

Como mantenedor do protocolo Farol, quero que o tipo que representa os itens de um widget (`WidgetItems`) seja desambiguado por um campo explícito no wire, não pela coincidência de que os campos obrigatórios de cada variante hoje não colidem — para que adicionar um quinto tipo de item no futuro não exija reanalisar manualmente a disjunção de campos de todas as variantes existentes.

**Why this priority**: É a causa raiz do débito — hoje a corretude da desserialização depende de uma invariante informal (nomes de campo escolhidos deliberadamente para não colidir, documentada em `research.md` seção D12) que não é garantida pelo compilador nem pelo schema.

**Independent Test**: Pode ser testado isoladamente serializando cada variante de `WidgetItems` e confirmando que a desserialização identifica a variante correta a partir do campo discriminador, mesmo com um item minimalista (array vazio ou item com só os campos obrigatórios).

**Acceptance Scenarios**:

1. **Given** um plugin envia um `widget_get_result` com itens do tipo Git, **When** o core desserializa a resposta, **Then** o item é identificado como Git pelo campo discriminador, não pela disjunção de campos.
2. **Given** um plugin envia um `widget_get_result` com `items: []` (array vazio), **When** o core desserializa a resposta, **Then** o resultado não depende de qual variante é declarada primeiro no enum (elimina a ambiguidade hoje mitigada só do lado consumidor).
3. **Given** os 4 plugins de referência (`git-local`, `uptime-kuma`, `openfortivpn-vpn`, `docker-containers`) após a migração desta feature, **When** cada um envia seu `widget_get_result`, **Then** todos continuam funcionando com o novo formato, sem regressão de comportamento visível ao usuário.

---

### User Story 2 - Superfície genérica de detalhe/drill-down de item (Priority: P2)

Como usuário do Farol, quero poder abrir uma visão de detalhe a partir de um item específico de qualquer widget (por exemplo, um container) para ver informação adicional que não cabe na linha resumida do painel principal — preparando o terreno para uma futura tela de logs de container, sem implementar os logs em si nesta feature.

**Why this priority**: Depende do discriminador da User Story 1 só parcialmente (a superfície de detalhe pode ser genérica por tipo de widget sem esperar o discriminador), mas tem prioridade menor porque é uma capacidade nova de UI, não uma correção de um risco já existente.

**Independent Test**: Pode ser testado isoladamente clicando num item de um widget já existente (ex.: um container) e verificando que uma superfície de detalhe abre mostrando informação estruturada daquele item específico, sem depender de nenhum plugin novo.

**Acceptance Scenarios**:

1. **Given** o painel principal exibindo itens de um widget, **When** o usuário interage com um item específico para pedir mais detalhe, **Then** uma superfície de detalhe abre mostrando os campos completos daquele item.
2. **Given** a superfície de detalhe aberta, **When** o usuário pede para fechá-la, **Then** o painel principal volta ao estado anterior, sem perder o estado dos outros widgets.
3. **Given** um item de um widget que não tem informação adicional além do que já é exibido na linha resumida, **When** o usuário tenta abrir o detalhe desse item, **Then** o sistema não obriga a exibir uma superfície vazia (a interação de "ver detalhe" só aparece para widgets que de fato têm mais informação a mostrar).

### Edge Cases

- O que acontece se o item selecionado para detalhe deixar de existir (ex.: container removido) entre o clique e a atualização de dados seguinte?
- Como o sistema se comporta se o usuário pedir detalhe de um item enquanto uma ação (start/stop/restart) já está em andamento nesse mesmo item?
- Como a superfície de detalhe se comporta quando o conteúdo é maior que a área visível (necessidade de rolagem, relevante para o caso futuro de logs)?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O protocolo MUST expor um campo discriminador explícito que identifique a variante de item de widget sendo transmitida, eliminando a dependência de disjunção incidental de campos obrigatórios.
- **FR-002**: A introdução do discriminador MUST seguir o padrão já usado pelo projeto para mudanças não aditivas de protocolo (bump de versão + migração coordenada dos plugins de referência), conforme praticado nas transições de versão anteriores.
- **FR-003**: O protocolo MUST adicionar um campo `kind` (string) ao nível do envelope `WidgetGetResult` (não em cada item individual), identificando a variante de `WidgetItems` transmitida, acompanhado de um bump de `PROTOCOL_VERSION`.
- **FR-004**: Os 4 plugins de referência (`git-local`, `uptime-kuma`, `openfortivpn-vpn`, `docker-containers`) MUST ser migrados para o novo formato como parte desta feature, sem quebrar seu comportamento observável.
- **FR-005**: O core MUST oferecer um mecanismo genérico (reutilizável por qualquer widget existente ou futuro) para o usuário solicitar a visualização de detalhe de um item específico.
- **FR-006**: O mecanismo de detalhe MUST ser um painel/overlay genérico exibido sobre a view existente (sem introduzir um sistema de rotas/telas no `iced`), disparado por uma mensagem genérica de pedido de detalhe e capaz de exibir conteúdo rolável.
- **FR-007**: Fechar a superfície de detalhe MUST retornar o usuário ao estado do painel principal sem perda de estado dos demais widgets.
- **FR-008**: A superfície de detalhe desta feature MUST ser demonstrada com pelo menos um tipo de item real já existente (ex.: item de container), mostrando seus campos completos — sem implementar a funcionalidade de logs de container em si.
- **FR-009**: As mudanças desta feature MUST preservar 100% dos testes hoje existentes no workspace (incluindo os testes de contrato de `widget_get_result` em `farol-protocol`), atualizados apenas onde o novo discriminador exigir ajuste explícito.

### Key Entities

- **Discriminador de WidgetItems**: campo explícito que identifica a variante de item transmitida num `widget_get_result`, substituindo a dependência de disjunção incidental de campos.
- **Superfície de Detalhe**: mecanismo genérico do core que exibe informação estruturada de um item específico de um widget, independente do tipo de widget de origem.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A adição de uma variante hipotética de `WidgetItems` no futuro não exige mais reanalisar manualmente a disjunção de campos das variantes existentes — a identificação é feita por um campo explícito, verificável por teste automatizado.
- **SC-002**: Os 4 plugins de referência continuam reportando seus widgets corretamente após a migração, sem nenhuma regressão visível na UI.
- **SC-003**: O usuário consegue abrir e fechar a superfície de detalhe de um item de container em menos de 2 interações (1 clique para abrir, 1 para fechar).
- **SC-004**: 100% dos testes hoje existentes no workspace continuam passando após a mudança (com os ajustes explícitos previstos em FR-009).

## Assumptions

- O bump de versão do protocolo necessário para introduzir o discriminador segue o mesmo padrão observado nas transições anteriores (`specs/005-docker-containers-plugin/tasks.md`, T014-T016) — migração coordenada dos plugins de referência dentro da mesma feature, não um mecanismo de compatibilidade retroativa automática.
- O mecanismo de detalhe (User Story 2) não introduz uma arquitetura de múltiplas telas/rotas na aplicação iced além do que for estritamente necessário para exibir a informação — o formato exato é decidido na fase de clarificação (Q4).
- O caso de uso futuro de logs de container (fora de escopo desta feature) exige que a superfície de detalhe suporte conteúdo rolável — isso é considerado no desenho da superfície desta feature, mesmo sem implementar logs agora.
- O campo `kind` novo em `WidgetGetResult` é adicionado como campo obrigatório junto do bump de `PROTOCOL_VERSION` — não há necessidade de suportar simultaneamente payloads com e sem `kind` na mesma versão de protocolo (consistente com o padrão de migração coordenada já usado nas transições anteriores).

## Out of Scope

- Implementação da funcionalidade de logs de container em si (streaming ou histórico de logs) — permanece para uma feature futura, agora desbloqueada pela superfície de detalhe entregue aqui.
- Qualquer mecanismo de compatibilidade retroativa automática entre versões de protocolo — migração é coordenada, não automática.
- Navegação multi-nível (detalhe de detalhe) — a superfície de detalhe desta feature é de um único nível de profundidade.
