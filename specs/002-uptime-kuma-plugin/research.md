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

> **Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: o `kind: "secret"` de `Capability`,
> desenhado abaixo, fica **obsoleto** para o propósito de declarar credencial — substituído pelo campo
> novo `required_config` (irmão de `capabilities` em `HandshakeHelloResult`; ver D8 revisado). Motivo:
> a decisão do usuário nesta sessão faz o **core**, não o plugin, gerenciar armazenamento e injeção de
> configuração/segredos — deixa de fazer sentido o manifesto de capacidades carregar uma `reference`
> opaca que o próprio core nunca resolve; o que o core precisa saber é *quais variáveis o plugin espera
> receber* (nome, se é secreta, descrição), exatamente a forma de `required_config`. O `kind: "network"`
> **não muda de papel** — continua expressando a permissão declarada de acesso de rede (host/port), só
> que agora `host`/`port` são derivados do `base_url` já resolvido **via variável de ambiente** (D8
> revisado) no momento de montar a resposta do handshake, em vez de lido de um arquivo TOML pelo
> próprio plugin. A tabela, os exemplos, o schema ilustrativo e o binding Rust abaixo foram atualizados
> para refletir isso: apenas `exec` e `network` continuam como `kind`s conhecidos desta versão do
> protocolo — `uptime-kuma`, especificamente, não declara mais nem `exec` (a chamada `op` que a
> justificava não existe mais, ver D8) nem `secret`. `CapabilityManifest.capabilities` também deixa de
> exigir `minItems: 1` — MAY ser `[]` quando nenhuma capacidade concreta é declarável (ex.:
> `uptime-kuma` sem `base_url` resolvido ainda).

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
```

| `kind` | Campos além de `kind` | Obrigatório | Notas |
|---|---|---|---|
| `exec` | nenhum | — | Mesma capacidade de hoje (`git-local`), agora como objeto de um campo só. **Não** declarada por `uptime-kuma` (D8 revisado — não invoca mais nenhum binário externo). |
| `network` | `host` (string, não-vazia) | sim | Nome de host ou IP. **Sem** campo `port` obrigatório. |
| `network` | `port` (integer, 1–65535) | não | Porta, quando fixa/conhecida (ex.: 443 para HTTPS). Ausente = não declarado. |

`kind: "secret"` **removido** do vocabulário conhecido desta versão do protocolo (revisão desta
sessão, ver callout acima e D8) — credencial passa a ser declarada via `required_config`, não via
`Capability`.

**Por que `host`/`port`, não `endpoint`/URL completa**: o Princípio IV da constitution descreve a
capacidade de rede literalmente como "acesso de rede restrito a uma **allowlist de hosts**
declarada" — não uma allowlist de URLs/caminhos. Modelar a capacidade em torno de `host` (+ `port`
opcional) mantém o campo alinhado com a unidade que um enforcement futuro (fora de escopo desta
feature, igual ao padrão já aceito para `exec`) restringiria: host:porta, não path/query. Uma URL
completa (`https://monitor.example.com/metrics?...`) carregaria informação (path, query) que a
allowlist de host não usa e que vazaria detalhe de implementação do plugin para o manifesto sem
necessidade.

**(Histórico — não se aplica mais)** O parágrafo original desta seção descrevia o formato do campo
`reference` de um `kind: "secret"` (referência de secret do CLI `op` do 1Password). Esse `kind` foi
removido nesta revisão (callout acima) — a credencial de `uptime-kuma` agora é declarada via
`required_config` (`{"name": "api_key", "secret": true, ...}`, D8 revisado), não via `Capability`. O
core continua sem jamais resolver nem ler o valor secreto por conta própria de qualquer forma — só que
agora ele **armazena e injeta** esse valor (D8), em vez de só exibir uma referência opaca.

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
        } } }
  ]
},
"CapabilityManifest": {
  "type": "object",
  "required": ["capabilities"],
  "properties": {
    "capabilities": { "type": "array", "items": { "$ref": "#/$defs/Capability" } }
  }
}
```

(`kind: "secret"` removido desta revisão — ver callout no topo de D1; `minItems: 1` também removido —
`capabilities` MAY ser `[]`.)

O binding Rust (`crates/farol-protocol/src/messages.rs`, não editado por este plano) evolui de
`pub struct CapabilityManifest { pub capabilities: Vec<String> }` para algo como:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    Exec,
    Network { host: String, #[serde(skip_serializing_if = "Option::is_none")] port: Option<u16> },
}

pub struct CapabilityManifest { pub capabilities: Vec<Capability> }
```

