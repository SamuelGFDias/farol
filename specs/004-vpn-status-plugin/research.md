# Research: Plugin de Status de VPN (openfortivpn-gui)

Todas as incógnitas do `spec.md` já foram resolvidas em `/speckit-clarify` (ver
`checklists/requirements.md`). Este documento cobre as decisões técnicas necessárias para desenhar
o protocolo e a integração — não há mais nenhum `NEEDS CLARIFICATION` de produto, só decisões de
implementação.

## D1 — Contrato de origem: CLI do `openfortivpn-gui`

**Decisão**: o plugin consome exatamente o contrato já publicado por aquele projeto —
`openfortivpn-gui status|connect|disconnect [--json]`, schema em
`../../../openfortivpn-gui/specs/001-add-cli-interface/contracts/status-schema.json` e comandos em
`.../contracts/cli-commands.md` (issue #8 daquele repo, fechada pelo commit `5748830`). Sem
depender de nenhum módulo Python daquele projeto — só do binário via `subprocess`, mesmo espírito
de `git-local` chamando `git` via `subprocess` sem importar nenhuma lib do Git.

**Rationale**: constitution do Farol (Princípio II) e requisito explícito da feature (FR-010: "não
duplicar lógica de conexão/desconexão VPN") — a CLI já resolve autenticação, gerenciamento de
perfil e o próprio túnel; o plugin só traduz.

**Resumo do contrato relevante** (ver os dois arquivos acima para a forma exata):

- `status --json` → sempre exit `0`, devolve `StatusPayload` (`state`, `selected_profile`,
  `profiles[]`, `session|null`) **ou**, em falha de infraestrutura inesperada, `ErrorPayload` com
  `error.code = "internal_error"`, exit `1`.
- `connect <perfil> --json [--timeout SEGUNDOS]` → bloqueia até confirmar; sucesso devolve
  `StatusPayload` com `state: "connected"`; falha devolve `ErrorPayload` com um de
  `profile_not_found`/`already_connected`/`connect_timeout`/`sudo_denied`/`internal_error`, exit
  `1`. Timeout default documentado naquele projeto: 20s.
- `disconnect --json` → sucesso devolve `StatusPayload` com `state: "disconnected"`; falha devolve
  `ErrorPayload` com um de `not_connected`/`sudo_denied`/`internal_error`, exit `1`.

**Alternativas consideradas**: importar `controller`/`core` daquele projeto diretamente em vez de
chamar a CLI — rejeitado, pois contradiz FR-010 e o Princípio II (o plugin viraria acoplado ao
código interno de outro projeto, não a um contrato estável de processo).

## D2 — Versão do protocolo: `0.2` → `0.3`, aditiva

**Decisão**: bump de `protocol_version` para `"0.3"`. As mudanças de schema desta feição são
aditivas (novo `kind` de widget, `ActionInvokeResult` generalizado para `oneOf`) — nenhum campo
obrigatório existente muda de forma, e o wire já emitido por `git-local`/`uptime-kuma` continua
validando contra os schemas novos sem alteração nenhuma da parte deles.

**Mas**: a série `0.x` do protocolo exige **igualdade exata** de versão para compatibilidade
(`ProtocolVersion::is_compatible_with`, `crates/farol-protocol/src/version.rs` — "a série `0.x` não
carrega garantia de compatibilidade nem entre MINORs"). Isso significa que, assim que o core passar
a falar `"0.3"`, `git-local` e `uptime-kuma` — que continuam declarando `"0.2"` — ficam
`Unavailable{VersionIncompatible}` mesmo sem nenhuma mudança de wire que os afete de fato.

**Decisão explícita**: migrar a constante `PROTOCOL_VERSION` de `git-local` e `uptime-kuma` para
`"0.3"` **dentro desta mesma feature** (mudança mecânica de uma linha em cada, sem tocar mais nada
— nenhuma delas usa nenhum campo novo), em vez de deixar como dívida técnica separada.

**Rationale**: a feature 002 já passou por exatamente essa situação ao bumpar `0.1 → 0.2` — deixou
`git-local` propositalmente não migrado, virou "débito técnico #4" rastreado como issue e só foi
resolvido na T050 daquela feature, sessões depois. Repetir esse padrão pela terceira vez (agora numa
mudança que sequer *precisa* ficar pendente, já que não há migração de forma nenhuma a fazer, só o
literal da versão) não tem benefício algum — é puro custo de rastreamento adiado sem motivo. Migrar
os dois na mesma feature elimina a dívida antes dela nascer.

**Alternativas consideradas**:
- Deixar `git-local`/`uptime-kuma` como dívida técnica de novo (padrão da feature 002) — rejeitada
  pelo raciocínio acima.
- Não bumpar a versão, encaixando o novo `kind` de widget dentro de `"0.2"` — rejeitada: contradiz
  a convenção já estabelecida nas duas features anteriores de que qualquer adição de vocabulário de
  protocolo (mesmo aditiva) bumpa `MINOR`, deliberadamente, para deixar rastreável no histórico do
  protocolo qual `kind`/campo passou a existir em qual versão (`protocol/schema/v0.{1,2}/` já
  documentam isso lado a lado).

## D3 — Novo `kind` de widget: `"vpn-status"`, item singleton

**Decisão**: novo `WidgetDeclaration.kind = "vpn-status"` (widget `id: "vpn-connection"`, `title:
"VPN"`), reportando `WidgetGetResult.items` como um array de **um único** `VpnStatusItem` — não uma
lista de N itens independentes como `status-grid`/`monitor-status-grid`. `WidgetItems` (Rust) ganha
uma terceira variante `Vpn(Vec<VpnStatusItem>)`, mantendo a forma de array já estabelecida pelo
contrato de `WidgetGetResult.items` em vez de inventar uma forma de objeto único à parte.

`VpnStatusItem` carrega:

- `state`: `"disconnected" | "connecting" | "connected"` — mesmo vocabulário de `StatusPayload.state`
  da CLI (D1). **Não existe `"error"` como valor de `state`** — uma falha ao consultar o estado
  (CLI ausente do `PATH`, ou `error.code = "internal_error"` da própria CLI) é reportada como erro
  de protocolo em `widget/get` (ver D5), nunca espremida dentro do enum de `state` — mesmo
  raciocínio já usado por `RemoteStatus`/`MonitorStatus` nesta base: um estado observável distinto
  de uma falha de leitura.
- `active_profile: Option<String>` — espelha `selected_profile` da CLI (FR-002).
- `elapsed_seconds: Option<f64>` — `Some` apenas quando `state == "connected"` (espelha
  `session.elapsed_seconds` da CLI quando `session` não é `null`); explicitamente `None` quando não
  aplicável, nunca inferido de campo ausente (mesmo espírito de
  `MonitorStatusItem.response_time_ms`). Usado pela User Story 3.
- `available_profiles: Vec<VpnProfile>` — ver D4 (FR-003/FR-008).
- `disconnect_action: ActionDeclaration` — ver D4 (FR-006).

**Campos da CLI conscientemente descartados**: `session.profile` (redundante com
`active_profile`), `session.iface`, `session.started_at` — nenhum requisito funcional desta feature
precisa deles; `elapsed_seconds` sozinho cobre a User Story 3. Adicioná-los sem uso concreto seria
complexidade especulativa.

**Alternativas consideradas**: modelar como lista de repositórios de status por perfil (paralelo
literal a `status-grid`) — rejeitada, pois há no máximo uma sessão VPN ativa por vez (Assumption do
`spec.md`); um "grid" de N linhas não reflete o domínio.

## D4 — Ações por perfil, simétrico a `WidgetItem.fetch_action`

**Decisão**: `VpnProfile { name: String, connect_action: ActionDeclaration }` — um
`ActionDeclaration` (`id: "vpn.connect"`, `target: {type: "vpn-profile", id: <nome do perfil>}`) por
perfil disponível, com `enabled = (state == "disconnected")`. Um único `disconnect_action`
(`id: "vpn.disconnect"`, `target: {type: "vpn-connection", id: "active"}`,
`enabled = (state == "connected")`) no próprio `VpnStatusItem`.

**Rationale**: `ActionTarget` já é genérico (`{type: string, id: string}`,
`crates/farol-protocol/src/messages.rs`) — não precisa de nenhuma mudança de schema. O padrão
"emparelhar item de dado com a `ActionDeclaration` que opera sobre ele" já existe
(`WidgetItem.fetch_action`) e é reaproveitado aqui um nível abaixo (por perfil, não pelo item
inteiro), porque é o perfil — não o widget como um todo — que é o alvo de `vpn.connect`. Isso
também resolve FR-008 (seletor de perfil) "de graça": a `view.rs` simplesmente renderiza um botão
por `VpnProfile` quando há mais de um, sem precisar de nenhum campo de protocolo adicional para
"seleção" — a escolha do usuário É a escolha de qual `connect_action` invocar.

**Alternativas consideradas**: um único `connect_action` no nível do `VpnStatusItem`, com o nome do
perfil passado por um campo de request novo fora do `ActionTarget` já existente — rejeitada, pois
`action/invoke` já MUST ecoar o `target` literal de uma `ActionDeclaration` conhecida
(`protocol/SPEC.md` §5.3); inventar um canal de parâmetro paralelo quebraria essa invariante sem
necessidade, já que `ActionTarget.id` já é suficiente para carregar o nome do perfil.

## D5 — Catálogo de erro: dois códigos de domínio novos

**Decisão**: reaproveitar `-32003`/`exec_unavailable` (já definido, "um binário do qual o plugin
depende ... não está disponível") para quando `openfortivpn-gui` não está no `PATH` — mesmo uso já
feito por `git-local` para o binário `git`, sem precisar de um código novo.

Dois códigos de domínio novos, reservados por este plugin:

- **`-32008` / `vpn_status_unavailable`** — erro de `widget/get`: `openfortivpn-gui status --json`
  presente no `PATH` mas devolveu `ErrorPayload`/`internal_error`, ou saída não interpretável como
  JSON válido do schema esperado. `data.detail` MAY carregar a mensagem bruta da CLI.
- **`-32009` / `vpn_action_failed`** — erro de `action/invoke` (tanto `vpn.connect` quanto
  `vpn.disconnect`): a CLI devolveu `ErrorPayload`, qualquer um dos seis `error.code` possíveis
  (`profile_not_found`, `already_connected`, `not_connected`, `connect_timeout`, `sudo_denied`,
  `internal_error`). `data.detail` carrega o `error.code` bruto da CLI + `error.message`; `message`
  do `ErrorObject` do protocolo Farol carrega a tradução legível já exigida por FR-007.

**Rationale**: um único código de domínio por método (não seis) segue exatamente o precedente já
estabelecido por `-32001`/`fetch_failed` (git-local) — "a ação subjacente falhou", com o motivo
específico carregado em `data`, não em `code`/`reason` distintos por causa. Simplicidade: FR-007
exige que a mensagem seja legível, não que o código de domínio Farol seja granular por causa da
CLI — a granularidade já existe em `data.detail`.

**Alternativas consideradas**: seis códigos de domínio novos, um por `error.code` possível da CLI —
rejeitada por inflar o catálogo (`error.schema.json`) sem necessidade; nenhuma outra parte do core
precisa decidir *comportamento* diferente por causa específica, só exibir a mensagem (FR-007 não
pede tratamento condicional por tipo de erro).

## D6 — Sem poller em background; sem tela de setup

**Decisão**: `widget/get` chama `openfortivpn-gui status --json` de forma síncrona a cada
requisição, sem cache — mesmo modelo de `git-local` (que re-varre `scan_root` a cada chamada), não
o modelo de `uptime-kuma` (thread de poller + cache, porque aquele precisa desacoplar I/O de rede
potencialmente lento do ciclo de RPC). `required_config: []`, sem nenhuma tela de setup — a CLI não
pede nenhuma credencial ao Farol (Assumption do `spec.md`); o plugin fica `Ready` assim que o
handshake completa, sem passar por `Unavailable{NotConfigured}`.

**Rationale**: a chamada é local (subprocess na mesma máquina), não uma requisição de rede —
replicar a complexidade do poller de `uptime-kuma` (thread dedicada, cache, invalidação) não tem
benefício aqui e seria complexidade especulativa.

**Alternativas consideradas**: poller em background como `uptime-kuma`, para não bloquear o loop de
I/O do plugin durante o `connect` (que pode levar ~20s) — desnecessário: `connect`/`disconnect` são
invocados via `action/invoke`, um método diferente de `widget/get`, com seu próprio orçamento de
timeout (`RPC_TIMEOUT_ACTION`); um `widget/get` concorrente durante um `connect` em andamento não é
um cenário real neste protocolo (uma conexão de plugin processa um request por vez, mesma premissa
já assumida por `git-local`).

## D7 — `"conectando"` na UI: estado local de UI, não polling concorrente

**Decisão**: o estado visual "conectando" durante uma invocação de `vpn.connect` disparada pelo
próprio Farol vem de um campo de UI local — `VpnWidgetViewModel.connect_in_flight: bool` — setado ao
disparar `action/invoke` e limpo ao receber a resposta, exatamente o padrão já usado por
`RepositoryViewModel.fetch_in_flight` (`crates/farol-core/src/model.rs`) para `git.fetch`. O valor
`"connecting"` de `VpnStatusItem.state` (vindo da CLI, D3) continua existindo no protocolo para o
caso em que o estado é observado como "conectando" por uma causa externa ao Farol (ex.: GUI ou outro
processo iniciou uma conexão), mas não é o mecanismo usado pelo próprio fluxo de `action/invoke`
desta feature, já que a chamada de `connect` da CLI é bloqueante (D1) e só retorna quando resolvida.

**Rationale**: evita reinventar um mecanismo já validado nesta base (T036 da feature 002) para o
mesmo problema — "mostrar que uma ação está em andamento antes da resposta chegar" — e evita a
complexidade de coordenar polling concorrente durante uma ação bloqueante, que D6 já descartou.

**Alternativas consideradas**: nenhuma — este é o único padrão já estabelecido no código para este
problema exato.

## D8 — Nome do plugin e diretório

**Decisão**: `plugin_name: "openfortivpn-vpn"`, diretório `plugins/openfortivpn-vpn/`.

**Rationale**: segue a convenção já em uso — `uptime-kuma` é nomeado pela ferramenta que envolve
(não algo genérico como "monitoring"), não pelo termo de domínio genérico ("VPN"). Como o Farol pode
um dia ganhar outro plugin de VPN para uma ferramenta diferente, nomear pelo backend concreto evita
colisão de nome e deixa claro, só pelo nome do plugin, qual integração é essa.
