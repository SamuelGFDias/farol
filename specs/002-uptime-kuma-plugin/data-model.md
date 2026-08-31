# Data Model: Plugin de Referência Uptime Kuma

**Feature**: `002-uptime-kuma-plugin` | **Data**: 2026-08-31

Entidades extraídas da seção `Key Entities` do `spec.md`, refinadas com as decisões de `research.md`
(D1–D9). Segue o mesmo padrão de `specs/001-walking-skeleton-git-plugin/data-model.md`: (a)
entidades de protocolo (novas ou estendidas em relação ao que já é normativo em `protocol/SPEC.md`
v0.1), e (b) estado interno do core. Entidades **inalteradas** da feature 001 (`ProtocolVersion`,
`WidgetDeclaration`, `ActionDeclaration`, `ActionTarget`, `GitRepository`, `RemoteStatus`) não são
repetidas aqui — só referenciadas quando relevante.

## 1. Entidades de protocolo (wire, JSON) — novas ou estendidas nesta feature

### 1.1 `Capability` (substitui o item `string` de `CapabilityManifest.capabilities`)

Objeto discriminado por `kind` (`research.md` D1). Vocabulário de `kind` permanece aberto
(`protocol/SPEC.md` §10, preservado) — um `kind` desconhecido é aceito, registrado e exibido
genericamente pelo core, sem os campos extras serem interpretados.

| `kind` | Campos além de `kind` | Obrigatório | Descrição |
|---|---|---|---|
| `"exec"` | — | — | Mesma capacidade da feature 001, agora como objeto de um campo só: `{"kind": "exec"}`. |
| `"network"` | `host` (`string`, não-vazia) | sim | Nome de host ou IP da instância consultada (Princípio IV: allowlist é por *host*, não por URL). |
| `"network"` | `port` (`integer`, 1–65535) | não | Porta, quando conhecida/fixa (ex.: `443` para HTTPS). Ausente = não declarado, sem inferência de default pelo core. |
| `"secret"` | `reference` (`string`, não-vazia) | sim | String opaca identificando a credencial — formato de referência do CLI `op` do 1Password (`"op://<vault>/<item>/<campo>"`), nunca a credencial em si (D1/D8). O core não interpreta o prefixo `op://` nem o resto da string — só exibe. |

### 1.2 `CapabilityManifest` (forma do campo inalterada, tipo do item mudou)

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `capabilities` | `Capability[]` | sim | **Mudou de `string[]` para `Capability[]`** — mudança de wire incompatível (D1); `protocol_version` bump para `"0.2"` é a consequência direta. |

Exemplo, plugin `uptime-kuma` (configurado, com credencial resolvível):

```jsonc
{
  "capabilities": [
    { "kind": "exec" },
    { "kind": "network", "host": "monitor.example.com", "port": 443 },
    { "kind": "secret", "reference": "op://Dev/UptimeKuma/API Keys/farol" }
  ]
}
```

Exemplo, plugin `uptime-kuma` (`not_configured` — sem `base_url`, D9): `capabilities` **omite** as
entradas `network`/`secret` (nada concreto para declarar honestamente), mantendo só `exec`:

```jsonc
{ "capabilities": [ { "kind": "exec" } ] }
```

Sem enforcement nesta feature (Out of Scope do spec, mesmo padrão de FR-006 para `exec` na feature
001) — o core registra e exibe; não há campo de "concedido" vs. "solicitado".

### 1.3 `MonitorStatusItem` (item do novo widget `kind: "monitor-status-grid"`)

Reportado pelo plugin em cada resposta de sucesso de `widget/get` para o widget
`uptime-kuma-monitors` (FR-011/FR-012/FR-013).

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `name` | `string` (não-vazia) | sim | Do label `monitor_name` do `/metrics` — possivelmente sanitizado em relação ao nome de exibição original do Uptime Kuma (Assumptions do spec: perda de informação inerente ao formato de origem, não um defeito). |
| `status` | `"up" \| "down" \| "pending" \| "maintenance"` | sim | Mapeado do valor de `monitor_status` (FR-012): `1→up`, `0→down`, `2→pending`, `3→maintenance`. Um valor fora de `{0,1,2,3}` não produz este item — invalida a resposta inteira daquela leitura (`metrics_parse_error`, ver `error-model-delta.md`). |
| `response_time_ms` | `integer \| null` | sim (nullable, nunca ausente) | De `monitor_response_time` (ms), quando aplicável ao status do monitor; `null` explícito quando não aplicável — união explícita, nunca inferida de campo ausente (mesmo espírito de `RemoteStatus.NoRemote` na feature 001). |

Diferente de `WidgetItem` (feature 001), `MonitorStatusItem` **não** carrega nenhuma
`ActionDeclaration` associada — este plugin nunca declara ações (FR-004); o campo simplesmente não
existe neste tipo, não é um campo opcional vazio.

