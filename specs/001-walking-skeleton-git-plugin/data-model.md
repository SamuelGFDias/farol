# Data Model: Walking Skeleton — Core, Protocolo de Plugin e Plugin Git Local

**Feature**: `001-walking-skeleton-git-plugin` | **Data**: 2026-08-31

Entidades extraídas da seção `Key Entities` do `spec.md`, refinadas com as decisões de
`research.md` (D1–D8). Dividido em: (a) entidades de protocolo (trocam entre core e plugin, via
JSON-RPC — formato exato em `contracts/`), e (b) estado interno do core (`Model` do iced, nunca
serializado).

## 1. Entidades de protocolo (wire, JSON)

### 1.1 ProtocolVersion

String `"MAJOR.MINOR"` (D7). Não é um objeto — um campo string em `HandshakeHello`.

- Exemplo: `"0.1"`.
- Regra de comparação: ver `research.md` D7 e `contracts/framing-and-versioning.md`.

### 1.2 CapabilityManifest

Declarado pelo plugin no handshake (FR-007/FR-008).

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `capabilities` | `string[]` | sim | Lista de identificadores de capacidade. Nesta feature, contém ao menos `"exec"`. |

Sem enforcement nesta feature (Out of Scope da spec) — o core apenas registra e exibe o valor
declarado (FR-008); não há campo de "concedido" vs. "solicitado", só o que o plugin declara.

### 1.3 WidgetDeclaration

Declarado pelo plugin no handshake (FR-006), simétrico a `ActionDeclaration`.

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `id` | `string` | sim | Identificador estável do widget dentro deste plugin (ex.: `"repo-status"`). |
| `kind` | `string` | sim | Tipo declarativo do widget (ex.: `"status-grid"`) — vocabulário do core, não do plugin; usado pelo core para escolher como renderizar. |
| `title` | `string` | sim | Rótulo legível exibido pelo core (ex.: `"Repositórios Git"`). |
| `suggested_refresh_interval_ms` | `integer` | não | Intervalo de refresh sugerido pelo plugin (FR-011). Ausente → core aplica default de 30000ms. |

### 1.4 ActionDeclaration

Declarado pelo plugin no handshake (FR-006a), simétrico a `WidgetDeclaration`.

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `id` | `string` | sim | Identificador estável da ação (ex.: `"git.fetch"`). |
| `label` | `string` | sim | Rótulo legível para exibição na UI (ex.: `"Fetch"`). |
| `target` | `ActionTarget` | sim | Alvo sobre o qual a ação opera. |
| `enabled` | `boolean` | sim | Estado declarado pelo plugin — o core MUST NOT decidir isso por conta própria (FR-006b). |
| `timeout_hint_ms` | `integer` | não | Sugestão do plugin para o orçamento de timeout desta ação específica (ver `RPC_TIMEOUT_ACTION`, D6 em `research.md`). Ausente → core aplica default de 120000ms. Mesmo padrão de "plugin sugere, core respeita, default na ausência" já usado em `WidgetDeclaration.suggested_refresh_interval_ms` (FR-011). |

`ActionTarget` (objeto):

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `type` | `string` | sim | Tipo de alvo (nesta feature, sempre `"repo"`). |
| `id` | `string` | sim | Identificador do alvo dentro do tipo (ex.: caminho do repositório). |

Nota de instância: cada repositório reportado por `widget/get` tem sua própria `ActionDeclaration`
de fetch (uma ação por repositório-alvo, não uma ação genérica "fetch" desacoplada de um repo) —
ver `contracts/action-protocol.md` para a forma exata de como o handshake versus atualizações
periódicas mantêm essa lista de ações em sincronia com a lista de repositórios.

### 1.5 GitRepository (item do widget)

Reportado pelo plugin em cada resposta de `widget/get` (FR-013/FR-014).

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `id` | `string` | sim | Identificador estável (caminho absoluto do repositório). |
| `name` | `string` | sim | Nome curto para exibição (ex.: nome do diretório). |
| `path` | `string` | sim | Caminho absoluto no filesystem. |
| `dirty` | `boolean` | sim | `true` = working tree com mudanças pendentes; `false` = limpa. |
| `remote_status` | `RemoteStatus` | sim | Ver abaixo — inclui o caso "sem remoto" explícito (FR-014). |

