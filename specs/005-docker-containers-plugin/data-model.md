# Data Model: Plugin de Containers Docker

Mapeia as `Key Entities` do `spec.md` para os tipos concretos de protocolo (`farol-protocol`) e de
UI (`farol-core`), na mesma forma de documento já usada por
`specs/004-vpn-status-plugin/data-model.md`. Decisões justificadas em `research.md` (D1-D12) não são
repetidas aqui, só referenciadas.

Rastreabilidade com `spec.md` § Key Entities:

| Entidade do `spec.md` | Tipo concreto |
|---|---|
| **Container** | `ContainerStatusItem` (§1.3) + `ContainerState` (§1.2) |
| **ContainerAction** | as três `ActionDeclaration` de `ContainerStatusItem` (§1.3, §1.4) |
| **ContainerActionOutcome** | `ActionInvokeResult::Container` (§1.6) ou `-32011` (§1.7) |
| **DockerAvailability** | não é um tipo de dado: é a taxonomia de erro de `widget/get` (§1.7 — `-32003` e `-32010` com `data.detail.condition`) |

## §1. Protocolo (`crates/farol-protocol/src/messages.rs`, `protocol/schema/v0.4/`)

### §1.1 Constante de versão

A versão corrente vive em **dois** lugares, e ambos passam a `0.4` (D2):

- `crates/farol-core/src/plugin_worker.rs::CORE_PROTOCOL_VERSION` — de
  `ProtocolVersion { major: 0, minor: 3 }` para `{ major: 0, minor: 4 }`;
- `PROTOCOL_VERSION` (string) nos **quatro** plugins Python: os três existentes migram, o novo já
  nasce em `"0.4"`.

`crates/farol-protocol/src/version.rs` **não** muda: ele define o tipo `ProtocolVersion` e a regra
`is_compatible_with` (igualdade exata sob `MAJOR == 0`), não a versão corrente.

### §1.2 `ContainerState`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerState {
    Created,
    Restarting,
    Running,
    Removing,
    Paused,
    Exited,
    Dead,
    /// Valor de `State` que o plugin não reconheceu (FR-012, `research.md` D3.1). Desvio
    /// deliberado do precedente de `MonitorStatus`, que invalida a leitura inteira — aqui o
    /// raio de dano é uma linha, não o documento todo.
    Unknown,
}
```

Sete primeiras variantes = vocabulário publicado pelo Docker, mapeadas 1:1 do campo `State` de
`docker ps --format '{{json .}}'` (D1.1). `Unknown` **nunca** é emitido pelo Docker: é produzido
pelo plugin ao encontrar um valor fora do vocabulário.

Vocabulário do `spec.md` (PT-BR) ↔ variante:

| `spec.md` | Variante | `State` da CLI |
|---|---|---|
| criado | `Created` | `created` |
| reiniciando | `Restarting` | `restarting` |
| rodando | `Running` | `running` |
| em remoção | `Removing` | `removing` |
| pausado | `Paused` | `paused` |
| parado | `Exited` | `exited` |
| morto | `Dead` | `dead` |
| desconhecido | `Unknown` | qualquer outro |

### §1.3 `ContainerStatusItem`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerStatusItem {
    /// ID completo do container (64 hex, de `docker ps --no-trunc`). Identidade estável para
    /// FR-013 e chave de casamento do merge de UI (§2.3). NUNCA o nome.
    pub id: String,
    /// Nome exibido. Primeiro nome quando a CLI reporta vários (D1.1); nunca vazio.
    pub name: String,
    /// Imagem de origem: tag quando existe, identificador da imagem quando não (D1.1).
    /// Nunca vazio.
    pub image: String,
    pub state: ContainerState,
    /// Texto humano auxiliar da CLI (`"Up 38 hours (healthy)"`). Puramente informativo — o core
    /// MUST NOT derivar estado dele (D1.1). `None` serializa como `null` explícito, campo
    /// obrigatório e nullable (mesmo espírito de `MonitorStatusItem.response_time_ms`).
    /// Deliberadamente **não** se chama `status`, para não colidir estruturalmente com
    /// `MonitorStatusItem` na desambiguação untagged (`research.md` D12).
    pub status_text: Option<String>,
    /// `id: "docker.container.start"`, `enabled` conforme a matriz de FR-008.
    pub start_action: ActionDeclaration,
    /// `id: "docker.container.stop"`, idem.
    pub stop_action: ActionDeclaration,
    /// `id: "docker.container.restart"`, idem.
    pub restart_action: ActionDeclaration,
}
```