### 1.4 `WidgetGetResult` — envelope inalterado, elemento de `items` passa a depender do `widget_id`

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `widget_id` | `string` | sim | Inalterado — ecoa o `widget_id` do request. |
| `items` | `WidgetItem[]` (para `kind: "status-grid"`) **ou** `MonitorStatusItem[]` (para `kind: "monitor-status-grid"`) | sim | O elemento esperado é determinado pelo `kind` que o `widget_id` declarou no handshake (§1.5 abaixo) — sem ambiguidade em runtime, o core já sabe qual forma esperar antes de receber a resposta. `items` MAY ser uma lista vazia para qualquer `kind` (instância sem monitores cadastrados é estado válido, análogo a diretório sem repositórios git — Assumptions do spec). |

### 1.5 `WidgetDeclaration` — novo valor de `kind` no vocabulário do core, forma inalterada

Forma do tipo inalterada da feature 001 (`id`, `kind`, `title`,
`suggested_refresh_interval_ms?`). Novo valor de `kind` reconhecido pelo core:
`"monitor-status-grid"` (D4 de `research.md`). Declaração deste plugin:

```jsonc
{
  "id": "uptime-kuma-monitors",
  "kind": "monitor-status-grid",
  "title": "Uptime Kuma",
  "suggested_refresh_interval_ms": 30000
}
```

`suggested_refresh_interval_ms` **é o mesmo valor** usado internamente pelo plugin como cadência de
sua própria thread de polling em background (D6) — não dois conceitos de intervalo desacoplados.

### 1.6 `HandshakeHelloResult` — inalterado em forma, `protocol_version` passa a ser `"0.2"`

Mesma forma da feature 001 (`protocol_version`, `plugin_name`, `capabilities`, `widgets`, `actions`).
Para `uptime-kuma`: `protocol_version: "0.2"`, `plugin_name: "uptime-kuma"`, `actions: []` **sempre**
(FR-004 — nunca populado, nem no handshake nem em `widget/get`, diferente de `git-local` que popula
`actions`/`fetch_action` via `widget/get`).

### 1.7 Erro estruturado — forma inalterada, novo catálogo de `reason` para este plugin

Ver `contracts/error-model-delta.md` para a tabela completa. Novos valores de `data.reason`:
`not_configured` (`-32005`), `metrics_unreachable` (`-32006`), `metrics_parse_error` (`-32007`).
Reaproveitados sem alteração: `exec_unavailable` (`-32003`, para o binário `op` ausente do `PATH`),
`protocol_version_incompatible` (`-32000`, genérico, caminho de recusa do handshake).

## 2. Estado interno do plugin (não é wire format — vive só no processo Python do plugin)

Diferente da feature 001 (cujo `git-local` não precisava de estado entre chamadas — cada
`widget/get` refazia a varredura do zero), o plugin `uptime-kuma` **precisa** de estado
persistente-em-memória entre a thread de polling e o handler de `widget/get` (D6, resolve FR-010).

### 2.1 `PluginConfig` (lido uma vez, no arranque do processo)

| Campo | Tipo | Descrição |
|---|---|---|
| `base_url` | `Optional[str]` | Lido de `~/.config/farol/plugins/uptime-kuma/config.toml` (FR-007). `None` se arquivo ausente ou campo ausente/vazio — não há default seguro (diferente de `scan_root`, FR-008/Edge Case do spec). |
| `secret_reference` | `str` (constante fixa) | Não vem do arquivo de configuração nesta feature — string fixa e documentada no código do plugin (`"op://Dev/UptimeKuma/API Keys/farol"` no ambiente de referência), já que só uma instância é suportada por vez (Assumptions do spec, D1/D8 de `research.md`). |

### 2.2 `not_configured` (booleano derivado, calculado uma vez no arranque)

`true` se `base_url is None` **ou** `op read <secret_reference>` falha (código de saída não-zero) —
tratamento unificado por FR-019 (D9 de `research.md`). Permanece `true` (ou `false`) pelo resto da
vida do processo — sem hot-reload de configuração nesta feature. Quando `true`: a thread de polling
(§2.3) nunca é iniciada, e todo `widget/get` subsequente devolve `error(-32005, not_configured)`.

### 2.3 `MetricsCache` (compartilhado entre a thread de polling e o handler de `widget/get`, sob `threading.Lock`)

| Campo | Tipo | Descrição |
|---|---|---|
| `last_success` | `Optional[{monitors: MonitorStatusItem[], at: float}]` | Estado da última leitura bem-sucedida de `/metrics`. `None` até a primeira leitura bem-sucedida ocorrer. Nunca é limpo por uma tentativa subsequente falhar — dado antigo permanece disponível. |
| `last_error` | `Optional[{reason: "metrics_unreachable" \| "metrics_parse_error", detail: str, at: float}]` | Estado da última **tentativa**, com ou sem sucesso anterior. Pré-populado, na inicialização da thread, com `{"reason": "metrics_unreachable", "detail": "aguardando primeira leitura", "at": <start>}` — cobre "ainda não tentei" pelo mesmo caminho de "última tentativa falhou", sem estado especial adicional (D6 de `research.md`). |

