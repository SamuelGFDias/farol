# Data Model: Plugin de Status de VPN (openfortivpn-gui)

Mapeia as `Key Entities` do `spec.md` para os tipos concretos de protocolo (`farol-protocol`) e de
UI (`farol-core`), na mesma forma de documento já usada por `specs/002-uptime-kuma-plugin/
data-model.md`. Decisões justificadas em `research.md` (D2-D8) não são repetidas aqui, só
referenciadas.

## §1. Protocolo (`crates/farol-protocol/src/messages.rs`, `protocol/schema/v0.3/`)

### §1.1 `VpnConnectionState`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnConnectionState {
    Disconnected,
    Connecting,
    Connected,
}
```

Espelha `StatusPayload.state` da CLI (D1/D3 de `research.md`) — sem variante `Error`; falha de
leitura é um erro de protocolo (`-32008`), não um valor deste enum (D3).

### §1.2 `VpnProfile`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VpnProfile {
    pub name: String,
    pub connect_action: ActionDeclaration,
}
```

Corresponde à entidade `VpnProfile` do `spec.md`. `connect_action.enabled ⟺ state ==
Disconnected` (D4) — invariante equivalente à já existente entre `WidgetItem.fetch_action.enabled`
e `repo.remote_status.kind == "no_remote"`.

### §1.3 `VpnStatusItem`

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VpnStatusItem {
    pub state: VpnConnectionState,
    pub active_profile: Option<String>,
    /// `Some` apenas quando `state == Connected`. Explícito, nunca inferido de ausência
    /// (mesmo espírito de `MonitorStatusItem.response_time_ms`).
    pub elapsed_seconds: Option<f64>,
    pub available_profiles: Vec<VpnProfile>,
    pub disconnect_action: ActionDeclaration,
}
```

Corresponde à entidade `VpnConnectionStatus` do `spec.md`. `disconnect_action.enabled ⟺ state ==
Connected` (D4).

**Invariantes** (validadas por teste de contrato, mesmo padrão de
`widget_remote_status_tracked_ahead_and_behind_minimum_boundaries`):

- `state == Connected ⟹ active_profile.is_some() && elapsed_seconds.is_some()`.
- `state != Connected ⟹ elapsed_seconds.is_none()`.
- Exatamente os `VpnProfile` cujo `name` não é o perfil atualmente conectado (quando `state ==
  Connected`) podem ter `connect_action.enabled == true`; nesta versão, todos ficam `false` durante
  `Connected` (só um perfil por vez, D3) — não há necessidade de excluir o ativo da lista, ele
  simplesmente aparece com `connect_action.enabled == false` como os demais.

### §1.4 `WidgetItems` — nova variante

```rust
#[serde(untagged)]
pub enum WidgetItems {
    Git(Vec<WidgetItem>),
    Monitor(Vec<MonitorStatusItem>),
    Vpn(Vec<VpnStatusItem>),               // NOVO — sempre length 0 ou 1 (D3)
}
```

Mesma ambiguidade estrutural de array vazio já documentada para `Git`/`Monitor` (débito #5,
corrigido por `update::normalize_widget_items` usando o `kind` do `widget_id`) — a correção
existente só precisa reconhecer mais um `kind` (`"vpn-status"`), sem mudança de abordagem.

### §1.5 `ActionInvokeResult` — generalizado para `oneOf`/enum untagged

Forma atual (`v0.2`, única variante):

```rust
pub struct ActionInvokeResult {
    pub repo: GitRepository,
}
```

Forma nova (`v0.3`):

```rust
#[serde(untagged)]
pub enum ActionInvokeResult {
    Git { repo: GitRepository },
    Vpn { vpn_status: VpnStatusItem },     // NOVO
}
```

Wire de `git-local` não muda (`{"repo": {...}}` continua validando, primeira variante tentada).
`vpn.connect`/`vpn.disconnect` bem-sucedidos devolvem `{"vpn_status": {...}}` — o `VpnStatusItem`
pós-ação, mesmo padrão de `git.fetch` devolvendo o `GitRepository` pós-fetch direto no resultado
(sem precisar de um `widget/get` extra, FR-018 equivalente desta feature).

Corresponde à entidade `VpnActionOutcome` do `spec.md`: sucesso = `ActionInvokeResult::Vpn`; erro =
`ActionInvokeResponse::Error` com `code = -32009`/`reason = "vpn_action_failed"` (D5).

### §1.6 Catálogo de erro — duas entradas novas (`error.schema.json`, D5)

| Code | Reason | Onde | `data` |
|---|---|---|---|
| `-32003` (reaproveitado) | `exec_unavailable` | `handshake/hello` ou `widget/get` | `openfortivpn-gui` ausente do `PATH`. |
| `-32008` (novo) | `vpn_status_unavailable` | `widget/get` | `status --json` da CLI falhou (`internal_error` ou saída não interpretável). `data.detail` MAY carregar a mensagem bruta. |
| `-32009` (novo) | `vpn_action_failed` | `action/invoke` | `connect`/`disconnect` falhou. `data.detail` = `{cli_code, cli_message}` da CLI; `message` do `ErrorObject` já é a tradução legível (FR-007). |

### §1.7 `WidgetDeclaration`/`ActionTarget` — sem mudança de forma

`kind: "vpn-status"` é só mais um valor de string livre (vocabulário já aberto,
`protocol/SPEC.md` §6.3). `ActionTarget { type: "vpn-profile" | "vpn-connection", id: String }` usa
a forma genérica já existente — nenhuma mudança em `handshake.schema.json`.

## §2. Core (`crates/farol-core/src/model.rs`)

### §2.1 `VpnWidgetViewModel`

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VpnWidgetViewModel {
    /// Último `VpnStatusItem` recebido com sucesso. `None` só antes do primeiro `widget/get`
    /// bem-sucedido (mesmo espírito de `MonitorWidgetViewModel::monitors` vazio inicialmente).
    pub status: Option<farol_protocol::messages::VpnStatusItem>,
    /// Erro pontual do último `widget/get` (`vpn_status_unavailable`/`exec_unavailable`),
    /// preservando `status` anterior — mesmo padrão de `MonitorWidgetViewModel::last_error`.
    pub last_error: Option<String>,
    /// `true` enquanto um `action/invoke` de `vpn.connect` está pendente para este widget
    /// (D7) — estado de UI local, não de protocolo, mesmo padrão de
    /// `RepositoryViewModel::fetch_in_flight`.
    pub connect_in_flight: bool,
    /// Idem, para `vpn.disconnect`.
    pub disconnect_in_flight: bool,
    /// Erro da última invocação de `vpn.connect`/`vpn.disconnect` (mensagem já traduzida,
    /// FR-007), limpo no próximo sucesso — mesmo padrão de `RepositoryViewModel::last_error`.
    pub last_action_error: Option<String>,
}
```