**Invariantes** (validadas por teste de contrato, mesmo padrão de
`widget_remote_status_tracked_ahead_and_behind_minimum_boundaries` da feature 001):

1. `id.len() == 64` e todos os caracteres em `[0-9a-f]`.
2. `!name.is_empty()` e `!image.is_empty()`.
3. Os três `ActionDeclaration` estão **sempre presentes**, com
   `target == ActionTarget { r#type: "docker-container", id: <self.id> }` — os três alvos são
   idênticos entre si e iguais ao `id` do próprio item (D4).
4. `start_action.id == "docker.container.start"`, e análogo para os outros dois.
5. `enabled` de cada ação satisfaz **exatamente** a matriz de FR-008:

   | `state` | `start_action.enabled` | `stop_action.enabled` | `restart_action.enabled` |
   |---|---|---|---|
   | `Created` | `true` | `false` | `true` |
   | `Running` | `false` | `true` | `true` |
   | `Restarting` | `false` | `true` | `true` |
   | `Paused` | `false` | `true` | `true` |
   | `Exited` | `true` | `false` | `true` |
   | `Removing` | `false` | `false` | `false` |
   | `Dead` | `false` | `false` | `false` |
   | `Unknown` | `false` | `false` | `false` |

6. `timeout_hint_ms` é **sempre** `Some`, com os valores de D6 (`20000`/`35000`/`45000`) — nunca
   ausente, para não cair no default de 120 s do core.

### §1.4 `ActionTarget` — sem mudança de forma

`ActionTarget { r#type: "docker-container", id: <ID completo> }` usa a forma genérica
(`{type: String, id: String}`) já existente desde a feature 001. **Nenhuma** mudança em
`handshake.schema.json`.

### §1.5 `WidgetItems` — quarta variante

```rust
#[serde(untagged)]
pub enum WidgetItems {
    Git(Vec<WidgetItem>),
    Monitor(Vec<MonitorStatusItem>),
    Vpn(Vec<VpnStatusItem>),
    Container(Vec<ContainerStatusItem>),   // NOVO — lista de N, 0..n (D3)
}
```

A ordem de declaração importa (untagged tenta em ordem). A análise de disjunção que garante que
nenhuma das quatro formas não-vazias colide está em `research.md` D12 — **é pré-requisito de
revisão** de qualquer quinta variante futura.

