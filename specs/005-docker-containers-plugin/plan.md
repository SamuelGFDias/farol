# Implementation Plan: Plugin de Containers Docker

**Branch**: `005-docker-containers-plugin` (trabalho feito direto em `main`, mesmo padrão das
features 001-004 deste repositório — sem branch de feature separada)

**Date**: 2026-09-03

**Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/005-docker-containers-plugin/spec.md`

## Summary

Plugin de referência `docker-containers` que expõe **todos** os containers Docker locais — incluindo
os que não estão em execução — como um widget declarativo do core, com nome, imagem e estado de cada
um (User Story 1, verbo Ver), e permite **iniciar, parar e reiniciar** cada container direto pelo
widget (User Story 2, verbo Agir). Sem duplicar nenhuma lógica de gerenciamento de container: o
plugin invoca o binário `docker` já instalado na máquina (`docker ps` / `docker start|stop|restart`)
e traduz a saída para o `farol-protocol`, mesmo modelo dos três plugins existentes — processo Python
independente falando JSON-RPC sobre stdin/stdout.

Abordagem técnica: extensão **aditiva** do protocolo (`0.3` → `0.4`) — novo `kind` de widget
`"container-status-grid"` com `ContainerStatusItem[]`, e uma terceira opção no `oneOf` de
`ActionInvokeResult` — sem quebrar o wire já emitido por `git-local`/`uptime-kuma`/
`openfortivpn-vpn`. Como a série `0.x` exige igualdade exata de versão
(`ProtocolVersion::is_compatible_with`, `crates/farol-protocol/src/version.rs`), os **três** plugins
existentes migram sua constante `PROTOCOL_VERSION` para `"0.4"` dentro desta mesma feature
(`research.md` D2 — mantendo o padrão que a feature 004 corrigiu, em vez de reabrir o débito #4 da
feature 002).

Duas decisões estruturam o resto do desenho:

1. **Fonte de dados: a CLI `docker`, não a Engine API em `/var/run/docker.sock`** (`research.md`
   D1). Falar o socket direto exigiria reimplementar resolução de `DOCKER_HOST`/`docker context`,
   negociação de versão da API e TLS — duplicando lógica de cliente Docker (contra FR-015) — e
   exigiria uma `capability` que não existe no `CapabilityManifest` atual, piorando a granularidade
   de permissão declarada ao usuário (contra o Princípio IV). A CLI já resolve tudo isso, e
   `capabilities: [{"kind": "exec"}]` é exatamente a permissão que `git-local` já declara.
2. **O widget é uma lista de N itens com três ações por item**, estruturalmente igual a
   `status-grid`/`monitor-status-grid` e diferente do singleton `vpn-status` — reusando o padrão
   "item de dado emparelhado com a `ActionDeclaration` que opera sobre ele" que existe desde
   `WidgetItem.fetch_action` (feature 001), agora com três ações em vez de uma (`research.md` D3/D4).
   Por isso o delta de protocolo é pequeno apesar de este ser o widget mais rico do produto até aqui.

O plugin não roda poller em background: como `git-local` e `openfortivpn-vpn`, reconsulta o estado a
cada `widget/get` chamando `docker ps` de forma síncrona, sem cache (`research.md` D9).

## Technical Context

**Language/Version**: Rust (core, `crates/farol-core`/`crates/farol-protocol`, mesma toolchain das
features 001-004) + Python 3 stdlib (plugin `docker-containers`, mesmo padrão dos outros três — sem
dependências externas, só `json`/`sys`/`subprocess`/`shutil`).

**Primary Dependencies**: `iced 0.14` (core, já em uso); **nenhuma dependência nova**. O plugin
invoca o binário `docker` via `subprocess` — não importa nenhuma biblioteca cliente do Docker
(`research.md` D1).

**Storage**: N/A — o plugin não persiste nada e não mantém estado entre chamadas; todo estado de
container vem da CLI a cada requisição.

**Testing**: `cargo test --workspace` (core/protocolo, incluindo Camada 1 e2e via
`iced_test::Emulator`, snapshots visuais via `insta`, e o gerador de casos de borda de contrato
`schema_boundaries.rs`); `python3 -m unittest discover -p "test_*.py"` dentro de
`plugins/docker-containers/` (testes colocados junto do código, mesmo padrão de `uptime-kuma`/
`openfortivpn-vpn`); harness de smoke `tests/integration/harness.sh` (Camada 2) estendido para
confirmar que `docker-containers` também chega a `Ready`. Fixture determinística de `docker` no
`PATH` (`PathPrefixGuard`, mesmo padrão de `tests/fixtures/fake-openfortivpn-gui/`) — **nenhum teste
pode depender de um daemon Docker real nem mutar containers da máquina de quem roda a suíte**, o que
importa mais aqui do que nas features anteriores porque as ações desta feature são mutantes sobre
recursos reais do usuário.

**Target Platform**: Linux desktop (mesmo do restante do Farol) — depende de `docker` já instalado e
no `PATH` da mesma máquina, e de o usuário do processo já ter acesso ao daemon (Assumptions do
`spec.md`).

**Project Type**: Desktop app + plugin de referência (mesma forma das features 001/002/004).

**Performance Goals**: Sem meta numérica nova além do modelo de polling existente (refresh a cada
`suggested_refresh_interval_ms`, default 30000 ms). O custo de um processo `docker ps` (~50-80 ms em
máquina de desenvolvimento) é irrelevante nesse orçamento.

**Constraints** (todas de `research.md` D6):

- `docker ps` dentro de `widget/get` MUST desistir em **3 s** (FR-014) — estritamente abaixo do
  `RPC_TIMEOUT_CONTROL` de 5 s (`protocol/SPEC.md` §7.1), para que um daemon travado produza um erro
  pontual do widget e não faça a conexão inteira do plugin parecer `Unresponsive`.
- `docker stop` tem período de graça de **10 s** por default; `restart` é `stop` + `start`. As três
  ações MUST declarar `timeout_hint_ms` explícito (`20000`/`35000`/`45000`), sempre **acima** do
  timeout interno do subprocess, para que um estouro vire erro de domínio traduzido (`-32011`) do
  plugin e não `-32002`/`action_timeout` sintetizado pelo core. O default de 120 s do core seria
  frouxo demais.

**Scale/Scope**: Dezenas de containers numa máquina de desenvolvimento; sem filtro nem paginação
nesta versão (`spec.md` § Out of Scope). Sem configuração e sem tela de setup (`research.md` D9).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Princípio | Avaliação |
|---|---|
| I. Nativo e sem navegador | N/A a esta feature — não introduz nenhum motor de navegador. |
| II. Plugins como processos isolados via JSON-RPC | Satisfeito — `docker-containers` é mais um processo Python independente falando o mesmo protocolo, mesmo padrão dos três existentes. A decisão D1 (CLI em vez do socket do daemon) reforça o princípio: o plugin fala com um **contrato de processo**, não com uma API interna. |
| III. Widgets declarativos, core renderiza | Satisfeito — `ContainerStatusItem` é dado declarativo (identidade, estado, três `ActionDeclaration`); a `view.rs` do core decide como desenhar a linha e os três controles. O plugin nunca emite markup. Ponto crítico: `enabled` das três ações é decidido **pelo plugin** a partir da matriz de FR-008 (conhecimento de domínio Docker), nunca inferido pelo core. |
| IV. Permissões explícitas por manifesto | Satisfeito — o plugin declara `capabilities: [{"kind": "exec"}]` (a mesma já usada por `git-local` para `git`) para justificar invocar `docker`. `required_config: []`, sem segredo a gerenciar e sem tela de setup. Nenhuma capability de rede: o daemon é local, e o que ele faz é responsabilidade dele, mesmo raciocínio já aceito para `git fetch`. **A alternativa rejeitada em D1 (socket do daemon) exigiria uma capability inexistente no vocabulário atual — este princípio foi um dos motivos da rejeição, não uma verificação a posteriori.** |
| V. Espaços por contexto | N/A a esta feature — plugin é ativado/desativado por espaço como qualquer outro, sem lógica nova. |
| VI. Paleta de comandos universal | Satisfeito "de graça" — as três ações são `ActionDeclaration`s como qualquer outra; a paleta já agrega toda ação declarada por todo plugin ativo. Nota: com N containers a paleta passa a ver `3 × N` ações em vez de um punhado — é a primeira feature em que a paleta enxerga uma quantidade de ações proporcional ao dado, e não fixa. Não exige trabalho nesta feature (as ações são bem rotuladas e têm `target` distinto), mas é o primeiro sinal de que a paleta vai precisar de agrupamento/busca quando ganhar volume; registrado em Complexity Tracking, não resolvido aqui. |
| VII. Registry federado sem infra própria | N/A a esta feature — plugin entra pelo mesmo registro hardcoded (`plugin_worker::known_plugins()`) das features anteriores. |

**Governance** (dívida técnica deliberada MUST virar issue): as duas dívidas identificadas nesta
feature já estão registradas no tracker — **issue #9** (desambiguação untagged de `WidgetItems`) e
**issue #10** (superfície de detalhe/drill-down no core, pré-requisito para logs de container). Ver
Complexity Tracking.

**Re-avaliação pós-Phase 1**: nenhuma das decisões de `research.md`/`data-model.md`/`contracts/`
introduziu violação. O ponto que mais se aproximou de uma foi FR-017 (o core recusando invocar uma
ação que o plugin declarou `enabled: true` enquanto há operação em curso naquele item); a análise
está em `research.md` D7 — §5.3 do `protocol/SPEC.md` proíbe o core **habilitar** o que o plugin
desabilitou, não recusar o que ele habilitou, então o Princípio III segue intacto.

## Project Structure

### Documentation (this feature)

```text
specs/005-docker-containers-plugin/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
│   ├── protocol-delta-v0.4.md
│   └── docker-cli-mapping.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
protocol/
├── SPEC.md                       # +§5.2.1 kind "container-status-grid"; §8.2 +2 códigos;
│                                 # §7.2 nota sobre timeout_hint_ms das ações de container
└── schema/
    ├── v0.3/                     # congelado, inalterado (histórico, como v0.1/v0.2)
    └── v0.4/                     # novo — widget/action/error/handshake .schema.json
                                  # (handshake sem mudança de forma, só o texto de topo)