`PluginConnection` ganha `pub vpn_widget: VpnWidgetViewModel` (campo irmão de `monitor_widget`),
populado por `update::handle_widget_outcome`/`handle_action_outcome` a partir da variante
`WidgetItems::Vpn`/`ActionInvokeResult::Vpn`, e vazio (`Default`) para qualquer conexão cujo widget
declarado não seja `"vpn-status"` — mesma regra já aplicada a `monitor_widget`.

## §3. Mapeamento CLI → Protocolo (referência rápida)

| Campo/valor da CLI (`status-schema.json`) | Campo do protocolo Farol |
|---|---|
| `state` | `VpnStatusItem.state` (mesmo vocabulário: `disconnected`/`connecting`/`connected`) |
| `selected_profile` | `VpnStatusItem.active_profile` |
| `profiles[]` | `VpnStatusItem.available_profiles[].name` |
| `session.elapsed_seconds` | `VpnStatusItem.elapsed_seconds` |
| `session.profile`, `session.iface`, `session.started_at` | descartados (D3 — sem uso por nenhum FR) |
| `ErrorPayload.error.code` (de `status`) | `-32008`/`vpn_status_unavailable`, `data.detail` |
| `ErrorPayload.error.code` (de `connect`/`disconnect`) | `-32009`/`vpn_action_failed`, `data.detail.cli_code` |
| binário `openfortivpn-gui` ausente do `PATH` | `-32003`/`exec_unavailable` |

Ver `contracts/openfortivpn-cli-mapping.md` para a tabela completa por comando/código, e
`contracts/protocol-delta-v0.3.md` para o diff normativo de schema.
