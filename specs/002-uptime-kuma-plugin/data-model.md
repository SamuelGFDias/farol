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
| `"exec"` | — | — | Mesma capacidade da feature 001, agora como objeto de um campo só: `{"kind": "exec"}`. **Não** declarada por `uptime-kuma` (ver nota abaixo). |
| `"network"` | `host` (`string`, não-vazia) | sim | Nome de host ou IP da instância consultada (Princípio IV: allowlist é por *host*, não por URL). Derivado do `base_url` já resolvido via variável de ambiente (`research.md` D8), não lido de arquivo pelo plugin. |
| `"network"` | `port` (`integer`, 1–65535) | não | Porta, quando conhecida/fixa (ex.: `443` para HTTPS). Ausente = não declarado, sem inferência de default pelo core. |

**Revisão desta sessão (`research.md` D1/D8, auditoria pós-plan)**: o `kind: "secret"` (`reference:
string`) desta tabela em versões anteriores deste documento foi **removido** — a credencial deixa de
ser declarada via `Capability` e passa a ser declarada via `required_config` (§1.6.1 abaixo), um campo
novo e irmão de `capabilities` em `HandshakeHelloResult`, gerenciado pelo core (armazenamento e
injeção), não mais resolvido pelo próprio plugin via CLI externo. Como consequência, `uptime-kuma`
também deixa de declarar `{"kind": "exec"}` — a única razão para declará-lo era invocar o CLI `op`
(1Password), que não existe mais nesta arquitetura; `uptime-kuma` não invoca nenhum binário externo.
`CapabilityManifest.capabilities` MAY ser `[]` (sem `minItems: 1`) quando nenhuma capacidade concreta
é declarável ainda (ex.: `base_url` não resolvido).

### 1.2 `CapabilityManifest` (forma do campo inalterada, tipo do item mudou)

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `capabilities` | `Capability[]` | sim | **Mudou de `string[]` para `Capability[]`** — mudança de wire incompatível (D1); `protocol_version` bump para `"0.2"` é a consequência direta. |

Exemplo, plugin `uptime-kuma` (configurado — `base_url` resolvido via variável de ambiente injetada
pelo core, `research.md` D8):

```jsonc
{
  "capabilities": [
    { "kind": "network", "host": "monitor.example.com", "port": 443 }
  ]
}
```

Exemplo, plugin `uptime-kuma` (`NotConfigured` — `base_url` não resolvido, §2.2/§3.2 abaixo):
`capabilities` **omite** a entrada `network` (nada concreto para declarar honestamente) — sem `exec`
como piso, diferente de versões anteriores deste documento (D1/D8 revisados: `uptime-kuma` não
declara mais `exec`):