**Lógica de leitura por `widget/get`** (pseudocódigo, D6/D9 de `research.md`):

```text
se not_configured:                            devolver error(-32005, not_configured)
senão se last_success is None
       ou last_error.at >= last_success.at:    devolver error(-32006|-32007, last_error.reason, detail=last_error.detail)
senão:                                          devolver success(items=last_success.monitors)
```

### 2.4 `PollerThread` (thread daemon, D6 de `research.md`)

Laço estritamente sequencial (nunca duas chamadas HTTP concorrentes, por construção — não é um
agendador paralelo): a cada `suggested_refresh_interval_ms` (mesmo valor declarado ao core, §1.5),
tenta `GET ${base_url}/metrics` com `Authorization: Basic ...` (credencial resolvida uma vez no
arranque, §2.1), timeout HTTP de 10s (D5 de `research.md`, não normativo do protocolo); em sucesso,
parseia (regras de FR-011/FR-012, § "Parsing e mapeamento de status" em
`contracts/uptime-kuma-plugin.md`) e atualiza `last_success`; em falha (rede ou parse), atualiza
`last_error` sem tocar `last_success`.

## 3. Estado interno do core (Model do iced — não é wire format)

Estende, sem alterar, o modelo já descrito por `specs/001-walking-skeleton-git-plugin/data-model.md`
§2 (`PluginConnection`, `PluginState`, `UnavailableReason`, `WidgetRefreshTimer`) — todos reutilizados
sem mudança, já que são genéricos por design (nenhum deles assume um plugin específico). Este
`data-model.md` só acrescenta o análogo, para este widget, de `RepositoryViewModel`:

### 3.1 `MonitorWidgetViewModel`

| Campo | Tipo | Descrição |
|---|---|---|
| `monitors` | `MonitorStatusItem[]` | Último `items` recebido com sucesso de `widget/get` para o `widget_id` `"uptime-kuma-monitors"`. Mantido inalterado quando uma chamada de `widget/get` retorna erro pontual (`protocol/SPEC.md` §5.2, inalterado — o core, não o plugin, é quem preserva o último estado bom para exibição, exatamente como já faz para `git-local`). |
| `last_error` | `Option<PluginError>` | Preenchido quando a última `widget/get` retornou `error` (`not_configured`/`metrics_unreachable`/`metrics_parse_error`); limpo no próximo sucesso. Distinto de `PluginState::Unavailable` — um erro de leitura pontual não muda o estado geral de disponibilidade da conexão (FR-017, herdado sem mudança do mecanismo já genérico da feature 001). |

Não existe `fetch_in_flight` nem qualquer campo ligado a ação — este widget nunca tem ação associada
(FR-004), diferente de `RepositoryViewModel.fetch_in_flight` da feature 001.

## 4. Regras de validação consolidadas (derivadas de FR)

- FR-004: `actions` MUST vir vazio (`[]`) tanto no handshake quanto em toda resposta de `widget/get`
  deste plugin — o core, ao processar este plugin, nunca encontra nenhuma `ActionDeclaration`
  associada a `uptime-kuma-monitors`.
- FR-005/D1: `Capability` de `kind: "network"`/`kind: "secret"` só aparece em `capabilities` quando o
  plugin tem, de fato, um `host`/`reference` concreto para declarar (config resolvida) — nunca um
  valor vazio/placeholder só para satisfazer a forma do manifesto.
- FR-008/FR-019/D9: `base_url` ausente **ou** credencial não resolvível via `op` (D8) produzem o
  **mesmo** estado observável (`not_configured`, `-32005`) — tratamento unificado, não dois estados
  de UI distintos.
- FR-012: `status` de `MonitorStatusItem` só assume um dos quatro valores do enum; um valor de
  `monitor_status` fora de `{0,1,2,3}` invalida a leitura inteira daquela tentativa
  (`metrics_parse_error`), não produz um item com status desconhecido.
- FR-014/D4: `kind: "monitor-status-grid"` é um vocabulário de renderização **independente** de
  `kind: "status-grid"` — o core nunca precisa interpretar um `MonitorStatusItem` como se fosse (ou
  contivesse) um `GitRepository`, e vice-versa.
- FR-017: erro pontual de `widget/get` (qualquer um dos três novos `reason`s) MUST NOT alterar
  `PluginState` — só timeout de `RPC_TIMEOUT_CONTROL` ou morte do processo fazem essa transição
  (mecanismo genérico da feature 001, inalterado, D3 de `research.md`).