Array **vazio** permanece ambíguo (débito #5, issue #7): `items: []` sempre desserializa como `Git`.
A correção existente, `update::normalize_widget_items`, ganha um quarto braço
(`(Git(vazio), WidgetKind::Container) → Container(vec![])`) — sem mudança de abordagem. Isso importa
mais nesta feature do que nas anteriores: máquina sem nenhum container é um caso **normal e comum**
(FR-011), não uma borda rara.

### §1.6 `ActionInvokeResult` — terceira opção do `oneOf`

Forma atual (`v0.3`):

```rust
#[serde(untagged)]
pub enum ActionInvokeResult {
    Git { repo: GitRepository },
    Vpn { vpn_status: VpnStatusItem },
}
```

Forma nova (`v0.4`):

```rust
#[serde(untagged)]
pub enum ActionInvokeResult {
    Git { repo: GitRepository },
    Vpn { vpn_status: VpnStatusItem },
    Container { container: ContainerStatusItem },   // NOVO (D11)
}
```

Wire de `git-local` (`{"repo": {...}}`) e de `openfortivpn-vpn` (`{"vpn_status": {...}}`) não muda —
as chaves de topo são disjuntas, então a desambiguação untagged aqui é estrutural e sólida (ao
contrário de §1.5, cujas variantes são todas arrays).

As três ações devolvem o `ContainerStatusItem` **inteiro** pós-ação, não só o estado, porque
`enabled` das três `ActionDeclaration` muda com o estado e o core MUST NOT recalculá-lo
(`protocol/SPEC.md` §5.3, D11).

### §1.7 Catálogo de erro — duas entradas novas (`error.schema.json`, D5)

| Code | Reason | Onde | `data` |
|---|---|---|---|
| `-32003` (reaproveitado) | `exec_unavailable` | `handshake/hello` ou `widget/get` | binário `docker` ausente do `PATH` (FR-010a). |
| `-32010` (novo) | `docker_unavailable` | `widget/get` | `data.detail.condition` ∈ `{daemon_unreachable, permission_denied, timeout, cli_error}` (FR-010b/c, FR-014); `data.detail.raw` MAY carregar stderr truncado. `message` = tradução legível e **distinta por condição** (FR-010). |
| `-32011` (novo) | `container_action_failed` | `action/invoke` | `data.detail.docker_condition` ∈ `{no_such_container, container_gone, permission_denied, daemon_unreachable, timeout, cli_error}`; `data.detail.raw` = stderr truncado. `message` = tradução legível (FR-009). |

`-32000`..`-32009` estão inteiramente ocupados (`protocol/SPEC.md` §8.2) — `-32010`/`-32011` são os
próximos livres.

### §1.8 `WidgetDeclaration` — sem mudança de forma

`kind: "container-status-grid"` é só mais um valor de string livre (vocabulário já aberto,
`protocol/SPEC.md` §5.2.1/§6.3). O widget declarado é
`{id: "docker-containers", kind: "container-status-grid", title: "Containers Docker"}`.

## §2. Core (`crates/farol-core/src/model.rs`, `update.rs`)

### §2.1 `ContainerActionKind`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerActionKind {
    Start,
    Stop,
    Restart,
}
```

Tipo **só de UI** — não trafega no protocolo (o protocolo carrega o `action.id` como string). Existe
para que `action_in_flight` diga *qual* operação está em curso, permitindo o rótulo correto na tela
("parando…", "reiniciando…").

### §2.2 `ContainerViewModel`

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerViewModel {
    /// Último `ContainerStatusItem` recebido para este container.
    pub item: farol_protocol::messages::ContainerStatusItem,
    /// `Some` enquanto um `action/invoke` deste container está pendente (FR-017, D7) —
    /// estado de UI local, não de protocolo. Generaliza o `bool`
    /// `RepositoryViewModel::fetch_in_flight` para "qual das três ações".
    pub action_in_flight: Option<ContainerActionKind>,
    /// Erro da última ação **deste** container (mensagem já traduzida, FR-009), limpo no
    /// próximo sucesso — mesmo padrão de `RepositoryViewModel::last_error`.
    pub last_action_error: Option<String>,
}
```

**Invariante de UI derivada de FR-017**: enquanto `action_in_flight.is_some()`, o core MUST NOT
permitir invocar nenhuma das três ações **deste** item, mesmo que o `ActionDeclaration` mais recente
as declare `enabled: true`. Isto é uma restrição *adicional* do core sobre o que já está habilitado
— **não** é o core decidindo `enabled` (D7): §5.3 proíbe habilitar o que o plugin desabilitou, não
recusar o que ele habilitou.