crates/
├── farol-protocol/
│   ├── src/
│   │   └── messages.rs           # +ContainerState, +ContainerStatusItem;
│   │                             # WidgetItems ganha variante Container(Vec<ContainerStatusItem>);
│   │                             # ActionInvokeResult ganha variante Container{container}
│   │                             # (version.rs NÃO muda: define o tipo e a regra de
│   │                             #  compatibilidade, não a versão corrente)
│   └── tests/
│       ├── contract_schema_validation.rs   # include_str!/$id v0.3 → v0.4; exemplos novos
│       └── schema_boundaries.rs            # idem + fronteiras de ContainerStatusItem
└── farol-core/
    └── src/
        ├── plugin_worker.rs      # CORE_PROTOCOL_VERSION 0.3 → 0.4 (a constante da versão
        │                         # corrente vive aqui); known_plugins() ganha "docker-containers"
        ├── model.rs              # +ContainerActionKind, +ContainerViewModel,
        │                         # +DockerWidgetViewModel, +PluginConnection::docker_widget
        ├── update.rs             # WidgetKind ganha Container; normalize_widget_items ganha o
        │                         # quarto braço; merge_widget_items preserva action_in_flight
        │                         # por id (FR-017); roteamento de widget/action outcome
        ├── view.rs               # renderização da lista de containers: nome, imagem, estado,
        │                         # três botões por linha, indicador de operação em curso, erros
        ├── e2e_tests.rs          # +cenários com a fixture de docker; asserções de versão "0.4"
        ├── visual_snapshot_tests.rs  # +cenário(s) de snapshot visual do novo widget
        └── snapshots/            # +arquivos .snap correspondentes (insta)

