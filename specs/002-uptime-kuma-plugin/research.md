# Phase 0 Research: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Feature**: `002-uptime-kuma-plugin` | **Data**: 2026-08-31

Este documento resolve as decisões técnicas necessárias antes do design (Fase 1), no mesmo padrão
de `specs/001-walking-skeleton-git-plugin/research.md` (D1–D8). Numeração própria desta feature
(D1–D9) — quando uma decisão apenas **reafirma** uma decisão já tomada na feature 001, isso é dito
explicitamente, sem redesenhar o que já está provado.

Contexto obrigatório já lido: `spec.md` (23 FRs + `## Clarifications`, 3 decisões fechadas),
`.specify/memory/constitution.md` v0.3.0, `specs/001-walking-skeleton-git-plugin/{plan,research}.md`
e `contracts/*.md`, `protocol/SPEC.md`, os 4 `protocol/schema/v0.1/*.schema.json`,
`crates/farol-protocol/src/messages.rs`.

---

## D1 — Evolução do `CapabilityManifest`: capacidades estruturadas por `kind`, bump de protocolo, débito registrado contra `git-local`

Esta é a decisão central desta feature (FR-005 já fixou o *rumo* comportamental; esta decisão fixa
o *schema exato*).

### Decisão — formato exato

`CapabilityManifest.capabilities` deixa de ser `string[]` e passa a ser um array de objetos
`Capability`, cada um com um campo discriminador `kind` (string, obrigatório, não-vazio) e campos
adicionais específicos daquele `kind`. O vocabulário de `kind` permanece **aberto** — já era assim
para a lista de strings hoje (`protocol/SPEC.md` §10: "o vocabulário de capacidades não é fechado
por esta especificação... o core MAY simplesmente não reconhecer uma capacidade desconhecida"), e
esta evolução preserva essa propriedade: um `kind` desconhecido é um objeto `{"kind": "...", ...}`
que o core aceita, registra e exibe genericamente, sem reconhecer os campos extras.

Três `kind`s conhecidos nesta versão do protocolo:

```jsonc
{ "kind": "exec" }

{ "kind": "network", "host": "monitor.example.com", "port": 443 }

{ "kind": "secret", "reference": "op://Dev/UptimeKuma/API Keys/farol" }
```

| `kind` | Campos além de `kind` | Obrigatório | Notas |
|---|---|---|---|
| `exec` | nenhum | — | Mesma capacidade de hoje (`git-local`), agora como objeto de um campo só. |
| `network` | `host` (string, não-vazia) | sim | Nome de host ou IP. **Sem** campo `port` obrigatório. |
| `network` | `port` (integer, 1–65535) | não | Porta, quando fixa/conhecida (ex.: 443 para HTTPS). Ausente = não declarado. |
| `secret` | `reference` (string, não-vazia) | sim | Ver "Formato do `reference`" abaixo. |

**Por que `host`/`port`, não `endpoint`/URL completa**: o Princípio IV da constitution descreve a
capacidade de rede literalmente como "acesso de rede restrito a uma **allowlist de hosts**
declarada" — não uma allowlist de URLs/caminhos. Modelar a capacidade em torno de `host` (+ `port`
opcional) mantém o campo alinhado com a unidade que um enforcement futuro (fora de escopo desta
feature, igual ao padrão já aceito para `exec`) restringiria: host:porta, não path/query. Uma URL
completa (`https://monitor.example.com/metrics?...`) carregaria informação (path, query) que a
allowlist de host não usa e que vazaria detalhe de implementação do plugin para o manifesto sem
necessidade.

**Formato do `reference` de `secret`**: string opaca, escolhida e documentada pelo próprio plugin —
nunca a credencial em si (Princípio IV: segredo real nunca trafega no protocolo). Nesta feature, o
`uptime-kuma` usa uma constante fixa e documentada, no formato de referência de secret do CLI `op`
do 1Password (`"op://<vault>/<item>/<campo>"` — ver D8 para a escolha do mecanismo de keyring),
concretamente `"op://Dev/UptimeKuma/API Keys/farol"` no ambiente de referência desta feature, já que
só uma instância é configurada por vez (Assumptions do spec) — não há necessidade de tornar o
`reference` configurável pelo usuário nesta feature. O core trata este valor como
**string de exibição opaca**: não resolve, não valida formato além de não-vazio (não interpreta o
prefixo `op://` nem o parseia em vault/item/campo), não faz nenhuma chamada ao 1Password por causa
dele — mesmo padrão "declarado, não aplicado" já usado para `exec` (ver D8 abaixo para quem de fato
resolve a credencial: o **plugin**, não o core).

**Schema ilustrativo** (o que `handshake.schema.json` precisa passar a expressar; arquivo real não
é tocado por este plano):

```jsonc
"Capability": {
  "type": "object",
  "required": ["kind"],
  "properties": { "kind": { "type": "string", "minLength": 1 } },
  "additionalProperties": true,   // kind desconhecido: aceito, campos extras ignorados pelo core
  "allOf": [
    { "if": { "properties": { "kind": { "const": "exec" } } },
      "then": { "additionalProperties": false, "properties": { "kind": { "const": "exec" } } } },
    { "if": { "properties": { "kind": { "const": "network" } } },
      "then": {
        "required": ["kind", "host"], "additionalProperties": false,
        "properties": {
          "kind": { "const": "network" },
          "host": { "type": "string", "minLength": 1 },
          "port": { "type": "integer", "minimum": 1, "maximum": 65535 }
        } } },
    { "if": { "properties": { "kind": { "const": "secret" } } },
      "then": {
        "required": ["kind", "reference"], "additionalProperties": false,
        "properties": {
          "kind": { "const": "secret" },
          "reference": { "type": "string", "minLength": 1 }
        } } }
  ]
},
"CapabilityManifest": {
  "type": "object",
  "required": ["capabilities"],
  "properties": {
    "capabilities": { "type": "array", "items": { "$ref": "#/$defs/Capability" }, "minItems": 1 }
  }
}
```