### §2.3 `DockerWidgetViewModel`

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DockerWidgetViewModel {
    /// Containers na ordem em que o plugin os enviou (já ordenada, D10) — o core NÃO reordena.
    pub containers: Vec<ContainerViewModel>,
    /// Erro pontual do último `widget/get` (`docker_unavailable`/`exec_unavailable`),
    /// preservando `containers` da leitura anterior (FR-006) — mesmo padrão de
    /// `MonitorWidgetViewModel::last_error`.
    pub last_error: Option<String>,
    /// `true` depois do primeiro `widget/get` bem-sucedido. Distingue "ainda não li nada"
    /// (não mostrar "nenhum container") de "li com sucesso e não há nenhum" (FR-011,
    /// mostrar a indicação explícita) — `containers.is_empty()` sozinho é ambíguo.
    pub loaded: bool,
}
```

`PluginConnection` ganha `pub docker_widget: DockerWidgetViewModel`, campo irmão de `monitor_widget`
e `vpn_widget`, populado por `update::handle_widget_outcome`/`handle_action_outcome` a partir das
variantes `WidgetItems::Container`/`ActionInvokeResult::Container`, e vazio (`Default`) para
qualquer conexão cujo widget declarado não seja `"container-status-grid"` — mesma regra já aplicada
aos outros dois.

### §2.4 Merge preservando estado de UI (FR-017)

`update::merge_widget_items` já preserva estado de UI por item para a variante `Git`, casando por
`repo.id`. A variante `Container` reusa **a mesma estratégia**, casando por
`ContainerStatusItem.id`.

**Mudança de assinatura necessária**: hoje a função recebe `previous: &[RepositoryViewModel]` — só
sabe do estado anterior de `git-local` — e devolve `MergedWidgetItems` (`Git`/`Monitor`/`Vpn`). Para
que a variante `Container` possa preservar `action_in_flight`, ela precisa também enxergar os
`ContainerViewModel` anteriores, e `MergedWidgetItems` ganha a variante `Container`. A forma mais
enxuta é passar a `&PluginConnection` (ou um par de fatias) em vez de só a fatia de repositórios; a
escolha exata fica para a implementação, o requisito é que o estado anterior **daquele mesmo
widget** esteja disponível no ponto do merge. As variantes `Monitor` e `Vpn` continuam passando
direto, sem merge por item, como hoje.

Regras de merge da variante `Container`:

- container presente nas duas listas → `item` é substituído pelo novo; `action_in_flight` e
  `last_action_error` são **preservados** (é exatamente o que FR-017 exige: um refresh no meio da
  operação não apaga a indicação);
- container só na lista nova → entra com `action_in_flight: None`, `last_action_error: None`;
- container só na lista antiga (sumiu) → é descartado, junto com qualquer `action_in_flight` que
  tivesse — que é o encerramento da indicação exigido por FR-017 para o caso "o container deixou de
  existir".

`WidgetKind` (`update.rs`) ganha a variante `Container`, e `normalize_widget_items` o quarto braço
(§1.5).

## §3. Mapeamento CLI → Protocolo (referência rápida)

| Campo/condição da CLI | Campo do protocolo Farol |
|---|---|
| `ID` (com `--no-trunc`) | `ContainerStatusItem.id` |
| `Names` (primeiro, se houver vírgula) | `ContainerStatusItem.name` |
| `Image` | `ContainerStatusItem.image` |
| `State` | `ContainerStatusItem.state` (mapeamento 1:1 da tabela de §1.2) |
| `State` fora do vocabulário | `ContainerState::Unknown` (FR-012) |
| `Status` | `ContainerStatusItem.status_text` |
| `Command`, `CreatedAt`, `Ports`, `Labels`, `Mounts`, `Networks`, `Size`, `LocalVolumes`, `RunningFor`, `HealthStatus`, `Platform` | descartados — nenhum FR desta feature precisa deles (mesma disciplina de D3 da feature 004: campo sem uso concreto é complexidade especulativa) |
| binário `docker` ausente do `PATH` | `-32003`/`exec_unavailable` |
| stderr casa `permission denied` | `-32010`, `condition: "permission_denied"` |
| stderr casa `failed to connect`/`cannot connect`/`is the docker daemon running` | `-32010`, `condition: "daemon_unreachable"` |
| subprocess estourou 3 s | `-32010`, `condition: "timeout"` |
| qualquer outra falha de `docker ps` | `-32010`, `condition: "cli_error"` |
| `start`/`stop`/`restart` falhou | `-32011`, `docker_condition` conforme `contracts/docker-cli-mapping.md` |

Ver `contracts/docker-cli-mapping.md` para a tabela completa por comando/condição e as traduções
PT-BR, e `contracts/protocol-delta-v0.4.md` para o diff normativo de schema.