(Variante `Secret { reference: String }` removida desta revisão — ver callout no topo de D1.)

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

## D8 — Configuração e segredos: gerenciados pelo core, tela de setup no app `iced` (substitui a decisão de 1Password/`op` CLI)

> **Histórico desta seção**: a versão original deste documento cogitou primeiro Secret
> Service/`libsecret`, depois foi corrigida para 1Password via CLI `op` (`op read`, por `subprocess`
> do lado do *plugin*, reaproveitando a capacidade `exec`). **Esta sessão de auditoria (2026-09-01)
> substitui integralmente esse mecanismo** por decisão explícita do usuário: quem gerencia
> armazenamento seguro de configuração — secreta ou não — passa a ser o **core**, e o provisionamento
> acontece por uma tela dentro do próprio Farol (app `iced`), nunca por CLI externo nem por edição
> manual obrigatória de arquivo. Nenhum dos dois mecanismos anteriores (Secret Service, 1Password/`op`)
> é usado nesta feature. Esta correção também torna obsoleto o `kind: "secret"` de `Capability` (D1
> revisado) e o uso de `exec_unavailable` para este plugin (D9 revisado) — `uptime-kuma` não invoca
> mais nenhum binário externo, então não declara `{"kind": "exec"}`.

### Decisão — novo campo de protocolo: `required_config`

O plugin declara, no `handshake/hello`, quais variáveis de configuração precisa — secretas ou não —
sem nunca lê-las de arquivo por conta própria. Novo campo em `HandshakeHelloResult`, **irmão** de
`capabilities`/`widgets`/`actions` (não aninhado dentro de `capabilities` — capacidades continuam
expressando *permissão de acesso a sistema*; `required_config` expressa *dados que o plugin precisa
do usuário*, uma preocupação distinta):

```jsonc
"required_config": [
  { "name": "base_url", "secret": false, "description": "URL base da instância Uptime Kuma" },
  { "name": "api_key", "secret": true, "description": "API Key de métricas do Uptime Kuma" }
]
```

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `name` | `string` (não-vazia) | sim | Identificador estável da variável, escolhido pelo plugin (ex.: `"base_url"`, `"api_key"`). |
| `secret` | `boolean` | sim | `true` ⟹ o core MUST armazenar em `secrets.toml` (nunca em `config.toml`) e mascarar o campo correspondente na tela de setup. |
| `description` | `string` (não-vazia) | sim | Rótulo legível exibido como label do campo na tela de setup (ver "Decisão — tela de setup" abaixo). |

Todo item de `required_config` é, pelo próprio nome do campo, obrigatório — não há variante
"opcional" nesta versão do protocolo (se o plugin não precisa de um valor para funcionar, simplesmente
não o declara). Diferente de `capabilities` (que só declara `network` quando já resolvido, D1),
`required_config` é a lista **fixa** do que o plugin sempre precisa — é o mecanismo que permite ao
core saber o que pedir na tela de setup mesmo na primeira execução, antes de qualquer valor existir.