O binding Rust (`crates/farol-protocol/src/messages.rs`, não editado por este plano) evolui de
`pub struct CapabilityManifest { pub capabilities: Vec<String> }` para algo como:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    Exec,
    Network { host: String, #[serde(skip_serializing_if = "Option::is_none")] port: Option<u16> },
    Secret { reference: String },
}

pub struct CapabilityManifest { pub capabilities: Vec<Capability> }
```

Nota para a fase de implementação (fora de escopo aqui): um enum `#[serde(tag = "kind")]` externamente
tagueado rejeita, por padrão, um `kind` desconhecido em vez de o preservar de forma forward-compatible
— diferente do comportamento "aceito e ignorado" que o JSON Schema acima permite. Reconciliar essa
diferença (ex.: variante `Unknown { kind: String, extra: Map<String, Value> }` via `#[serde(other)]`
combinado com uma estratégia de captura, ou desenho alternativo) é uma decisão de implementação da
tarefa que escrever `farol-protocol`, não desta sessão de planejamento — registrado aqui só para não
ser esquecido.

### Decisão — versionamento: bump para `protocol_version = "0.2"`, MINOR

Aplicando a regra de `protocol/SPEC.md` §6.4 (= `research.md` D7 da feature 001):

- **Classificação MAJOR vs. MINOR, pela letra da regra geral** ("MAJOR incrementa em... mudança de
  semântica de um campo existente"): trocar o **tipo** de `capabilities[i]` de `string` para
  `object` é, no limite, mais que uma mudança de semântica — é uma mudança de forma. Uma leitura
  estrita da regra geral apontaria para MAJOR.
- **Mas o regime vigente é `MAJOR == 0`** — e a própria justificativa da regra de comparação por
  igualdade exata em `MAJOR == 0` (D7 da feature 001, `protocol/SPEC.md` §6.4) já diz: *"a série
  `0.x` não carrega garantia de compatibilidade nem entre MINORs"*. Ou seja: sob `MAJOR == 0`, um
  bump de MINOR **já é permitido, por convenção semver explícita deste protocolo, carregar quebra
  de compatibilidade** — a igualdade exata cobre exatamente esse caso.
- **O efeito em runtime é idêntico goste-se ou não da classificação**: como a comparação sob
  `MAJOR == 0` exige igualdade de string exata, um plugin declarando `"0.1"` é rejeitado por um core
  em `"0.2"` da mesma forma que seria rejeitado por um core em `"1.0"` — a escolha entre MINOR e
  MAJOR aqui **não muda nenhum comportamento observável do core ou do plugin**.
- **Decisão**: bump para **`"0.2"`, um MINOR**. Subir para `MAJOR = 1` seria prematuro e enganoso —
  sinalizaria, por convenção semver, "o protocolo estabilizou", quando na verdade o projeto está em
  sua segunda feature de plugin e ainda claramente em fase de descoberta de forma. Manter
  `MAJOR == 0` mantém a versão honesta sobre o estágio do protocolo, enquanto a regra de igualdade
  exata já garante que a mudança não passa despercebida por nenhum lado.

**Alternativas consideradas**:
- *Bump para `MAJOR = 1` (`"1.0"`)*: rejeitada — mesmo efeito de compatibilidade (rejeição exata sob
  `MAJOR == 0` já produz o mesmo resultado que a regra geral de MAJOR produziria), mas com o custo de
  sinalizar prematuramente estabilidade que o protocolo não tem.
- *Manter `protocol_version = "0.1"` e deixar o `git-local` antigo "simplesmente quebrar" ao tentar
  deserializar o novo formato*: rejeitada — é exatamente o cenário que a checagem de versão por
  igualdade exata existe para evitar. Sem o bump, um `git-local` desatualizado passaria pela checagem
  de versão (ambos dizem `"0.1"`) e só falharia depois, ao tentar desserializar
  `capabilities: ["exec"]` contra o novo schema `Capability[]` — um erro de parse confuso em vez da
  transição limpa e já construída para `Unavailable{VersionIncompatible}` (feature 001, D6/D7).

### Consequência direta — `git-local` (feature 001) quebra, e isso é débito técnico rastreável

Esta é uma consequência **deliberada e prevista**, não um efeito colateral descoberto tarde:

- O plugin `git-local` da feature 001 declara hoje `capabilities: {"capabilities": ["exec"]}` (lista
  de strings) sob `protocol_version = "0.1"`. Um core atualizado para `"0.2"` (esta feature) recusa
  esse plugin de forma limpa e já construída (`Unavailable{VersionIncompatible}`, mensagem legível
  citando as duas versões, D7 da feature 001) — o core **não cai**, mas o `git-local` **para de
  funcionar** até ser migrado.
- Isto não é um efeito colateral secundário: **qualquer usuário que já tenha `git-local` configurado
  perde essa funcionalidade assim que atualiza para o core desta feature**, até que `git-local` seja
  atualizado para: (a) declarar `protocol_version = "0.2"`, e (b) reescrever seu
  `capabilities.capabilities` no novo formato estruturado (`[{"kind": "exec"}]` em vez de
  `["exec"]`).
- **Esse trabalho de migração do `git-local` NÃO é implementado nesta sessão** — está fora do escopo
  desta task de planejamento (a task pediu explicitamente para não tocar em `plugins/`).
- **Pela constitution v0.3.0, seção Governance, regra "Dívida técnica rastreável"** (adicionada nesta
  mesma versão da constitution): *"dívida técnica identificada durante o desenvolvimento e
  deliberadamente deixada sem correção imediata... MUST ser registrada como issue no tracker do
  projeto (GitHub Issues) antes de a mudança correspondente ser considerada concluída."* — esta é
  exatamente essa situação: a quebra de `git-local` é identificada agora, deliberadamente adiada, e
  portanto **MUST virar uma issue no GitHub antes que a feature 002 seja considerada encerrada**.
  Criar essa issue está, por instrução explícita desta task, fora do escopo desta sessão — mas o
  registro do requisito de criá-la é obrigatório neste plano (ver `plan.md` § Constitution Check e
  § Complexity Tracking/Débito Técnico).

**Alternativas consideradas** (para evitar quebrar `git-local` nesta feature):
- *Migrar `git-local` para o novo formato dentro desta mesma sessão de planejamento*: rejeitada por
  instrução explícita da task ("NÃO toque em `plugins/`", "Planeje SOMENTE a feature 002"). Também
  seria, de qualquer forma, escopo de uma sessão de *implementação*, não desta sessão de
  *planejamento*.
- *Projetar o novo `CapabilityManifest` para aceitar os dois formatos simultaneamente (string OU
  objeto por item, união)*: rejeitada — sob `MAJOR == 0` a compatibilidade retroativa entre versões
  não é uma garantia que este protocolo oferece (D7, feature 001); introduzir um formato "união" só
  para não quebrar uma única versão anterior específica adiciona complexidade permanente ao schema
  (todo consumidor futuro teria que lidar com dois formatos de item para sempre) por um benefício que
  a própria convenção de versionamento do protocolo já diz que não é esperado nesta série.

---

## D2 — Framing e transporte: NDJSON, sem mudança (reafirma D2 da feature 001)

**Decisão**: nenhuma mudança. Continua NDJSON — uma linha de JSON compacto por mensagem, terminada
por `\n`, UTF-8, sem `Content-Length`. O plugin `uptime-kuma` fala o mesmo transporte já normativo em
`protocol/SPEC.md` §4. Nenhum requisito desta feature (leitura periódica de rede, cache interno,
autenticação HTTP Basic) toca o framing — a chamada de rede acontece inteiramente **dentro** do
processo do plugin, nunca no canal NDJSON entre core e plugin.

**Rationale**: `research.md` D2 da feature 001 já avaliou NDJSON vs. `Content-Length` e decidiu por
NDJSON com justificativa que não depende de qual plugin está do outro lado — nada nesta feature
introduz payload binário, mensagem grande ou necessidade de framing diferente.

---

## D3 — Modelo de concorrência do core: reafirma executor tokio do iced + worker `Subscription` (D4/D5 da feature 001)

**Decisão**: nenhuma mudança arquitetural do lado do core. O core continua usando o executor tokio
nativo do iced (feature `tokio`, D4 da feature 001) e isolando toda I/O de plugin — spawn do
processo, leitura/escrita NDJSON, correlação de `id`, timeouts — num worker `Subscription` + canal
`mpsc` por conexão de plugin (D5 da feature 001), com o resultado voltando como `Message` para
`update`. O plugin `uptime-kuma` é só **mais uma conexão desse mesmo tipo genérico** — o core não
precisa saber que este plugin específico faz uma chamada de rede internamente; do ponto de vista do
core, `handshake/hello` e `widget/get` deste plugin são só mais duas chamadas IPC locais sujeitas ao
mesmo `RPC_TIMEOUT_CONTROL` de sempre (ver D5 abaixo — é justamente esse orçamento curto que FR-010
exige que o plugin nunca estoure por causa de rede lenta).

**Rationale**: a arquitetura de isolamento de I/O do core (D4/D5, feature 001) já foi desenhada para
ser genérica por plugin — nenhuma decisão ali assumia que só existiria um plugin git local. Nenhuma
restrição de arquitetura nova surge desta feature do lado do core; o desafio novo (não bloquear
`widget/get` na chamada de rede) é resolvido inteiramente **dentro** do processo do plugin (D6
abaixo), não no core.

**Alternativas consideradas**: nenhuma nova — a arquitetura already-decided do core é suficiente;
reabri-la seria redesenhar algo que a feature 001 já provou funcionar para o caso genérico.

---

## D4 — Novo `kind` de widget: `monitor-status-grid` (não reaproveita `status-grid` como hoje schemado)

FR-014 e as Assumptions do spec pedem explicitamente para essa decisão ser avaliada, não presumida:
*"reaproveitando o vocabulário de `kind` de widget já existente (`status-grid`) quando ele for
suficiente... a introdução de um novo `kind`... é uma decisão de design a ser tomada na fase de
planejamento."*

### Avaliação: `status-grid` é suficiente como está schemado hoje?

**Não.** `widget.schema.json` define hoje um único `WidgetItem` para o `kind: "status-grid"`, com
dois campos **obrigatórios**: `repo: GitRepository` (caminho absoluto, `dirty`, `remote_status`) e
`fetch_action: ActionDeclaration` (alvo, `enabled`, etc.). Um monitor Uptime Kuma não tem caminho de
filesystem, não tem estado "dirty", e — por FR-004 desta feature — **nunca tem uma ação associada**
(`actions` MUST ser `[]`). Preencher `fetch_action` com um valor artificial só para satisfazer o
schema violaria a honestidade do dado declarativo (Princípio III: o dado descreve o que o plugin
realmente oferece, não um placeholder para caber num contrato alheio).

### Decisão

Introduzir um novo `kind: "monitor-status-grid"`, com seu próprio item, `MonitorStatusItem`,
independente de `GitRepository`/`ActionDeclaration`:

```jsonc
{ "name": "api_example_com", "status": "up", "response_time_ms": 42 }
{ "name": "internal_service", "status": "down", "response_time_ms": null }
```

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `name` | `string` (não-vazia) | sim | Do label `monitor_name` do `/metrics` — possivelmente sanitizado (Assumptions do spec). |
| `status` | `"up" \| "down" \| "pending" \| "maintenance"` | sim | Mapeado de `monitor_status` (FR-012): `1→up`, `0→down`, `2→pending`, `3→maintenance`. |
| `response_time_ms` | `integer \| null` | sim (nullable, não ausente) | De `monitor_response_time`, quando aplicável ao status daquele monitor; `null` explícito quando não aplicável — mesmo espírito de design de `RemoteStatus.NoRemote` na feature 001 (union explícita, nunca inferida de campo ausente). |

O envelope `WidgetGetResult { widget_id, items }` **não muda de forma** — `items` continua sendo o
nome do campo, só o tipo do elemento passa a depender de qual `kind` o `widget_id` requisitado
declara (`WidgetItem` para `status-grid`, `MonitorStatusItem` para `monitor-status-grid`). O core já
sabe, desde o handshake, qual `kind` cada `widget_id` tem — não há ambiguidade em runtime sobre qual
forma esperar de volta.

**Rationale**: manter um `kind` = uma forma de item fixa e auto-suficiente é o que já torna `kind` o
mecanismo de extensão do protocolo (`protocol/SPEC.md` §5.2: *"o `kind`... é o único sinal que o core
usa para escolher como desenhar os itens recebidos"*). Transformar `status-grid` num item polimórfico
(união interna `GitRepository`-shaped | `Monitor`-shaped) reintroduziria, dentro de um único `kind`,
exatamente a mesma distinção que `kind` já existe para fazer — complexidade a mais para o core (dois
formatos de renderização sob o mesmo rótulo) sem ganho real, já que só há dois plugins conhecidos
hoje e nenhum requisito pede renderização unificada entre eles. Nada impede o **core**, internamente
(detalhe de implementação, não de protocolo), de reaproveitar código de layout visual entre os dois
`kind`s "grid de status" — isso é opaco ao contrato.

**Alternativas consideradas**:
- *Generalizar `status-grid` para um item polimórfico via discriminador interno*: rejeitada pelo
  motivo acima — duplica a função de `kind` dentro do próprio item, sem necessidade demonstrada.
- *Renomear `status-grid` para algo mais genérico e ampliar seu contrato para caber ambos os casos
  desde já*: rejeitada — mudaria o contrato do plugin `git-local` já implementado (fora do escopo
  desta feature tocar `plugins/git-local`), sem necessidade: um segundo `kind` sibling resolve o
  requisito sem tocar no primeiro.

---

## D5 — Orçamentos de timeout: `RPC_TIMEOUT_CONTROL` inalterado; timeout HTTP interno do plugin como parâmetro separado, não normativo do protocolo

**Decisão**: nenhuma mudança nos dois orçamentos definidos por `protocol/SPEC.md` §7 (D6 da feature
001):

- `RPC_TIMEOUT_CONTROL` (5s default) continua se aplicando a `handshake/hello` e `widget/get` deste
  plugin — **sem exceção e sem orçamento maior por causa da chamada de rede subjacente**: é
  precisamente essa restrição que FR-010 exige respeitar, e é resolvida projetando o plugin para
  **nunca fazer a chamada de rede dentro do handler de `widget/get`** (ver D6 abaixo) — o handler só
  lê um cache local em memória, IPC-local de fato, exatamente como o protocolo assume (`protocol/
  SPEC.md` §7.1: *"handshake/hello e widget/get são IPC local sem I/O de rede"*).
- `RPC_TIMEOUT_ACTION` **não se aplica** a este plugin — FR-004 proíbe qualquer `action/invoke`
  (`actions: []` sempre). Nenhuma decisão de orçamento de ação é necessária.

**Novo, específico deste plugin**: um **terceiro orçamento, interno ao plugin, não normativo do
protocolo** — o timeout da própria chamada HTTP contra `${base_url}/metrics`. Este orçamento:

- É um parâmetro de implementação do plugin `uptime-kuma`, não um campo do protocolo (mesmo status
  que `RPC_TIMEOUT_CONTROL`/`RPC_TIMEOUT_ACTION` já têm — "parâmetro de implementação do core... não
  é, ele mesmo, negociado por campo do protocolo", `protocol/SPEC.md` §7.1) — só que aqui o parâmetro
  vive do lado do **plugin**, não do core.
- **Decisão de valor**: **10 segundos**, deliberadamente **curto e desacoplado** do intervalo de
  polling em background (30s default, D6 abaixo) — o suficiente para não travar indefinidamente numa
  rede degradada, sem competir com o próximo ciclo de poll (ver D6 para a garantia de que nunca há
  duas chamadas HTTP concorrentes).
- Estourar este timeout **não** é reportado ao core como um estouro de `RPC_TIMEOUT_CONTROL` — é
  tratado inteiramente dentro do plugin como uma falha pontual daquela tentativa de leitura (FR-015),
  atualizando o cache interno com um erro (`metrics_unreachable`, ver D9) que só se torna visível ao
  core na próxima chamada de `widget/get`, sempre respondida dentro do orçamento de controle normal.

**Rationale**: é exatamente o gap que FR-010 identifica: a suposição original de `protocol/SPEC.md`
§7.1 (controle = IPC local, sem rede) deixa de valer *para a fonte de dados do plugin*, mas continua
valendo *para o contrato `widget/get` em si*, porque o design deste plugin garante que o handler
nunca espera a rede — ver D6.

---

## D6 — Estratégia de polling em background + cache no plugin (resolve FR-010)

Este é o desenho técnico concreto que resolve FR-010: *"a resposta do plugin a `widget/get` MUST NOT
depender, de forma síncrona, da latência da chamada de rede"*.

### Decisão

O plugin roda uma **thread dedicada em background** (`threading.Thread(daemon=True)`, stdlib —
mantém D3/D7 abaixo: sem dependência externa) que:

1. Inicia assim que a configuração (`base_url`) e a credencial (keyring, D8) são resolvidas com
   sucesso no arranque do processo — concorrente com a espera pelo `handshake/hello` do core, para
   que a primeira leitura já esteja em andamento (ou concluída) quando o primeiro `widget/get`
   chegar.
2. Em loop **estritamente sequencial** (nunca duas chamadas HTTP em voo ao mesmo tempo, por
   construção — um único laço: tenta → aguarda até o próximo tick → tenta de novo): a cada
   `suggested_refresh_interval_ms` (o **mesmo** valor que o plugin declara ao core no handshake para
   este widget — não dois conceitos de intervalo separados; default 30000ms, FR-009), faz uma
   requisição HTTP `GET ${base_url%/}/metrics` com `Authorization: Basic ...` (D7), timeout de 10s
   (D5).
3. Em caso de sucesso: parseia o corpo (D7/parsing) e grava no cache, sob um `threading.Lock`:
   `{"monitors": MonitorStatusItem[], "at": timestamp}` como o novo `last_success`; **nunca** limpa
   `last_success` por causa de uma tentativa futura falhar (dados antigos continuam disponíveis).
4. Em caso de falha (erro de rede, HTTP não-2xx, timeout, corpo não parseável): grava
   `{"reason": "metrics_unreachable" | "metrics_parse_error", "detail": "...", "at": timestamp}` como
   o novo `last_error` — **sem** apagar um `last_success` anterior.
5. **Estado inicial do cache**, antes da primeira tentativa resolver: `last_error` já pré-populado
   com `{"reason": "metrics_unreachable", "detail": "aguardando primeira leitura", "at": <start>}` e
   `last_success = None` — assim a lógica de `widget/get` (abaixo) não precisa de um terceiro estado
   especial só para "ainda não tentei" — ela já nasce coberta pelo mesmo caminho que trata "última
   tentativa falhou".

O handler de `widget/get` (rodando na thread principal, no mesmo loop de leitura de stdin que atende
o protocolo NDJSON) **nunca** faz I/O de rede — só lê o cache sob o mesmo lock:

```text
se not_configured:              devolver erro (not_configured, D9)
senão se last_success is None
       ou (last_error.at >= last_success.at):   devolver erro (last_error.reason, detail=last_error.detail)
senão:                          devolver sucesso (items = last_success.monitors)
```

**Garantia de não-sobreposição**: como o loop de polling é um único laço sequencial numa única
thread (não um agendador que dispara requisições em paralelo), nunca existe mais de uma chamada HTTP
em voo — mesmo que uma chamada demore os 10s inteiros do timeout, ela só atrasa o próximo tick, nunca
roda concorrentemente consigo mesma. Isso vale independentemente da relação entre o timeout HTTP (10s)
e o intervalo de refresh (30s default) — a garantia é estrutural, não depende dos valores escolhidos.

**Rationale**: é a tradução direta, em Python com stdlib, do mesmo princípio que fundamenta D5 da
feature 001 no lado do core ("I/O de longa duração isolada do caminho síncrono, resultado consultado
depois") — aqui aplicado dentro do próprio processo do plugin, porque é o plugin, não o core, quem
tem a nova fonte de latência (a rede até o Uptime Kuma).

**Alternativas consideradas**:
- *Fazer a chamada HTTP diretamente dentro do handler de `widget/get`, com timeout curto*: rejeitada
  diretamente por FR-010 — mesmo um timeout HTTP "curto" (ex.: 3s) ainda arrisca estourar
  `RPC_TIMEOUT_CONTROL` (5s) somado à latência de I/O local, e uma rede "lenta mas não travada"
  produziria falso positivo de "plugin travado" (exatamente o cenário que FR-010 descreve).
  Adicionalmente, uma chamada de rede síncrona dentro do handler bloquearia esse handler específico,
  mas não corromperia o processo — ainda assim, é o desenho que a spec explicitamente proíbe.
- *`asyncio` em vez de `threading`*: tecnicamente viável (um loop de eventos rodando a leitura de
  stdin e o polling como duas corrotinas), mas adiciona uma camada de abstração (event loop,
  corrotinas, `asyncio.Lock`) sem necessidade — o modelo do plugin de referência `git-local` já é
  síncrono/bloqueante por request (lê uma linha de stdin, processa, escreve uma linha, repete); uma
  thread daemon simples é a menor mudança estrutural possível sobre esse modelo já provado, e
  `threading` cobre exatamente o requisito (uma tarefa de fundo, um cache compartilhado, um lock) sem
  reescrever o loop principal do plugin em torno de `asyncio`.
- *Cache persistido em disco entre reinícios do plugin*: rejeitada — fora do escopo da spec (nenhum
  FR pede que o estado sobreviva a um restart do plugin); o cache é puramente em memória, reiniciado
  do zero a cada `spawn()` do processo, como qualquer outro estado interno do plugin nesta feature.

---

## D7 — Linguagem do plugin de referência: reafirma Python 3 stdlib (D3 da feature 001); cliente HTTP via `urllib.request`; parsing Prometheus por parser mínimo próprio

### Linguagem

**Decisão**: `plugins/uptime-kuma` continua em **Python 3.11+, apenas biblioteca padrão**, mesma
decisão e mesma justificativa de D3 (feature 001) — nada nesta feature exige sair de Python nem
introduzir dependência externa via `pip`:

- HTTP com Basic Auth: `urllib.request` (stdlib) é suficiente e trivial —

  ```python
  import urllib.request, base64

  credentials = base64.b64encode(f":{api_key}".encode()).decode()
  req = urllib.request.Request(f"{base_url.rstrip('/')}/metrics")
  req.add_header("Authorization", f"Basic {credentials}")
  with urllib.request.urlopen(req, timeout=10) as resp:
      body = resp.read().decode("utf-8")
  ```

  `urllib.request.urlopen` já aceita `timeout=` diretamente (sem dependência extra) e levanta
  `urllib.error.HTTPError` para qualquer status não-2xx (inclusive `401`, cobrindo o caso de
  credencial inválida) e `urllib.error.URLError`/`socket.timeout` para falha de conexão/timeout —
  ambos capturados e tratados como `metrics_unreachable` (D9).

- Acesso ao keyring: **não** é possível só com stdlib pura (ver D8 — decisão explícita de como isso é
  resolvido sem sair de "sem dependência `pip`").

**Rationale**: mesma de D3 da feature 001 — reforça estruturalmente (não só declarativamente) que
`protocol/SPEC.md` + os JSON Schemas bastam para qualquer linguagem implementar um plugin, evita
etapa de build, e mantém os dois plugins de referência do Farol na mesma linguagem por consistência
de quem for ler/manter ambos.

**Alternativas consideradas**: mesmas de D3 (feature 001) — Rust (rejeitada, não prova o princípio
poliglota), Go (rejeitada, toolchain adicional sem ganho). Nenhuma alternativa nova surge
especificamente da necessidade de HTTP+auth, já que `urllib` resolve isso sem sair de stdlib.

### Parsing Prometheus — parser mínimo próprio (sem `prometheus_client` do PyPI)

**Decisão**: o plugin **não** depende do pacote `prometheus_client` (PyPI, não-stdlib) para o parsing
completo do formato de exposição Prometheus. Em vez disso, implementa um parser mínimo, orientado a
linha, que reconhece **apenas** as duas famílias de métrica que FR-011 define como relevantes:

- Ignora linhas de comentário (`#`) e qualquer família de métrica diferente de `monitor_status`/
  `monitor_response_time` (ex.: `monitor_cert_days_remaining`, `monitor_uptime_ratio` — FR-011 diz
  explicitamente que o plugin MUST NOT depender delas).
- Para uma linha que começa com `monitor_status{` ou `monitor_response_time{`: extrai o bloco de
  labels entre `{` e `}` (pares `chave="valor"`, interessa `monitor_name`) e o valor numérico
  (último token da linha, separado por espaço).
- Linhas individuais malformadas dentro dessas duas famílias são **puladas** (tolerância parcial —
  não derruba o parse inteiro por uma linha ruim).
- **Falha global** (`metrics_parse_error`, FR-016) quando: (a) nenhuma linha `monitor_status{...}` é
  encontrada em todo o corpo (sinal de que a resposta não é um `/metrics` do Uptime Kuma reconhecível
  — FR-016), **ou** (b) qualquer valor de `monitor_status` encontrado está fora do conjunto
  `{0, 1, 2, 3}` — seguindo literalmente o Edge Case do spec (*"Tratado como resposta não parseável
  como esperado, mesmo tratamento de erro pontual de FR-016"*, tratado aqui como falha da resposta
  inteira daquela tentativa, não só daquele monitor individual — leitura literal do texto do Edge
  Case, que fala em "resposta", não em "monitor").
- `monitor_response_time` é lido como `float` (Prometheus gauges são tipicamente `float` na
  serialização de texto) e arredondado para `int` ao popular `response_time_ms` (campo do protocolo é
  `integer`).

**Rationale**: as duas métricas relevantes têm formato trivial e totalmente conhecido (FR-011/012 já
especificam nomes, labels e mapeamento exatos) — um parser completo de Prometheus (histogramas,
summaries, múltiplas linhas `# TYPE`/`# HELP` interpretadas semanticamente) resolveria um problema
maior do que o que esta feature tem, só para descartar quase tudo que reconheceria. Reafirma a mesma
filosofia "stdlib apenas, dependência mínima" de D3/deste D7.

**Alternativas consideradas**:
- *Dependência `prometheus_client` (PyPI)*: rejeitada — dependência externa desnecessária para
  extrair duas métricas gauge de formato conhecido; quebraria a política "stdlib apenas" sem
  benefício proporcional.
- *Expressão regular única cobrindo as duas famílias*: considerada equivalente em espírito à decisão
  acima (é uma forma de implementar o parser orientado a linha); não é uma decisão de arquitetura
  distinta, só um detalhe de implementação deixado para a fase de tasks/implementação.

---

## D8 — Acesso ao keyring: 1Password via `op` CLI (`op read`), reaproveitando a capacidade `exec` já existente

O task explicitamente pede esta decisão: bibliotecas Python de keyring (`keyring` no PyPI) não são
stdlib — introduzi-las quebraria a política "stdlib apenas" do plugin de referência (D3/D7). A
alternativa correta, e a que esta feature adota, é descrita abaixo.

> **Correção de rumo registrada nesta sessão**: a decisão originalmente cogitada para este documento
> era Secret Service/`libsecret` via `secret-tool`. O usuário indicou que a credencial real deste
> ambiente já vive no **1Password**, referenciada no formato `op://Dev/UptimeKuma/API Keys/farol`
> (formato de referência de secret do CLI `op` do 1Password — não é a senha em si, é o
> caminho/localização do item no cofre). A decisão abaixo reflete esse mecanismo corrigido; nenhuma
> outra decisão deste documento depende de qual gerenciador de segredos é usado, então nada além
> desta seção (e das referências a ela em D1/D9/plan.md) muda por causa desta correção.

### Decisão

O plugin lê a credencial diretamente do cofre **1Password**, via o **CLI oficial `op`**
(`op read "op://<vault>/<item>/<campo>"`), invocado por `subprocess` — não Secret Service/`libsecret`:

```python
import subprocess

reference = "op://Dev/UptimeKuma/API Keys/farol"  # mesmo valor declarado em capabilities (D1)

result = subprocess.run(
    ["op", "read", reference],
    capture_output=True, text=True, timeout=5,
)
if result.returncode != 0 or not result.stdout:
    # credencial não encontrada/op não autenticado -> tratado como "não configurado" (D9/FR-019)
    ...
api_key = result.stdout.rstrip("\n")
```

- Isso **é exatamente o mesmo mecanismo já usado para `git`** no plugin `git-local` (feature 001):
  invocar um binário externo do sistema via `subprocess` — coberto pela mesma capacidade `exec` já
  existente no manifesto (`{"kind": "exec"}`), sem precisar de uma capacidade nova só para isto.
- **A referência `op://vault/item/campo` é literalmente o valor do campo `reference` da capacidade
  `secret`** (D1) — uma string opaca que identifica *onde* a credencial mora, nunca a credencial em
  si; o core apenas registra/exibe esse caminho, nunca chama `op` nem resolve o valor (mesmo padrão
  "declarado, sem enforcement" já aceito para `exec`, FR-008 da feature 001).
- **Isto é leitura do plugin, não enforcement do core** — o core **nunca** lê nada de segredo nesta
  feature — ele só registra e exibe a capacidade `secret` declarada (FR-006). Quem de fato resolve a
  credencial é o **plugin**, chamando o CLI `op` do 1Password (uma ferramenta de sistema operacional,
  não uma capacidade do protocolo Farol sendo "concedida" pelo core).
- **Provisionamento da credencial** (fora do escopo desta feature implementar/automatizar — só
  documentado em `quickstart.md` como pré-requisito manual do usuário): a API Key/senha já precisa
  existir como item no cofre 1Password apontado pela referência configurada (`Dev/UptimeKuma/API
  Keys/farol` no ambiente do usuário) — o Farol/plugin não cria nem gerencia esse item, só o lê.
- **Autenticação do próprio `op`**: o CLI `op` precisa estar **instalado e autenticado** (sessão
  ativa/`op signin` já realizado, ou integração com o 1Password desktop app) no ambiente onde o
  `farol-core` (e, por consequência, o processo filho do plugin) roda — um pré-requisito de ambiente,
  não algo que o plugin provisiona. Documentado como Assumption/dependência externa (ver `plan.md` §
  Technical Context e `contracts/uptime-kuma-plugin.md`), no mesmo espírito de "binário `git` precisa
  estar no `PATH`" já assumido pela feature 001 para `git-local`.
- **Ausência do próprio binário `op` no `PATH`, ou `op` presente mas não autenticado**: ambos os casos
  fazem `op read` falhar (código de saída não-zero) — tratados, nesta feature, como o mesmo sinal que
  "referência não encontrada": reaproveitam `-32003 exec_unavailable` quando o binário `op` está
  ausente do `PATH` (mesmo `reason` já usado por `git-local` para "binário do qual o plugin depende
  não está disponível", `protocol/SPEC.md` §8.2); quando `op` está presente mas retorna erro (item
  não encontrado, sessão expirada, não autenticado) → `not_configured` (D9) — o ambiente tem a
  ferramenta certa, só falta acesso/provisionamento válido à credencial. Esta feature **não**
  distingue "sessão `op` expirada" de "item não existe no cofre" — ambos caem em `not_configured`,
  sem granularidade adicional (não pedida por FR-019).

**Rationale**: reutiliza a capacidade `exec` já provada pela feature 001 em vez de inventar uma nova
categoria de acesso a sistema; evita adicionar qualquer dependência Python via `pip` ao plugin de
referência, mantendo a política "stdlib apenas" intacta — `op` é um binário de sistema, invocado do
mesmo jeito que `git` já é; usar o mecanismo real do ambiente (1Password) em vez de um mecanismo
genérico hipotético (Secret Service) mantém a referência de capacidade `secret` verificável/testável
de fato no ambiente onde esta feature será validada.

**Alternativas consideradas**:
- *Secret Service/`libsecret` via `secret-tool`*: era a decisão original deste documento antes da
  correção de rumo acima — rejeitada porque a credencial real do ambiente vive no 1Password, não no
  keyring do Secret Service; manter essa decisão produziria um plano tecnicamente correto em
  abstrato, mas que não teria onde ler a credencial de fato neste ambiente.
- *Pacote `keyring` do PyPI (com backend 1Password ou genérico)*: rejeitada — não é stdlib, quebraria
  a política de dependência zero do plugin de referência sem necessidade, já que `op` (CLI oficial)
  resolve o mesmo problema via o mesmo mecanismo (`exec`) que o protocolo já modela.
- *Ler a credencial de uma variável de ambiente ou de um segundo arquivo de configuração próprio do
  plugin*: rejeitada — viola diretamente FR-019/Princípio IV ("segredos... NUNCA de arquivo de
  configuração em texto plano gerenciado pelo plugin"); variável de ambiente teria o mesmo problema de
  fundo (texto plano gerido fora do keyring do sistema, tipicamente herdado de um arquivo de shell
  profile igualmente em texto plano).
- *SDK oficial do 1Password para Python (`onepassword-sdk`, PyPI)*: tecnicamente mais "correto" que
  invocar um binário externo via `subprocess`, mas não é stdlib — reintroduziria exatamente a
  dependência externa que a decisão busca evitar, por um ganho (evitar um `subprocess.run`) que não
  se justifica neste escopo, já que o CLI `op` cobre o caso de uso (`op read`) sem dependência Python
  nenhuma.

---

## D9 — Novos `reason`s de erro de domínio Farol; detecção unificada de "não configurado"

`protocol/SPEC.md` §10 permite explicitamente que um plugin reserve novos `reason`s dentro da faixa
`-32000`–`-32099`, documentados na seção própria daquele plugin — sem mecanismo de registro central
(§8.2: *"fora do escopo desta versão do documento formalizar um mecanismo de registro central de
reasons"*). Esta feature escolhe códigos que não colidem com os já usados por `git-local`
(`-32000`..`-32004`) por prudência, ainda que não haja garantia formal de unicidade entre plugins.

### Decisão — tabela de novos `reason`s

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32005` | `not_configured` | `widget/get` | `base_url` ausente/vazio no arquivo de configuração (FR-008) **ou** credencial não resolvível via 1Password — `op read` executou, mas retornou erro (item não encontrado, sessão não autenticada) (FR-019). Detectado uma única vez, no arranque do processo, antes de a thread de polling (D6) começar; **toda** chamada subsequente de `widget/get` devolve este erro até o processo ser reiniciado com configuração válida (sem hot-reload nesta feature). |
| `-32006` | `metrics_unreachable` | `widget/get` | A última tentativa da thread de polling (D6) de contatar `${base_url}/metrics` falhou por motivo de rede: timeout HTTP (D5), conexão recusada, host incorreto, ou resposta HTTP não-2xx (inclui `401`/`403` de autenticação inválida — esta feature não distingue "credencial errada" de "host inacessível" dentro deste mesmo `reason`, por não haver requisito de FR-015/016 pedindo essa granularidade; um refinamento futuro poderia introduzir `metrics_auth_failed` separadamente). |
| `-32007` | `metrics_parse_error` | `widget/get` | A última tentativa obteve uma resposta HTTP, mas o corpo não é reconhecível como `/metrics` Prometheus válido do Uptime Kuma (nenhuma linha `monitor_status{...}` encontrada, ou algum valor de `monitor_status` fora de `{0,1,2,3}` — D7/parsing, Edge Case do spec). |

Reutilizados sem alteração (já normativos, `protocol/SPEC.md` §8.2):

- **`-32003` `exec_unavailable`**: reutilizado especificamente para "o binário `op` (1Password CLI)
  não está disponível no `PATH`" — distinto de `not_configured` (D8 acima já traça essa distinção:
  ambiente quebrado/ferramenta ausente vs. credencial simplesmente não resolvível pelo `op` presente).
- **`-32000` `protocol_version_incompatible`**: já genérico a qualquer plugin (caminho de recusa do
  lado do plugin no handshake) — não específico de `git-local`, reaproveitável sem qualquer mudança.

**Não reutilizados nesta feature** (não fazem sentido para um plugin sem ações): `-32001
fetch_failed`, `-32002 action_timeout` — ambos ligados a `action/invoke`, que este plugin nunca expõe
(FR-004).

### Lógica de detecção de "não configurado" — unifica FR-008 e FR-019

```text
No arranque do processo (antes de iniciar a thread de polling, D6):
  1. Lê o arquivo de configuração (mesmo padrão XDG de git-local, ver contracts/uptime-kuma-plugin.md).
     base_url ausente/vazio → not_configured = true, motivo "sem base_url".
  2. Se base_url presente: tenta `op read "op://Dev/UptimeKuma/API Keys/farol"` (referência fixa, D1).
     - binário `op` ausente do PATH → exec_unavailable (-32003), não not_configured.
     - `op` presente mas retorna erro (returncode != 0: item não encontrado, sessão não autenticada)
         → not_configured = true, motivo "credencial não resolvível via 1Password".
  3. Se ambos presentes → not_configured = false; thread de polling inicia.

Se not_configured (por qualquer um dos dois motivos): todo widget/get subsequente devolve
error(-32005, "not_configured"), permanentemente para a vida deste processo.
```

**Rationale**: FR-019 pede explicitamente esse tratamento unificado (*"Ausência de credencial
configurada no keyring MUST receber o mesmo tratamento de estado explícito de 'não configurado' já
previsto em FR-008"*) — uma única condição observável (`not_configured`) cobre as duas causas raiz,
sem exigir que quem consome o protocolo (o core, a UI) distinga "faltou URL" de "faltou credencial"
como dois estados de UI diferentes; a mensagem humana (`error.message`, nunca destinada a parsing
programático — `protocol/SPEC.md` §8) pode, e deve, ser específica sobre qual das duas causas se
aplica, para ajudar o usuário a corrigir.

**Alternativas consideradas**:
- *Dois `reason`s separados (`no_base_url`, `no_credential`)*: rejeitada — FR-019 explicitamente pede
  "o mesmo tratamento" de FR-008, não um tratamento paralelo distinto; um único `reason` com
  `message`/`data.detail` humano específico atende ao requisito sem introduzir uma distinção que a
  spec não pediu.
- *Sinalizar "não configurado" já na resposta do handshake (erro em vez de `result`)*: rejeitada — a
  Acceptance Scenario 4 de FR-008/US1 fala especificamente do **widget** reportando o estado
  ("*o widget reporta um estado explícito de 'não configurado'*"), não do handshake falhando; um
  handshake que falha impediria até o processo de ficar `Ready`/visível como plugin ativo, o que é
  mais destrutivo do que o requisito pede — o handshake MUST completar normalmente (processo vivo,
  widget declarado), só `widget/get` carrega o sinal de erro, exatamente como o padrão já
  estabelecido para `scan_root_unreadable` (`-32004`) do `git-local`.

---

## Resumo das decisões

| # | Decisão | Resolve |
|---|---|---|
| D1 | `CapabilityManifest` evolui para `Capability[]` estruturada por `kind` (`exec`/`network`/`secret`); bump `protocol_version` para `"0.2"` (MINOR, sob `MAJOR == 0`); `git-local` quebra e a migração dele fica registrada como débito técnico rastreável (issue GitHub obrigatória antes de encerrar a feature, não criada nesta sessão) | FR-005, Clarifications Q1, constitution Governance ("Dívida técnica rastreável") |
| D2 | Framing NDJSON inalterado (reafirma D2 da feature 001) | FR-001 |
| D3 | Concorrência do core inalterada — executor tokio + worker `Subscription` (reafirma D4/D5 da feature 001) | — |
| D4 | Novo `kind` de widget `monitor-status-grid` + `MonitorStatusItem`, não reaproveita `status-grid` como schemado hoje | FR-014, Assumptions |
| D5 | `RPC_TIMEOUT_CONTROL`/`RPC_TIMEOUT_ACTION` inalterados; timeout HTTP interno do plugin (10s) como parâmetro separado, não normativo do protocolo | FR-010 |
| D6 | Thread de polling em background + cache em memória (`last_success`/`last_error`, lock); `widget/get` só lê o cache, nunca faz I/O de rede | FR-010, FR-009, FR-015/016/017 |
| D7 | Python 3 stdlib (reafirma D3 da feature 001); `urllib.request` + Basic Auth para HTTP; parser Prometheus mínimo próprio (sem `prometheus_client`) | FR-019, FR-011, FR-016 |
| D8 | Credencial via **1Password** (CLI `op`, `op read "op://vault/item/campo"`) por `subprocess`, reaproveitando a capacidade `exec` já existente; core nunca resolve a credencial (declarado, não aplicado) | FR-019, Princípio IV |
| D9 | Novos `reason`s `-32005 not_configured`, `-32006 metrics_unreachable`, `-32007 metrics_parse_error`; reaproveita `-32003 exec_unavailable` para binário `op` ausente; lógica unificada de detecção "não configurado" | FR-008, FR-015, FR-016, FR-019 |

Nenhum item acima permanece como `NEEDS CLARIFICATION`.