`RemoteStatus` (union discriminada por `kind`):

- `{ "kind": "tracked", "ahead": <int>=0>, "behind": <int>=0> }` — repositório com remoto
  configurado; `ahead`/`behind` sempre presentes (podem ser `0`).
- `{ "kind": "no_remote" }` — repositório sem remoto configurado (FR-014). Distinto de
  `{"kind":"tracked","ahead":0,"behind":0}` — são serializações diferentes, o core nunca precisa
  adivinhar "0 ahead/0 behind" a partir de um campo ausente.

**Validação/Regra de negócio**: se `remote_status.kind == "no_remote"`, a `ActionDeclaration` de
fetch correspondente a este repositório MUST ter `enabled: false` (FR-014, decisão de
Clarifications Q4). O plugin garante essa consistência; o core apenas reflete o que recebe.

### 1.6 HandshakeHello (request, core → plugin) e HandshakeHelloResult (response, plugin → core)

Ver `contracts/handshake.md` para o schema completo. Resumo dos campos:

`HandshakeHello` (params do request):
- `protocol_version: ProtocolVersion` — versão que o core suporta.
- `core_name: string` — identificação do core (ex.: `"farol-core"`).

`HandshakeHelloResult` (result do response):
- `protocol_version: ProtocolVersion` — versão que o plugin fala.
- `plugin_name: string` — identidade do plugin (FR-006).
- `capabilities: CapabilityManifest`.
- `widgets: WidgetDeclaration[]`.
- `actions: ActionDeclaration[]` (pode estar vazia na resposta do handshake inicial, se a lista de
  ações só é conhecida após a primeira varredura — ver `contracts/handshake.md` para a decisão de
  sequenciamento adotada).

### 1.7 Erro estruturado (JSON-RPC error object)

Ver `contracts/error-model.md`. Estrutura padrão JSON-RPC 2.0 (`code`, `message`, `data` opcional),
com `data.reason` de domínio Farol quando aplicável (ex.: `"git_not_found"`,
`"fetch_failed"`, `"protocol_version_incompatible"`).

## 2. Estado interno do core (Model do iced — não é wire format)

Estas estruturas vivem só no `Model` da aplicação iced; nunca são serializadas para o plugin.
Descritas aqui para deixar explícito como o core interpreta as entidades de protocolo acima ao
longo do tempo (estado, não só forma de mensagem).

### 2.1 PluginConnection

| Campo | Tipo | Descrição |
|---|---|---|
| `state` | `PluginState` | Ver abaixo. |
| `identity` | `Option<PluginIdentity>` | Preenchido após handshake bem-sucedido (`plugin_name`, `protocol_version`, `capabilities`). |
| `widgets` | `Vec<WidgetDeclaration>` | Congelado no handshake (FR-006 — a lista de widgets não muda depois). |

`PluginState` (enum, transições — ver Seção 3):

- `Starting` — processo filho sendo iniciado (`spawn()` em andamento).
- `Handshaking` — processo vivo, handshake enviado, aguardando `HandshakeHelloResult`.
- `Ready` — handshake concluído com versão compatível; widget(s) disponíveis para renderização.
- `Unavailable { reason: UnavailableReason, detail: string }` — estado terminal exibido como
  "indisponível" na UI (FR-020), distinguível de `Starting`/`Handshaking` (que são estados de
  "carregando", não de erro).

`UnavailableReason` (enum, uso interno/diagnóstico — D6; a UI desta feature não precisa distinguir
estes casos entre si, só precisa distinguir "Unavailable" de "Starting/Handshaking/Ready"):