Binding Rust ilustrativo (`crates/farol-protocol/src/messages.rs`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredConfigItem {
    pub name: String,
    pub secret: bool,
    pub description: String,
}

pub struct HandshakeHelloResult {
    pub protocol_version: ProtocolVersion,
    pub plugin_name: String,
    pub capabilities: CapabilityManifest,
    pub required_config: Vec<RequiredConfigItem>,   // NOVO — irmão de capabilities/widgets/actions
    pub widgets: Vec<WidgetDeclaration>,
    pub actions: Vec<ActionDeclaration>,
}
```

### Decisão — armazenamento (core, Rust) — sem CLI externo, sem daemon de keyring

- **Não-secretos**: `$XDG_CONFIG_HOME/farol/plugins/<nome>/config.toml` — mesmo caminho/mecanismo já
  usado pelo `scan_root` de `git-local` (feature 001) — agora também escrito pela tela de setup, além
  de continuar editável manualmente pelo usuário.
- **Secretos**: arquivo único `$XDG_CONFIG_HOME/farol/secrets.toml`, com permissão do arquivo forçada
  a `0600` (Unix), escrito **exclusivamente** pelo core na submissão do formulário de setup. Formato:
  uma tabela por plugin, chave = `name` do `required_config`:

  ```toml
  [uptime-kuma]
  api_key = "..."
  ```

  **O plugin nunca lê este arquivo** — só o core (Rust) tem esse caminho no seu vocabulário de I/O.
- **Injeção no spawn**: em `plugin_worker.rs` (já sendo alterada por C2 do checklist de auditoria — o
  worker precisa saber qual plugin está subindo para resolver o `required_config` certo), o core
  resolve cada item de `required_config` contra `config.toml`/`secrets.toml` e injeta como variável de
  ambiente do processo filho via `Command::env(env_var_name, value)`.
- **Convenção de nome de variável de ambiente** (decisão desta sessão, para evitar colisão com o
  ambiente herdado do processo pai — `PATH`, `HOME`, etc. — e entre plugins que usem o mesmo `name`,
  ex. dois plugins com um campo `"base_url"`):

  ```text
  FAROL_PLUGIN_<PLUGIN_NAME em SNAKE_CASE MAIÚSCULO>_<NAME em SNAKE_CASE MAIÚSCULO>
  ```

  Transformação: `plugin_name`/`name` viram maiúsculo, qualquer caractere não alfanumérico vira `_`.
  Para `uptime-kuma` (`plugin_name = "uptime-kuma"`): `base_url` → `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL`;
  `api_key` → `FAROL_PLUGIN_UPTIME_KUMA_API_KEY`. **Ambos os lados** (o código Rust que injeta em
  `plugin_worker.rs` e o código Python que lê em `plugins/uptime-kuma/config.py`) aplicam a mesma
  transformação determinística — o nome prefixado nunca é transmitido por nenhum campo de protocolo,
  é derivado independentemente pelos dois lados a partir de `plugin_name`+`name`, já conhecidos por
  ambos.
- **Quando um item obrigatório de `required_config` não tem valor armazenado** (nenhuma entrada em
  `config.toml`/`secrets.toml`, ou os dois arquivos ausentes/corrompidos do lado do core): o core
  **ainda assim spawna o processo do plugin normalmente** (sem a variável de ambiente correspondente
  definida) — é assim que o plugin consegue completar o handshake e declarar `required_config` de
  volta, o que o core precisa para saber o que renderizar na tela de setup, inclusive na primeira
  execução, sem nenhum valor jamais ter existido. Depois do handshake, o core compara
  `required_config` recebido contra o que conseguiu injetar; se algo faltar, a conexão vai para
  `PluginState = Unavailable { reason: NotConfigured, .. }` (novo `UnavailableReason`, ver "Decisão —
  tela de setup" abaixo) em vez de `Ready` — o core não chama `widget/get` para essa conexão enquanto
  ela estiver neste estado (mesma regra geral já existente: `widget/get` só é disparado quando
  `PluginState == Ready`).

### Decisão — tela de setup (core, `iced`)

Quando `PluginState = Unavailable { reason: NotConfigured, .. }`, a `view` do Farol MUST renderizar,
no lugar onde o widget daquele plugin apareceria, um formulário construído a partir do
`required_config` recebido no handshake: um campo de texto por item (mascarado quando `secret: true`),
rótulo = `description`, um botão de confirmar.

Ao confirmar: os valores digitados são persistidos (`config.toml`/`secrets.toml`, conforme `secret` de
cada item) e o core precisa **reconectar** — reenviar o handshake ao mesmo plugin com as novas
variáveis de ambiente injetadas. Isto é uma capacidade nova que a máquina de estados de `PluginState`
não tinha: a feature 001 documenta `Unavailable` como terminal ("nenhuma transição nesta feature",
`model.rs`, doc de `PluginState`) — `NotConfigured` é a **primeira exceção** a essa regra, precisa de
um caminho de volta a `Starting`/`Handshaking`. Mecanicamente, a forma mais simples dentro da
arquitetura já existente (worker = `iced::Subscription` de longa duração por conexão, D5 da feature
001) é fazer a subscription reiniciar: a identidade da subscription do worker de um plugin passa a
depender de um contador de "tentativa de setup" no `Model` (`PluginConnection`); incrementar esse
contador ao processar a submissão do formulário faz o `iced` encerrar a subscription antiga (o
processo filho anterior é morto — `kill_on_drop`) e iniciar uma nova do zero, com o comando de spawn
agora enxergando as variáveis de ambiente recém-persistidas. Detalhe de implementação a confirmar na
fase de tasks, registrado aqui só para não deixar a obrigação "o core tenta reconectar" sem nenhum
mecanismo concreto.

### Decisão — lado do plugin (Python)

`config.py`/`secrets.py` deixam de fazer parsing de TOML ou de qualquer arquivo — o plugin só lê
`os.environ.get(env_var_name)` para cada variável que ele mesmo declarou em `required_config`,
aplicando a mesma convenção de nome (`FAROL_PLUGIN_...`, acima) para saber qual variável de ambiente
ler. `uptime-kuma` colapsa a leitura de config e de segredo no mesmo mecanismo — a única diferença
entre `base_url` e `api_key`, do ponto de vista do plugin, é o valor de `secret` que ele mesmo já
declarou (relevante só para a tela de setup do core, o plugin não precisa se importar com isso de
novo).

### Decisão — `not_configured` deixa de ser só um `reason` de `widget/get`

Ver D9 (revisado) para a lógica completa. Resumo: o caminho **primário** de UX passa a ser a
transição de `PluginState` para `Unavailable{NotConfigured}` (o core nem chama `widget/get` nesse
estado) — o `error(-32005, not_configured)` de `widget/get` ainda existe, mas como salvaguarda
(defesa em profundidade) para o caso em que o plugin, já rodando, constata que uma variável de
ambiente esperada está ausente/vazia apesar do core achar que estava tudo certo (ex.: divergência
entre o que o core injetou e o que o plugin declarou).

**Rationale**: atende à decisão explícita do usuário de que o core, não o plugin, é quem gerencia
armazenamento seguro — elimina a dependência de sistema externa (CLI `op`/1Password, ou qualquer
keyring do SO) que as decisões anteriores exigiam, e centraliza o provisionamento numa única UX (a
tela de setup do próprio Farol) em vez de pedir ao usuário para operar uma ferramenta de terceiros
antes de conseguir usar o Farol. Um único mecanismo de injeção (variável de ambiente) para os dois
tipos de valor (secreto e não-secreto) é mais simples do que o plugin ter dois caminhos de leitura
(arquivo para não-secreto, keyring/CLI para secreto).

**Alternativas consideradas**:
- *Manter 1Password via `op` CLI (decisão anterior deste documento)*: rejeitada — decisão explícita do
  usuário nesta sessão de auditoria; exigiria que todo usuário do Farol tivesse `op` instalado e
  autenticado só para usar um plugin de referência, um pré-requisito de ambiente desproporcional ao
  que a feature precisa entregar.
- *Secret Service/`libsecret`, pacote `keyring` do PyPI*: rejeitadas pelos mesmos motivos já registrados
  na versão anterior desta seção — dependência de keyring do sistema operacional/dependência externa
  via `pip`, nenhuma das duas necessária agora que o core gerencia o armazenamento diretamente.
- *Plugin lê `secrets.toml`/`config.toml` diretamente*: rejeitada pela decisão explícita do usuário —
  quem gerencia armazenamento seguro é o core, nunca o plugin (nem para o não-secreto: um único
  mecanismo de injeção via ambiente é mais simples que o plugin ter dois caminhos de leitura).

---

## D9 — Novos `reason`s de erro de domínio Farol; detecção unificada de "não configurado"

`protocol/SPEC.md` §10 permite explicitamente que um plugin reserve novos `reason`s dentro da faixa
`-32000`–`-32099`, documentados na seção própria daquele plugin — sem mecanismo de registro central
(§8.2: *"fora do escopo desta versão do documento formalizar um mecanismo de registro central de
reasons"*). Esta feature escolhe códigos que não colidem com os já usados por `git-local`
(`-32000`..`-32004`) por prudência, ainda que não haja garantia formal de unicidade entre plugins.

> **Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: o texto original desta seção descrevia
> `not_configured` como um `reason` de `widget/get` detectado via `op read` (1Password). Essa detecção
> muda de mecanismo com D8 revisado (armazenamento gerenciado pelo core, injeção por variável de
> ambiente) — a tabela e a lógica abaixo foram atualizadas de acordo. A mudança mais importante não é
> de valor de código (continua `-32005`), é de **papel**: `not_configured` deixa de ser o caminho
> primário de UX (a transição de `PluginState` para `Unavailable{NotConfigured}`, decidida pelo core
> antes de sequer chamar `widget/get`, é o caminho primário agora — D8) e passa a ser uma salvaguarda
> de defesa em profundidade. `exec_unavailable` (`-32003`) deixa de ter qualquer uso neste plugin —
> não há mais nenhum binário externo invocado por `uptime-kuma`.

### Decisão — tabela de novos `reason`s

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32005` | `not_configured` | `widget/get` (salvaguarda) | Variável de `required_config` ausente/vazia na *environment* do processo (`FAROL_PLUGIN_...`, D8) — coincide, na prática, tanto com "core não conseguiu injetar" quanto com "arquivo de storage do core (`config.toml`/`secrets.toml`) ausente/corrompido" (esse segundo caso normalmente nem chega a esta chamada: o core já detecta e mostra a tela de setup antes de chamar `widget/get`, ver D8). Detectado uma única vez, no arranque do processo, antes de a thread de polling (D6) começar; **toda** chamada subsequente de `widget/get` devolve este erro até o processo ser reiniciado com configuração válida (sem hot-reload nesta feature). |
| `-32006` | `metrics_unreachable` | `widget/get` | A última tentativa da thread de polling (D6) de contatar `${base_url}/metrics` falhou por motivo de rede: timeout HTTP (D5), conexão recusada, host incorreto, ou resposta HTTP não-2xx (inclui `401`/`403` de autenticação inválida — esta feature não distingue "credencial errada" de "host inacessível" dentro deste mesmo `reason`, por não haver requisito de FR-015/016 pedindo essa granularidade; um refinamento futuro poderia introduzir `metrics_auth_failed` separadamente). |
| `-32007` | `metrics_parse_error` | `widget/get` | A última tentativa obteve uma resposta HTTP, mas o corpo não é reconhecível como `/metrics` Prometheus válido do Uptime Kuma (nenhuma linha `monitor_status{...}` encontrada, ou algum valor de `monitor_status` fora de `{0,1,2,3}` — D7/parsing, Edge Case do spec). |

Reutilizados sem alteração (já normativos, `protocol/SPEC.md` §8.2):

- **`-32000` `protocol_version_incompatible`**: já genérico a qualquer plugin (caminho de recusa do
  lado do plugin no handshake) — não específico de `git-local`, reaproveitável sem qualquer mudança.

**Não reutilizados nesta feature**:
- **`-32003` `exec_unavailable`**: continua existindo no catálogo geral do protocolo (usado por
  `git-local` para o binário `git`), mas **sem uso neste plugin** desde a revisão de D8 — `uptime-kuma`
  não invoca mais nenhum binário externo (a chamada `op` que justificava reaproveitar este `reason` não
  existe mais).
- `-32001 fetch_failed`, `-32002 action_timeout` — ambos ligados a `action/invoke`, que este plugin
  nunca expõe (FR-004).

### Lógica de detecção de "não configurado" — unifica FR-008 e FR-019 (revisada, D8)

Dois níveis, não mais um só:

```text
Nível 1 — core, antes/depois do spawn (caminho primário, D8):
  1. Core spawna o processo do plugin normalmente (injeta como variável de ambiente cada item de
     required_config para o qual encontrou valor em config.toml/secrets.toml; itens sem valor
     simplesmente não viram variável de ambiente).
  2. Handshake completa normalmente — o plugin sempre responde com sucesso, declarando
     required_config (a lista é fixa, independe de haver valor ou não).
  3. Core compara required_config recebido contra o que conseguiu injetar no passo 1.
     - Algum item sem valor injetado → PluginState = Unavailable{NotConfigured}; core NÃO chama
       widget/get para esta conexão; view renderiza a tela de setup (D8) em vez do widget.
     - Todos os itens com valor → PluginState = Ready; ciclo normal de widget/get começa.

Nível 2 — dentro do próprio plugin, no handler de widget/get (salvaguarda, defesa em profundidade):
  Para cada nome declarado em required_config: lê os.environ.get(env_var_name) (convenção
  FAROL_PLUGIN_..., D8).
     - Algum valor ausente/vazio → not_configured = true; a thread de polling (D6) nunca inicia; todo
       widget/get subsequente devolve error(-32005, "not_configured").
     - Todos presentes → not_configured = false; thread de polling inicia normalmente.
  Este nível só é alcançável, na prática, se o Nível 1 já deveria ter barrado a conexão em
  Unavailable{NotConfigured} — cobre divergência entre o que o core acha que injetou e o que o plugin
  de fato recebeu (ex.: bug, ambiente do processo filho alterado por outro meio).
```

**Rationale**: FR-019 pede explicitamente tratamento unificado de "URL ausente" e "credencial
ausente" (*"Ausência de credencial... MUST receber o mesmo tratamento de estado explícito de 'não
configurado' já previsto em FR-008"*) — a decisão desta sessão preserva essa unificação e a estende:
qualquer item de `required_config` sem valor (`base_url` ou `api_key`, sem distinção de motivo)
produz o mesmo estado observável em ambos os níveis. A mudança em relação ao desenho original (D9
antes desta revisão) é só de **onde** o estado fica primariamente visível: antes, um erro pontual de
`widget/get` que o widget precisava saber renderizar distintamente de "0 monitores"; agora, uma tela
de setup dedicada, mais direta para o usuário resolver (ele digita o valor ali mesmo, sem precisar
editar arquivo).

**Alternativas consideradas**:
- *Manter só o Nível 2 (erro pontual de `widget/get`), sem gate de `PluginState` no core*: rejeitada —
  não atende à decisão do usuário de que o setup acontece "de dentro do próprio Farol" com uma tela
  dedicada; um erro de widget, por si só, não é lugar natural para um formulário de entrada de dados.
- *Sinalizar "não configurado" recusando o handshake (erro em vez de `result`)*: rejeitada — o mesmo
  motivo já registrado na versão anterior desta seção: um handshake que falha impediria até o processo
  de declarar `required_config`, que é exatamente o que o core precisa para montar a tela de setup;
  o handshake MUST completar normalmente (processo vivo, `required_config` declarado).

---

## Resumo das decisões

| # | Decisão | Resolve |
|---|---|---|
| D1 | `CapabilityManifest` evolui para `Capability[]` estruturada por `kind` (`exec`/`network`); bump `protocol_version` para `"0.2"` (MINOR, sob `MAJOR == 0`); `git-local` quebra e a migração dele fica registrada como débito técnico rastreável (issue GitHub #4, já aberta). `kind: "secret"` removido nesta revisão — ver D8. | FR-005, Clarifications Q1, constitution Governance ("Dívida técnica rastreável") |
| D2 | Framing NDJSON inalterado (reafirma D2 da feature 001) | FR-001 |
| D3 | Concorrência do core inalterada — executor tokio + worker `Subscription` (reafirma D4/D5 da feature 001); revisão desta sessão: múltiplas `PluginConnection`/subscription simultâneas (uma por plugin), não uma só (C2 do checklist de auditoria) | — |
| D4 | Novo `kind` de widget `monitor-status-grid` + `MonitorStatusItem`, não reaproveita `status-grid` como schemado hoje | FR-014, Assumptions |
| D5 | `RPC_TIMEOUT_CONTROL`/`RPC_TIMEOUT_ACTION` inalterados; timeout HTTP interno do plugin (10s) como parâmetro separado, não normativo do protocolo | FR-010 |
| D6 | Thread de polling em background + cache em memória (`last_success`/`last_error`, lock); `widget/get` só lê o cache, nunca faz I/O de rede | FR-010, FR-009, FR-015/016/017 |
| D7 | Python 3 stdlib (reafirma D3 da feature 001); `urllib.request` + Basic Auth para HTTP; parser Prometheus mínimo próprio (sem `prometheus_client`) | FR-019, FR-011, FR-016 |
| D8 | **(revisado)** Configuração/segredos gerenciados pelo **core** via `required_config` (novo campo de `HandshakeHelloResult`) + `config.toml`/`secrets.toml` (0600) + injeção por variável de ambiente no spawn (`FAROL_PLUGIN_<PLUGIN>_<NAME>`) + tela de setup no app `iced`; substitui a decisão anterior de 1Password/CLI `op`. Plugin só lê `os.environ`, nunca arquivo. | FR-019, Princípio IV |
| D9 | Novos `reason`s `-32005 not_configured` (revisado: salvaguarda, não mais caminho primário), `-32006 metrics_unreachable`, `-32007 metrics_parse_error`; `-32003 exec_unavailable` sem uso neste plugin (D8 revisado); lógica de detecção em dois níveis (core via `PluginState`, plugin via `widget/get`) | FR-008, FR-015, FR-016, FR-019 |

Nenhum item acima permanece como `NEEDS CLARIFICATION`.