plugins/
├── git-local/main.py             # PROTOCOL_VERSION "0.3" → "0.4" (mecânico, sem mudança de wire)
├── uptime-kuma/main.py           # idem
├── openfortivpn-vpn/main.py      # idem
└── docker-containers/            # NOVO plugin de referência
    ├── main.py                   # dispatch handshake/widget/action, mesmo esqueleto de git-local
    ├── docker_cli.py             # wrapper subprocess sobre `docker ps|start|stop|restart`,
    │                             # classificação de stderr, tradução para ErrorObject
    ├── pyproject.toml            # config ruff, mesmo padrão dos outros três plugins
    └── test_docker_cli.py        # testes colocados junto do código (padrão uptime-kuma/vpn)

tests/
├── fixtures/fake-docker/         # NOVO — binário `docker` determinístico para os testes e2e
└── integration/harness.sh        # Camada 2 — mais uma condição: docker-containers chega a Ready
```

**Structure Decision**: mesma forma de projeto das features 001-004 (core Rust em `crates/`,
protocolo compartilhado em `protocol/`, plugins Python independentes em `plugins/<nome>/`, fixtures
determinísticas em `tests/fixtures/`) — esta feature não introduz nenhum diretório de topo novo, só
um novo plugin, uma nova fixture e uma nova versão de schema lado a lado com as anteriores
(congeladas).

## Complexity Tracking

Sem **violação** de Constitution Check — nenhuma linha da tabela acima ficou insatisfeita. Esta
seção registra três acoplamentos/dívidas conscientes que o desenho aceita, para que fiquem
rastreáveis e não sejam redescobertos como surpresa:

| Item | Por que é aceito agora | Onde está registrado |
|---|---|---|
| **Classificação de erro por substring de stderr** (`research.md` D5.1) — as três condições de FR-010 são distinguidas por texto não-contratual do cliente Docker, que já mudou de formulação entre versões maiores. | Não há alternativa: a CLI não expõe códigos de saída distintos por condição, e a alternativa (Engine API) foi rejeitada em D1 por motivos mais pesados. Mitigado por três camadas: a substring mais estável (`permission denied`) é testada primeiro; o `fallback` `cli_error` garante que uma reformulação futura degrade para "erro genérico legível" e nunca para silêncio ou classificação errada silenciosa; e `data.detail.raw` sempre carrega o stderr bruto. | `research.md` D5.1, `contracts/docker-cli-mapping.md` |
| **Desambiguação untagged de `WidgetItems` por disjunção incidental de formato** — a quarta variante ainda desambigua, mas porque os conjuntos de campos obrigatórios são disjuntos por acaso, não por desenho. Um quinto `kind` precisará repetir a análise, e um dia duas formas colidirão. | A alternativa estrutural (envelope com tag explícita, `{"kind": ..., "items": [...]}`) é uma mudança **não aditiva** do wire de todos os plugins — escopo de uma feature própria de protocolo, não desta integração. Mitigado nesta feature nomeando o campo auxiliar `status_text` (e não `status`) para não aproximar `ContainerStatusItem` de `MonitorStatusItem`. | `research.md` D12; **issue #9** (Governance) |
| **Logs de container fora de escopo** — o `README.md` cita "Docker — containers up/down, **logs**"; esta feature entrega só a primeira metade. | Logs exigem uma superfície de UI de detalhe/drill-down que o core não tem (hoje todo widget renderiza lista plana). É decisão de arquitetura do **core**, reutilizável por qualquer plugin futuro; resolvê-la de dentro de uma feature de integração misturaria dois problemas de design independentes. | `spec.md` § Out of Scope, C1 de `checklists/requirements.md`; **issue #10** (Governance) |

Um quarto ponto, mais fraco, fica só anotado sem issue: esta é a primeira feature em que a paleta de
comandos universal (Princípio VI) enxerga uma quantidade de ações **proporcional ao dado**
(`3 × N` containers) em vez de um punhado fixo. Não afeta esta entrega — as ações têm rótulo e
`target` distintos — mas é o primeiro sinal de que a paleta precisará de agrupamento/busca quando
ganhar volume. Sem requisito concreto ainda, portanto sem issue (mesma disciplina de não abrir
dívida especulativa).

## Phase 0 / Phase 1 — artefatos gerados

| Fase | Artefato | Conteúdo |
|---|---|---|
| 0 | [`research.md`](./research.md) | D1 (CLI vs. Engine API, com a tabela de alternativas rejeitadas), D1.1 (forma exata da consulta), D2 (bump `0.4` + migração dos três plugins), D3/D3.1 (novo `kind` e `ContainerState` com `Unknown`), D4 (três ações por item, `enabled` no plugin), D5/D5.1 (dois códigos de erro novos e detecção das três condições de FR-010), D6 (orçamentos de tempo), D7 (`action_in_flight` sobrevivente ao refresh), D8 (nomes), D9 (sem poller/sem setup), D10 (ordenação estável), D11 (o que `action/invoke` devolve), D12 (verificação da desambiguação untagged) |
| 1 | [`data-model.md`](./data-model.md) | Tipos de protocolo (§1) com invariantes testáveis incluindo a matriz de `enabled`; tipos de UI do core (§2) incluindo o merge que preserva `action_in_flight`; mapeamento CLI→protocolo (§3) |
| 1 | [`contracts/protocol-delta-v0.4.md`](./contracts/protocol-delta-v0.4.md) | Diff normativo de schema `0.3` → `0.4`, seções de `protocol/SPEC.md` a atualizar, checklist de migração dos plugins existentes |
| 1 | [`contracts/docker-cli-mapping.md`](./contracts/docker-cli-mapping.md) | Sequência e classificação de `widget/get` e das três ações, tabelas de tradução PT-BR, saídas observadas em Docker 29.6.2 para as fixtures, rastreabilidade com os Edge Cases do `spec.md` |
| 1 | [`quickstart.md`](./quickstart.md) | Cenários de validação manual ponta a ponta das duas User Stories e das condições de falha |
