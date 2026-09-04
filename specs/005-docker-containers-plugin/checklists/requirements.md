# Specification Quality Checklist: Plugin de Containers Docker

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-09-03
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

- Items marked incomplete require spec updates before `/speckit-clarify` or `/speckit-plan`.

### Clarificações resolvidas em 2026-09-03

**Modo de trabalho desta sessão**: por instrução explícita do usuário, o ciclo
`/speckit-specify` → `/speckit-clarify` → `/speckit-plan` → `/speckit-tasks` da feature 005 foi
conduzido em **modo silencioso**. Cada uma das clarificações abaixo foi **resolvida pelo
orquestrador sem input do usuário, conforme instrução da sessão**, tomando como base (a) o
precedente já estabelecido pelas features 001-004 deste repositório, (b) convenções razoáveis do
domínio Docker e (c) o escopo já sinalizado no `README.md` (§ "Integrações previstas" e a tabela
Ver/Agir/Lembrar). Registradas aqui, no mesmo lugar e formato que
`specs/004-vpn-status-plugin/checklists/requirements.md` usa, para que a autoria das decisões fique
rastreável no histórico do repositório.

| # | Questão | Resolução | Base da decisão |
|---|---|---|---|
| C1 | "logs" é escopo desta primeira versão (o `README.md` cita "Docker — containers up/down, logs") ou fica para depois? | **Fora de Escopo** desta feature, com justificativa registrada em `spec.md` § Out of Scope e issue #10. | Exibir N linhas de log exige uma superfície de UI de detalhe/drill-down que o core não tem — hoje todo widget renderiza uma lista plana e nenhum tem conceito de "abrir um item". É uma decisão de arquitetura do **core**, reutilizável por qualquer plugin futuro, e resolvê-la dentro de uma feature de integração misturaria dois problemas de design independentes. A tabela Ver/Agir do `README.md` cita "reiniciar container" (não logs) como o exemplo do verbo Agir para Docker. |
| C2 | O widget lista só containers rodando ou também os parados? | **Todos**, incluindo os parados (FR-001). | "containers up/**down**" no `README.md`; um widget que só mostra o que está no ar não responde "o que caiu?", que é a pergunta central do verbo Ver. |
| C3 | Quais ações de ciclo de vida entram? | Exatamente **iniciar, parar e reiniciar** (FR-007). Criar, remover, pausar/despausar e operações de imagem/rede/volume ficam Fora de Escopo. | `README.md` delimita o verbo Agir para Docker em "reiniciar container". Ação destrutiva (remover) exigiria um modelo de confirmação que o produto ainda não tem — escopo de outra feature. |
| C4 | Como se comporta a lista entre atualizações, dado que a ordem de listagem de uma ferramenta externa não é garantida? | Ordem **estável** entre atualizações consecutivas quando o conjunto não muda (FR-004, Acceptance Scenario 4 de US1). | Uma lista que embaralha sob o cursor torna o verbo Agir perigoso (o usuário clica no container errado). Requisito de produto, não detalhe de implementação — por isso virou FR. |
| C5 | O que fazer com um estado de container que o Farol não reconhece (versão futura do Docker introduz um estado novo)? | O container aparece com estado **"desconhecido"**, sem ações acionáveis, e os demais continuam sendo exibidos (FR-012). | Desvio **deliberado** do precedente de `MonitorStatus` (feature 002: valor fora do vocabulário invalida a leitura inteira). O raio de dano é diferente: o Uptime Kuma entrega um documento de métricas que o plugin interpreta como um todo (um token inválido torna a leitura inteira suspeita), enquanto o Docker reporta cada container de forma independente. Degradar uma linha é estritamente melhor do que esvaziar o widget por causa de um container. Justificativa completa no `research.md` desta feature. |
| C6 | Existe configuração (filtro por nome, só rodando, escolha de daemon)? | **Nenhuma** nesta versão — sem tela de setup. | `git-local` tem configuração porque não existe raiz de varredura padrão sensata; "todos os containers locais" já é o padrão completo e correto para Docker. Filtro sem requisito concreto seria complexidade especulativa (mesma disciplina de D3 da feature 004). Registrado em `spec.md` § Out of Scope como extensão aditiva futura. |
| C7 | Como o Farol trata a exigência de privilégio para falar com o daemon Docker? | Documentado como **premissa** (usuário já tem acesso ao daemon) + FR-010c/FR-016 (permissão negada vira erro de domínio legível, nunca prompt de senha). A questão mais ampla do `README.md` § "Decisões em aberto" fica **fora de escopo**. | Mesmo padrão de Assumption já usado por `specs/004-vpn-status-plugin/spec.md` para autorização de sistema da VPN. Resolver "sudo sob demanda vs. polkit vs. daemon auxiliar" é decisão de produto de alcance muito maior que esta integração. |
| C8 | "Ferramenta ausente", "daemon parado" e "permissão negada" precisam ser distinguíveis para o usuário, ou basta um erro genérico? | **Distinguíveis**, com mensagens diferentes (FR-010). | São três remédios completamente diferentes (instalar, subir o serviço, entrar no grupo), e a permissão negada é a condição mais provável numa máquina recém-configurada. Um erro genérico deixaria o usuário sem saber o que fazer — falha do verbo Ver. |