- `FailedToStart` — o processo nem chegou a subir (binário ausente/não executável).
- `VersionIncompatible` — handshake concluiu, versões incompatíveis (FR-005).
- `Crashed` — processo terminou inesperadamente depois de `Ready` (FR-019).
- `Unresponsive` — processo vivo, mas não respondeu dentro do `RPC_TIMEOUT_CONTROL` no handshake ou
  num ciclo de refresh (FR-019, D6). Estouro do `RPC_TIMEOUT_ACTION` de uma invocação de ação
  pontual (`action/invoke`) NÃO produz este estado — ver `contracts/action-protocol.md`.

### 2.2 RepositoryViewModel

Espelha `GitRepository` (1.5) mais estado de UI derivado (não vem do plugin):

| Campo | Tipo | Descrição |
|---|---|---|
| `repo` | `GitRepository` | Último dado recebido de `widget/get`. |
| `fetch_action` | `ActionDeclaration` | Última declaração de ação de fetch para este repo. |
| `fetch_in_flight` | `boolean` | Estado de UI local — true enquanto uma invocação de `action/invoke` está pendente para este repo (evita o core reinterpretar ausência de requisito de concorrência da spec como "sem controle nenhum" — ver Edge Cases da spec: não há requisito formal, mas o Model precisa de *algum* estado para não desenhar dois spinners conflitantes; tratado como detalhe de implementação, não de protocolo). |
| `last_error` | `Option<PluginError>` | Preenchido quando a última invocação de fetch retornou erro (FR-018); limpo no próximo sucesso. |

### 2.3 WidgetRefreshTimer

Estado (não persistido) do ciclo de FR-011: intervalo efetivo (`suggested_refresh_interval_ms` do
handshake, ou 30000 default) e o `Instant` do último refresh — modelado como uma
`iced::Subscription` de `time::every(interval)` (ver D4, `research.md`), não como um campo
explícito de polling manual.

## 3. Transições de estado — `PluginState`

```
Starting ──(spawn falhou)──────────────────────────► Unavailable{FailedToStart}
Starting ──(spawn ok)──► Handshaking
Handshaking ──(timeout RPC_TIMEOUT_CONTROL, D6)────► Unavailable{Unresponsive}
Handshaking ──(resposta, versão incompatível, D7)──► Unavailable{VersionIncompatible}
Handshaking ──(resposta, versão compatível)────────► Ready
Ready ──(child.wait() resolve, D6)─────────────────► Unavailable{Crashed}
Ready ──(timeout RPC_TIMEOUT_CONTROL em refresh)───► Unavailable{Unresponsive}
Ready ──(widget/get e action/invoke normais)───────► Ready (permanece; atualiza RepositoryViewModel)
Unavailable{*} ──(nenhuma transição nesta feature)──► (terminal — Out of Scope: restart automático)
```

`Unavailable` é terminal nesta feature — reinício automático do plugin está explicitamente Fora de
Escopo no `spec.md`. Uma vez indisponível, o core permanece funcionando (FR-021) mas não tenta
reconectar sozinho.

## 4. Regras de validação consolidadas (derivadas de FR)

- FR-005/D7: `HandshakeHelloResult.protocol_version` incompatível com o core → transição direta
  para `Unavailable{VersionIncompatible}`, nenhum widget é registrado.
- FR-006b: o core MUST NOT construir `ActionDeclaration` a partir de heurística própria — só usa o
  que veio em `widgets[]`/`actions[]` do handshake (ou de `widget/get`, para ações por-repositório
  — ver `contracts/action-protocol.md`).
- FR-014: `remote_status.kind == "no_remote"` ⟺ `fetch_action.enabled == false` para aquele
  `GitRepository.id` — consistência que o plugin garante; o core não infere isso, apenas confia e
  reflete (mas pode, defensivamente, tratar uma inconsistência recebida como
  `fetch_action.enabled == false` de qualquer forma — nunca habilitar fetch para um repo sem
  remoto, mesmo que o plugin declare errado; ver nota de robustez em
  `contracts/git-local-plugin.md`).
- FR-018: sucesso de `action/invoke` para `git.fetch` MUST vir acompanhado do `GitRepository`
  atualizado (novo `ahead`/`behind`) — o core não faz um `widget/get` adicional só para atualizar
  esse repositório (ver `contracts/action-protocol.md` para a forma da resposta).