```jsonc
{ "capabilities": [] }
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

### 1.4 `WidgetGetResult` — `items` passa a ser uma união discriminada, MUDA de forma (correção C3 da auditoria pós-plan)

**Correção desta sessão**: versões anteriores deste documento afirmavam que o envelope
`WidgetGetResult { widget_id, items }` "não muda de forma" e que só o *tipo do elemento* de `items`
passaria a depender do `kind` do `widget_id`. Isso é impreciso para o binding Rust real: hoje,
`crates/farol-protocol/src/messages.rs` define `WidgetGetResult.items: Vec<WidgetItem>` — um `Vec` de
um único tipo Rust concreto, sem espaço para um segundo tipo de item (`MonitorStatusItem`). Introduzir
um segundo `kind` de widget com uma forma de item diferente exige, do lado do binding Rust, que
`items` deixe de ser `Vec<WidgetItem>` fixo e passe a ser uma união discriminada que aceite
`WidgetItem` (git) **ou** `MonitorStatusItem` (uptime-kuma) — não apenas uma reinterpretação de tipo
em runtime sem mudança de código.

| Campo | Tipo (wire, JSON) | Obrigatório | Descrição |
|---|---|---|---|
| `widget_id` | `string` | sim | Inalterado — ecoa o `widget_id` do request. |
| `items` | `WidgetItem[]` (para `kind: "status-grid"`) **ou** `MonitorStatusItem[]` (para `kind: "monitor-status-grid"`) | sim | O elemento esperado é determinado pelo `kind` que o `widget_id` declarou no handshake (§1.5 abaixo) — sem ambiguidade em runtime, o core já sabe qual forma esperar antes de receber a resposta. `items` MAY ser uma lista vazia para qualquer `kind` (instância sem monitores cadastrados é estado válido, análogo a diretório sem repositórios git — Assumptions do spec). |

**Binding Rust — desenho da união discriminada** (a decisão exata de forma — enum genérico sobre
`items`, ou dois campos `Option<Vec<T>>` mutuamente exclusivos, ou `WidgetGetResult<T>` genérico por
`widget_id` — é decisão de implementação da task de protocolo desta feature, não redesenhada aqui;
qualquer uma das formas MUST preservar a garantia central: o core, ao processar a resposta de
`widget_id: "uptime-kuma-monitors"`, nunca tenta desserializar `items` como `WidgetItem`, e
vice-versa para `"repo-status"`). Consequência direta em `crates/farol-core/src/update.rs`:
`merge_widget_items` (hoje tipado para `previous: &[RepositoryViewModel], new_items:
Vec<farol_protocol::WidgetItem>`) precisa de um equivalente irmão para `MonitorStatusItem` — ou uma
generalização que aceite ambos, resolvida do mesmo jeito. Nenhuma destas mudanças toca
`WidgetDeclaration`/`kind` em si (§1.5), que continua sendo o único sinal que determina qual forma
esperar.

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

### 1.6 `HandshakeHelloResult` — ganha o campo novo `required_config`, demais campos inalterados em forma

Forma da feature 001 (`protocol_version`, `plugin_name`, `capabilities`, `widgets`, `actions`) **mais
um campo novo, irmão dos demais**: `required_config: RequiredConfigItem[]` (`research.md` D8 —
substitui a declaração de credencial que uma versão anterior deste documento modelava via
`Capability{kind:"secret"}`, ver §1.1). Posição no objeto: logo após `capabilities`, antes de
`widgets` (agrupamento lógico: "o que o plugin pode fazer" seguido de "o que o plugin precisa que o
usuário forneça").

Para `uptime-kuma`: `protocol_version: "0.2"`, `plugin_name: "uptime-kuma"`, `actions: []` **sempre**
(FR-004 — nunca populado, nem no handshake nem em `widget/get`, diferente de `git-local` que popula
`actions`/`fetch_action` via `widget/get`).

### 1.6.1 `RequiredConfigItem` (novo — `research.md` D8)

Cada item declara uma variável de configuração que o plugin precisa do usuário, secreta ou não. O
core, não o plugin, resolve, armazena e injeta o valor (§2.1/§2.2 abaixo, §3.2 para o estado de UI).

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `name` | `string` (não-vazia) | sim | Identificador estável da variável, escolhido pelo plugin (ex.: `"base_url"`, `"api_key"`). Usado pelo core para derivar o nome da variável de ambiente injetada (`FAROL_PLUGIN_<PLUGIN_NAME>_<NAME>`, maiúsculo, não-alfanumérico → `_` — `research.md` D8) e para indexar `config.toml`/`secrets.toml`. |
| `secret` | `boolean` | sim | `true` ⟹ o core MUST armazenar em `$XDG_CONFIG_HOME/farol/secrets.toml` (permissão `0600`, nunca em `config.toml`) e mascarar o campo correspondente na tela de setup (§3.2). |
| `description` | `string` (não-vazia) | sim | Rótulo legível exibido como label do campo na tela de setup. |

`uptime-kuma` declara, sempre, independentemente de já estar configurado (diferente de
`capabilities`, que só declara `network` quando resolvido — `required_config` é a lista **fixa** do
que o plugin sempre precisa, inclusive na primeira execução, sem nenhum valor ainda existir):

```jsonc
[
  { "name": "base_url", "secret": false, "description": "URL base da instância Uptime Kuma" },
  { "name": "api_key", "secret": true, "description": "API Key de métricas do Uptime Kuma" }
]
```

### 1.7 Erro estruturado — forma inalterada, novo catálogo de `reason` para este plugin

Ver `contracts/error-model-delta.md` para a tabela completa. Novos valores de `data.reason`:
`not_configured` (`-32005`, agora uma salvaguarda de defesa em profundidade — o caminho primário para
"não configurado" passa a ser `PluginState = Unavailable{NotConfigured}`, decidido pelo core antes de
chamar `widget/get`, ver §2.2/§3.2 e `research.md` D8/D9 revisados), `metrics_unreachable` (`-32006`),
`metrics_parse_error` (`-32007`). Reaproveitado sem alteração: `protocol_version_incompatible`
(`-32000`, genérico, caminho de recusa do handshake). **Não reaproveitado por este plugin**:
`exec_unavailable` (`-32003`) — `uptime-kuma` não invoca mais nenhum binário externo (revisão de D8,
sem CLI `op`).

## 2. Estado interno do plugin (não é wire format — vive só no processo Python do plugin)

Diferente da feature 001 (cujo `git-local` não precisava de estado entre chamadas — cada
`widget/get` refazia a varredura do zero), o plugin `uptime-kuma` **precisa** de estado
persistente-em-memória entre a thread de polling e o handler de `widget/get` (D6, resolve FR-010).

### 2.1 `PluginConfig` (lido uma vez, no arranque do processo — revisado, `research.md` D8)

**Correção desta sessão**: o plugin não lê mais nenhum arquivo TOML nem invoca nenhum CLI externo —
os dois valores abaixo vêm de variável de ambiente, injetada pelo core no spawn do processo
(`Command::env`, `plugin_worker.rs`), seguindo a convenção `FAROL_PLUGIN_UPTIME_KUMA_<NAME
maiúsculo>` (`research.md` D8). `config.py`/`secrets.py` colapsam no mesmo mecanismo de leitura
(`os.environ.get`) — a única diferença entre os dois campos abaixo é que um é `secret: true` no
`required_config` que o próprio plugin declara (relevante só para o core montar a tela de setup,
§3.2; o plugin não precisa tratá-los diferente na leitura).

| Campo | Tipo | Descrição |
|---|---|---|
| `base_url` | `Optional[str]` | `os.environ.get("FAROL_PLUGIN_UPTIME_KUMA_BASE_URL")`. `None` se a variável estiver ausente/vazia — não há default seguro (diferente de `scan_root`, FR-008/Edge Case do spec). |
| `api_key` | `Optional[str]` | `os.environ.get("FAROL_PLUGIN_UPTIME_KUMA_API_KEY")`. `None` se a variável estiver ausente/vazia. Substitui a resolução via `op read`/1Password de versões anteriores deste documento. |

### 2.2 `not_configured` (booleano derivado, calculado uma vez no arranque — revisado, `research.md` D8/D9)

`true` se `base_url is None` **ou** `api_key is None` — tratamento unificado por FR-019 (D9 de
`research.md`), preservado desta revisão. Permanece `true` (ou `false`) pelo resto da vida do
processo — sem hot-reload de configuração nesta feature. Quando `true`: a thread de polling (§2.3)
nunca é iniciada, e todo `widget/get` subsequente devolve `error(-32005, not_configured)` —
**salvaguarda de defesa em profundidade** (§1.7): o caminho primário pelo qual o usuário percebe
"não configurado" é o core recusar avançar a conexão para `Ready` (§3.2 — `PluginState =
Unavailable{NotConfigured}` + tela de setup), sem sequer chamar `widget/get`; este cálculo interno ao
plugin só é observado na prática se essa barreira do core, por algum motivo, não pegar o caso.

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
tenta `GET ${base_url}/metrics` com `Authorization: Basic ...` (credencial lida de variável de
ambiente uma vez no arranque, §2.1), timeout HTTP de 10s (D5 de `research.md`, não normativo do protocolo); em sucesso,
parseia (regras de FR-011/FR-012, § "Parsing e mapeamento de status" em
`contracts/uptime-kuma-plugin.md`) e atualiza `last_success`; em falha (rede ou parse), atualiza
`last_error` sem tocar `last_success`.

## 3. Estado interno do core (Model do iced — não é wire format)

Estende o modelo já descrito por `specs/001-walking-skeleton-git-plugin/data-model.md` §2
(`PluginConnection`, `PluginState`, `UnavailableReason`, `WidgetRefreshTimer`) — a maior parte
reutilizada sem mudança, já que são genéricos por design (nenhum deles assume um plugin específico).
**Exceção, nesta revisão** (`research.md` D8): `UnavailableReason` ganha uma variante nova,
`NotConfigured` (§3.2), e a garantia "`Unavailable` é terminal, nenhuma transição nesta feature" —
válida para as quatro variantes já existentes (`FailedToStart`, `VersionIncompatible`, `Crashed`,
`Unresponsive`) — deixa de valer **especificamente** para `NotConfigured`, que precisa de um caminho
de volta a `Starting`/`Handshaking` depois que o usuário submete a tela de setup. Este
`data-model.md` acrescenta o análogo, para este widget, de `RepositoryViewModel` (§3.1), e o novo
estado de UI do formulário de setup (§3.2), compartilhado por design com qualquer plugin futuro que
declare `required_config`.

### 3.1 `MonitorWidgetViewModel`

| Campo | Tipo | Descrição |
|---|---|---|
| `monitors` | `MonitorStatusItem[]` | Último `items` recebido com sucesso de `widget/get` para o `widget_id` `"uptime-kuma-monitors"`. Mantido inalterado quando uma chamada de `widget/get` retorna erro pontual (`protocol/SPEC.md` §5.2, inalterado — o core, não o plugin, é quem preserva o último estado bom para exibição, exatamente como já faz para `git-local`). |
| `last_error` | `Option<PluginError>` | Preenchido quando a última `widget/get` retornou `error` (`not_configured`/`metrics_unreachable`/`metrics_parse_error`); limpo no próximo sucesso. Distinto de `PluginState::Unavailable` — um erro de leitura pontual não muda o estado geral de disponibilidade da conexão (FR-017, herdado sem mudança do mecanismo já genérico da feature 001). |

Não existe `fetch_in_flight` nem qualquer campo ligado a ação — este widget nunca tem ação associada
(FR-004), diferente de `RepositoryViewModel.fetch_in_flight` da feature 001.

### 3.2 `UnavailableReason::NotConfigured` + `SetupForm` (novo — `research.md` D8, compartilhado entre plugins)

**`UnavailableReason::NotConfigured`** (novo membro do enum já existente em `model.rs`, feature 001):
motivo pelo qual `PluginState::Unavailable` foi atingido quando o core, comparando o `required_config`
recebido no handshake contra o que conseguiu injetar como variável de ambiente no spawn, encontra ao
menos um item sem valor. Distinto das quatro variantes já existentes (`FailedToStart`,
`VersionIncompatible`, `Crashed`, `Unresponsive`) em uma propriedade importante: **não é terminal** —
é o único motivo de `Unavailable` que tem um caminho de volta (via `SetupForm` abaixo).

**`SetupForm`** (novo estado de UI, `model.rs`, vive fora de `PluginConnection` — associado à conexão
por um identificador de plugin, já que múltiplas conexões existem simultaneamente após a correção C2
da auditoria):

| Campo | Tipo | Descrição |
|---|---|---|
| `plugin_name` | `String` | Identifica a qual conexão este formulário pertence. |
| `fields` | `Vec<(RequiredConfigItem, String)>` | Um par (`item` declarado no handshake, valor digitado até agora) por item de `required_config` — inicializado com string vazia por campo; `secret: true` ⟹ a `view` MUST renderizar como campo mascarado. |

Ciclo: `PluginState::Unavailable{reason: NotConfigured, ..}` ⟹ a `view` (§ tasks de UI em `tasks.md`)
renderiza o formulário construído de `identity.required_config` em vez do widget normal daquele
plugin ⟹ usuário edita `fields` (mensagem por keystroke) ⟹ usuário confirma (mensagem de submit) ⟹
core persiste cada valor em `config.toml`/`secrets.toml` (conforme `secret` de cada item) ⟹ core
reinicia a subscription do worker deste plugin (D8 — a única forma, dentro da arquitetura atual de
`Subscription` por conexão, de fazer o processo filho subir de novo já enxergando as novas variáveis
de ambiente) ⟹ novo handshake ⟹ se todos os itens de `required_config` agora resolvem,
`PluginState::Ready`; senão, `Unavailable{NotConfigured}` de novo, com o formulário reexibido.

## 4. Regras de validação consolidadas (derivadas de FR)

- FR-004: `actions` MUST vir vazio (`[]`) tanto no handshake quanto em toda resposta de `widget/get`
  deste plugin — o core, ao processar este plugin, nunca encontra nenhuma `ActionDeclaration`
  associada a `uptime-kuma-monitors`.
- FR-005/D1/D8 (revisado): `Capability` de `kind: "network"` só aparece em `capabilities` quando o
  plugin tem, de fato, um `host` concreto para declarar (config resolvida via variável de ambiente) —
  nunca um valor vazio/placeholder só para satisfazer a forma do manifesto. A credencial não é mais
  declarada via `Capability` (não há mais `kind: "secret"`) — é declarada via `required_config`
  (§1.6.1), sempre presente independentemente de estar resolvida ou não.
- FR-008/FR-019/D8/D9 (revisado): `base_url` ausente **ou** `api_key` ausente (variável de ambiente
  não injetada) produzem o **mesmo** estado observável — primariamente `PluginState =
  Unavailable{NotConfigured}` + tela de setup (§3.2), com `error(-32005, not_configured)` de
  `widget/get` como salvaguarda — tratamento unificado, não dois estados de UI distintos entre as duas
  causas.
- FR-012: `status` de `MonitorStatusItem` só assume um dos quatro valores do enum; um valor de
  `monitor_status` fora de `{0,1,2,3}` invalida a leitura inteira daquela tentativa
  (`metrics_parse_error`), não produz um item com status desconhecido.
- FR-014/D4: `kind: "monitor-status-grid"` é um vocabulário de renderização **independente** de
  `kind: "status-grid"` — o core nunca precisa interpretar um `MonitorStatusItem` como se fosse (ou
  contivesse) um `GitRepository`, e vice-versa.
- FR-017: erro pontual de `widget/get` (qualquer um dos três novos `reason`s) MUST NOT alterar
  `PluginState` — só timeout de `RPC_TIMEOUT_CONTROL` ou morte do processo fazem essa transição
  (mecanismo genérico da feature 001, inalterado, D3 de `research.md`).