### Clarificações adicionais da passagem `/speckit-clarify` (mesma sessão, mesmo modo silencioso)

As três questões abaixo foram levantadas pela varredura estruturada de ambiguidade do
`/speckit-clarify` e, como as anteriores, **resolvidas pelo orquestrador sem input do usuário,
conforme instrução da sessão**. Estão registradas em `spec.md` § Clarifications (Session
2026-09-03) como bullets `Q:`/`A:` e integradas aos requisitos citados na coluna "Resolução".

| # | Questão | Resolução | Base da decisão |
|---|---|---|---|
| C9 | Quais das três ações ficam acionáveis em cada estado de container? A spec dizia "ações coerentes com o estado" sem fixar a matriz. | Matriz normativa em **FR-008**: iniciar em `criado`/`parado`; parar em `rodando`/`reiniciando`/`pausado`; reiniciar em todos esses cinco; **nenhuma** ação acionável em `em remoção`, `morto` e `desconhecido`. | "Coerente com o estado" é exatamente o tipo de adjetivo não quantificado que reprova o item "Requirements are testable and unambiguous" do checklist: sem a matriz, cada implementação escolheria um conjunto diferente e nenhum teste de aceitação seria escrevível. A matriz segue a semântica do próprio Docker (um container `criado` nunca foi iniciado, logo não há o que parar; `em remoção` e `morto` são estados terminais/transitórios em que qualquer comando de ciclo de vida falharia de todo jeito, então oferecer o botão só produziria erro). `desconhecido` herda "nenhuma ação" de FR-012 — o Farol não age sobre um estado que não sabe interpretar. |
| C10 | Qual o limite de tempo de uma consulta de estado quando o daemon não responde? | **3 segundos por consulta** (FR-014), com o erro chegando à tela dentro desse limite (SC-006). | O orçamento de tempo de `widget/get` no protocolo é de 5 s; a consulta precisa caber **com folga** dentro dele para que um daemon travado produza um erro pontual do widget em vez de estourar o orçamento e fazer a conexão inteira do plugin parecer não-responsiva. Três segundos deixam margem para o resto do ciclo (serialização, ida e volta do NDJSON) sem serem tão curtos a ponto de falhar em uma máquina carregada. Sem esse número a spec teria um requisito de robustez não verificável. |
| C11 | O que acontece se uma ação for acionada sobre um container que já tem outra em andamento, ou se o ciclo de atualização chegar no meio de uma ação? | **FR-017**: as três ações **daquele** container ficam não acionáveis enquanto a operação está em andamento (os demais seguem normais), e uma atualização de lista que chegue no meio **não** apaga a indicação de "em andamento". | Concorrência entre o ciclo de atualização automática e uma ação do usuário é a fonte clássica de bug sutil neste tipo de widget: sem o requisito, um refresh no meio de um `reiniciar` reabilitaria o botão e o usuário dispararia a segunda operação sobre um container em transição. O bloqueio é **por container**, e não global, porque bloquear o widget inteiro puniria o uso normal (agir em vários containers em sequência) sem ganho de segurança. Precedente direto: o `connect_in_flight` da feature 004, aqui generalizado para uma lista de N itens. |

- Todos os itens do checklist passam. Spec pronta para `/speckit-plan`.
