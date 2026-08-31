# Farol Plugin Protocol — Especificação v0.1

**Status**: Normativo. **Versão do protocolo descrita neste documento**: `0.1`.

Este documento é a fonte da verdade, agnóstica de linguagem, do protocolo de comunicação entre o
core do Farol (`farol-core`) e um processo filho de plugin. Junto com os JSON Schemas em
`protocol/schema/v0.1/*.schema.json`, ele é suficiente, por si só, para implementar um plugin
compatível em qualquer linguagem — nenhum outro artefato do repositório (incluindo qualquer crate
Rust) é normativo sobre o formato das mensagens. Um binding de linguagem específica (ex.: o crate
Rust `farol-protocol`, usado pelo core) é uma implementação *desta* especificação, nunca a origem
dela: toda mudança de protocolo é feita primeiro aqui e nos schemas; um binding é atualizado depois,
para acompanhar.

## 1. Convenções

As palavras-chave **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**,
**SHOULD NOT**, **RECOMMENDED**, **MAY** e **OPTIONAL** neste documento devem ser interpretadas
conforme RFC 2119.

Terminologia usada ao longo deste documento:

| Termo | Significado |
|---|---|
| **core** | O processo `farol-core` — quem inicia a conexão, sempre o "cliente" JSON-RPC. |
| **plugin** | O processo filho falando este protocolo pelo seu stdin/stdout — sempre o "servidor" JSON-RPC. |
| **mensagem** | Uma requisição, uma resposta de sucesso ou uma resposta de erro, cada uma correspondendo a exatamente uma linha do stream NDJSON (§3). |
| **método** | Um dos identificadores de operação definidos por esta especificação (`handshake/hello`, `widget/get`, `action/invoke`) ou por uma extensão futura compatível. |
| **conexão** | O ciclo de vida de um processo de plugin específico, do `spawn()` até sua saída/morte. `id` de requisição é único apenas dentro de uma conexão. |

## 2. Modelo de mensagens: JSON-RPC 2.0

Toda comunicação entre core e plugin usa [JSON-RPC 2.0](https://www.jsonrpc.org/specification) como
formato de mensagem. Este documento assume familiaridade com JSON-RPC 2.0 e descreve apenas as
regras específicas do Farol por cima dele.

- Toda requisição MUST conter `jsonrpc: "2.0"`, `id` (ver §2.1), `method` (string) e `params`
  (objeto).
- Toda resposta de sucesso MUST conter `jsonrpc: "2.0"`, `id` igual ao da requisição correspondente
  e `result` (objeto).
- Toda resposta de erro MUST conter `jsonrpc: "2.0"`, `id` igual ao da requisição correspondente
  (ou `null` apenas no caso — não exercitado por esta versão do protocolo — de a requisição nem ter
  sido parseável) e `error`, um objeto no formato definido em §6.
- Notificações JSON-RPC (mensagem sem `id`, sem resposta esperada) não são usadas nesta versão do
  protocolo (v0.1) — nem pelo core, nem pelo plugin. O plugin MUST NOT enviar mensagens não
  solicitadas; toda leitura de estado é iniciada pelo core (modelo de *polling*, ver `widget/get` em
  §5.2).
- O core é sempre quem envia a primeira requisição de uma conexão (§4). O plugin MUST NOT enviar
  nenhuma mensagem antes de receber e responder `handshake/hello`.

### 2.1 Correlação de requisição/resposta por `id`

- Todo request do core carrega um `id`, escolhido pelo core, do tipo `integer` ou `string`, único
  dentro da conexão (isto é, não reutilizado enquanto uma requisição anterior com o mesmo `id`
  ainda não tiver sido respondida).
- Toda resposta do plugin (sucesso ou erro) MUST ecoar exatamente o mesmo `id` da requisição que a
  originou. É assim que o core casa uma resposta assíncrona com o pedido que a gerou, quando várias
  requisições podem estar em voo.
- O core MUST descartar (logar para diagnóstico, nunca crashar) qualquer mensagem lida do stdout do
  plugin que:
  - não seja JSON válido; ou
  - seja JSON válido mas não seja uma mensagem JSON-RPC reconhecível; ou
  - referencie um `id` que o core não tem mais como pendente (já respondido antes, ou nunca
    enviado).
- Receber uma dessas mensagens descartáveis não é, por si só, tratado como "plugin indisponível"
  nesta versão do protocolo — apenas o estouro de um orçamento de timeout (§7) ou a morte do
  processo do plugin fazem essa transição.

## 3. Transporte

- O canal de transporte é **stdin/stdout do processo filho do plugin**. O core spawna o plugin como
  subprocesso; o `stdin` do plugin recebe requisições do core; o `stdout` do plugin emite respostas
  ao core.
- **stderr é livre para logging humano.** O plugin MAY escrever qualquer conteúdo em stderr (logs de
  diagnóstico, stack traces, mensagens de debug). O core MUST NOT tentar interpretar stderr como
  parte do protocolo — stderr nunca carrega uma mensagem JSON-RPC válida do ponto de vista deste
  documento, e nada do que for escrito lá afeta o estado da conexão.
- O protocolo não define nenhum outro canal de transporte (rede, socket, arquivo). Um plugin fala
  este protocolo exclusivamente pelos descritores de arquivo padrão herdados do processo que o
  spawnou.

## 4. Framing: NDJSON

- Cada mensagem JSON-RPC MUST ser serializada como **uma única linha de JSON compacto** — sem
  espaços, quebras de linha ou indentação supérfluos dentro da mensagem — seguida por exatamente um
  `\n` (LF, `0x0A`).
- Codificação: **UTF-8**.
- Não há cabeçalho de tamanho (`Content-Length` ou equivalente) e nenhum prefixo binário antes da
  linha. O delimitador de mensagem é o próprio `\n`.
- **MUST**: um serializador JSON "bonito" (pretty-printed, com quebras de linha internas) é uma
  violação do protocolo — ele introduziria bytes `\n` crus dentro do corpo de uma única mensagem,
  quebrando o framing por linha para quem lê. Tanto core quanto plugin MUST serializar cada mensagem
  em modo compacto de linha única.
- **MUST**: o leitor de cada lado (o core lendo stdout do plugin; o plugin lendo stdin do core)
  MUST ler uma linha completa (até e incluindo o `\n`) antes de tentar decodificar JSON a partir
  dela. Decodificar antes de a linha estar completa produz erro de parse espúrio.
- **MUST**: uma linha vazia (uma ocorrência de `\n` sem nenhum byte de conteúdo antes dela) MUST ser
  ignorada silenciosamente por quem lê — nem tratada como mensagem inválida, nem propagada como
  erro. Isso permite heartbeats/robustez de buffering; esta versão do protocolo não emite
  ativamente linhas vazias, mas leitores MUST tolerá-las.
- **Não é um risco de fato**: uma mensagem JSON válida e compacta nunca contém um byte `\n` cru
  dentro dela — todo `\n` que aparecer dentro de uma *string* JSON já vem escapado pela própria
  gramática de string do JSON (`\n`, duas posições de caractere: barra invertida e `n`). Ou seja,
  para JSON serializado corretamente, o risco teórico de "conteúdo com quebra de linha embutida
  quebrando o framing" não existe na prática — ele só se manifestaria a partir de uma implementação
  que já estivesse violando a regra de serialização compacta acima.

### 4.1 Exemplo (ilustrativo)

Duas mensagens consecutivas no stream de stdout do plugin, exatamente como aparecem byte a byte
(quebra de linha real após cada `}`):

```
{"jsonrpc":"2.0","id":1,"result":{"protocol_version":"0.1","plugin_name":"git-local","capabilities":{"capabilities":["exec"]},"widgets":[{"id":"repo-status","kind":"status-grid","title":"Repositórios Git"}],"actions":[]}}
{"jsonrpc":"2.0","id":2,"result":{"widget_id":"repo-status","items":[]}}
```

## 5. Métodos RPC

Esta versão (`0.1`) do protocolo define exatamente três métodos. Cada um tem seu request/response
totalmente especificado, campo a campo, no JSON Schema correspondente em
`protocol/schema/v0.1/` — este documento descreve o papel de cada método, sua sequência de uso e
seu orçamento de timeout; os schemas são a fonte normativa da forma exata de cada mensagem.

| Método | Quem inicia | Quando | Orçamento de timeout | Schema |
|---|---|---|---|---|
| `handshake/hello` | core | uma vez, logo após o `spawn()` do plugin, antes de qualquer outro método | `RPC_TIMEOUT_CONTROL` | `handshake.schema.json` |
| `widget/get` | core | a cada ciclo de refresh (periódico) para cada widget declarado | `RPC_TIMEOUT_CONTROL` | `widget.schema.json` |
| `action/invoke` | core | sob demanda, quando o usuário aciona uma ação exposta na UI | `RPC_TIMEOUT_ACTION` | `action.schema.json` |

### 5.1 `handshake/hello`

Ver §5 da tabela acima para o papel geral; a sequência completa de handshake, incluindo diagrama e
o tratamento da lista de ações, está em §6 deste documento (dedicada a ela por ser o passo
fundacional da conexão). Forma exata do request/response: `handshake.schema.json`.

### 5.2 `widget/get`

Modelo: **polling iniciado pelo core**. O core chama `widget/get` a cada ciclo de refresh —
intervalo dado por `suggested_refresh_interval_ms`, declarado pelo plugin no `widgets[]` do
handshake (§6.2), ou 30000ms (30s) na ausência desse campo. O plugin MUST NOT enviar dados de
widget sem ser perguntado: não há push/notificação nesta versão do protocolo.

Request (resumo — forma exata em `widget.schema.json`):

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "widget/get",
  "params": {
    "widget_id": "repo-status"
  }
}
```

- `widget_id` MUST ser um dos `id` declarados em `widgets[]` da resposta de `handshake/hello`
  (§6.2). O plugin MUST responder com um erro JSON-RPC padrão (`method not found`/`invalid params`
  — faixa reservada, §7.1) caso o core peça um `widget_id` desconhecido; este caso não é exercitado
  pelo plugin de referência desta versão do protocolo.

Response de sucesso (resumo):

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "widget_id": "repo-status",
    "items": [ /* ... itens específicos do widget, ver widget.schema.json ... */ ]
  }
}
```

- `items` MAY ser uma lista vazia. Ausência de dados não é, por si só, uma condição de erro.
- O conteúdo de cada item é dado puro — nenhum campo de widget carrega instrução de desenho, cor,
  layout ou markup. O `kind` declarado no handshake (§6.2) é o único sinal que o core usa para
  escolher como desenhar os itens recebidos. Um plugin MUST NOT assumir que o core sabe renderizar
  um `kind` que ele mesmo inventou: um `kind` fora do vocabulário conhecido do core tem seu widget
  ignorado (não renderizado), sem que isso derrube o plugin ou o core.
- Se o plugin responder com um erro JSON-RPC (§7) para uma chamada pontual de `widget/get`, o core
  MUST manter os últimos `items` conhecidos daquele widget e sinalizar o erro pontualmente — uma
  falha de `widget/get` isolada não muda, por si só, o estado de disponibilidade da conexão com o
  plugin (isso só acontece por timeout ou morte de processo, §7 e §8).
- O estouro do orçamento `RPC_TIMEOUT_CONTROL` numa chamada de `widget/get` do ciclo de refresh
  conta para a detecção de plugin travado (§8).

### 5.3 `action/invoke`

Invocação de uma ação previamente declarada pelo plugin (§6.3), disparada sob demanda pelo core
(tipicamente em resposta a uma interação do usuário na UI).

Request (resumo — forma exata em `action.schema.json`):

```jsonc
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "action/invoke",
  "params": {
    "action_id": "git.fetch",
    "target": { "type": "repo", "id": "/home/dev/projetos/farol" }
  }
}
```

- `action_id` MUST corresponder ao `id` de uma `ActionDeclaration` que o plugin declarou em algum
  momento anterior da conexão (no handshake ou numa resposta de `widget/get`, §6.3). O core MUST NOT
  inventar/inferir um `action_id` ou `target` por conta própria — ele apenas ecoa de volta,
  literalmente, o `target` da última `ActionDeclaration` conhecida que motivou a invocação.
- O core só envia este request quando a última `ActionDeclaration` conhecida para este
  `action_id` + `target` tinha `enabled: true`. Esta versão do protocolo não define nenhuma
  validação adicional de "ainda está habilitada" no momento em que o plugin recebe a invocação, nem
  suporte formal a múltiplas invocações concorrentes da mesma ação sobre o mesmo alvo.

Response de sucesso e response de erro estruturado: forma exata em `action.schema.json`; o modelo
de erro em si (campos `code`/`message`/`data`) é comum a todos os métodos e está em §7.

- Um erro retornado por `action/invoke` (ex.: `git fetch` falhou) MUST NOT encerrar o processo do
  plugin — é uma resposta JSON-RPC de erro normal; o plugin MUST continuar respondendo a chamadas
  subsequentes.
- `action/invoke` usa o orçamento `RPC_TIMEOUT_ACTION`, não `RPC_TIMEOUT_CONTROL` — ver §7 para a
  justificativa e para o que acontece quando esse orçamento estoura.

## 6. Handshake

O handshake é a primeira troca de mensagens de toda conexão e é obrigatório: nenhum outro método
pode ser chamado antes dele completar com sucesso.

### 6.1 Sequenciamento

O core spawna o processo do plugin e, assim que `stdin`/`stdout` estão conectados, envia o request
`handshake/hello` **primeiro** — o core é sempre quem inicia; o plugin nunca fala antes de ser
perguntado (papel simétrico ao de "client" enviando `initialize` no Language Server Protocol). O
plugin MUST responder ao `handshake/hello` antes de aceitar qualquer outra requisição.

```
core                                     plugin (processo filho)
 |--- spawn() -------------------------->|
 |--- handshake/hello (request id=1) --->|
 |                                       | (valida a versão recebida, monta seu manifesto)
 |<-- result (id=1) ---------------------|
 |
 | core compara protocol_version (§6.4)
 | se compatível  → conexão pronta, widgets/actions do handshake ficam disponíveis
 | se incompatível → core recusa este plugin, expõe mensagem legível citando ambas as versões
```

- O plugin MAY, alternativamente, responder ao `handshake/hello` com um erro JSON-RPC em vez de um
  `result` (§7), recusando por conta própria a versão proposta pelo core. Esta versão do protocolo
  reserva esse caminho para plugins futuros mais restritivos; nenhum plugin desta especificação é
  obrigado a exercitá-lo — o caminho normal, e o único usado pelo plugin de referência, é responder
  sempre com `result`, deixando para o **core** aplicar a regra de compatibilidade de §6.4 sobre o
  `protocol_version` recebido.

### 6.2 Request

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "handshake/hello",
  "params": {
    "protocol_version": "0.1",
    "core_name": "farol-core"
  }
}
```

- `protocol_version`: string `MAJOR.MINOR` (§6.4) — a versão de protocolo que o core suporta.
- `core_name`: string identificando o core; informativo, não usado por regra de compatibilidade.

### 6.3 Response de sucesso: `HandshakeHelloResult`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.1",
    "plugin_name": "git-local",
    "capabilities": {
      "capabilities": ["exec"]
    },
    "widgets": [
      {
        "id": "repo-status",
        "kind": "status-grid",
        "title": "Repositórios Git",
        "suggested_refresh_interval_ms": 30000
      }
    ],
    "actions": []
  }
}
```

- `protocol_version`: a versão de protocolo que o plugin fala (§6.4).
- `plugin_name`: identidade do plugin.
- `capabilities`: o manifesto de capacidades do plugin — um objeto com o campo `capabilities`
  (lista de strings, ex.: `"exec"` para plugins que rodam subprocessos). Esta versão do protocolo
  não define enforcement algum sobre este manifesto: o core apenas registra e exibe o que o plugin
  declara; não há distinção entre "capacidade solicitada" e "capacidade concedida".
- `widgets`: lista de declarações de widget que este plugin oferece (`WidgetDeclaration`, ver
  `handshake.schema.json`). Esta lista é congelada no momento do handshake — não muda depois, pelo
  resto da conexão.
- `actions`: lista de declarações de ação (`ActionDeclaration`, ver `handshake.schema.json`). **MAY
  ser uma lista vazia nesta resposta.** Ver §6.3.1 para o porquê.

Um plugin declara pelo menos um widget ou uma capacidade de forma consistente com o que efetivamente
oferece; esta especificação não impõe um mínimo de widgets/ações declarados — um plugin sem nenhum
widget é uma configuração válida (embora sem utilidade visível), assim como `actions: []` é sempre
válido.

#### 6.3.1 Por que `actions` pode vir vazio no handshake

A lista completa de ações de um plugin nem sempre é conhecível no instante do handshake — ela pode
depender de uma varredura ou consulta que o plugin só faz quando efetivamente perguntado (ex.:
`widget/get`). Dois desenhos foram considerados:

1. O handshake bloquear até essa descoberta completar, devolvendo `actions[]` já totalmente
   populado no `HandshakeHelloResult`. Este desenho acopla o tempo de resposta do handshake ao
   tempo da descoberta subjacente, o que pode facilmente estourar `RPC_TIMEOUT_CONTROL` (§7) em
   ambientes com muito estado para descobrir.
2. O handshake sempre poder devolver `actions: []` (plugin ainda não descobriu nada), com a lista
   real de ações viajando, a partir daí, dentro de cada resposta subsequente de `widget/get` — uma
   `ActionDeclaration` por item retornado, quando aplicável ao tipo de widget. **Este é o desenho
   adotado por esta especificação.**

Um plugin MAY, quando sua lista de ações já for conhecida sem custo no momento do handshake,
devolvê-la já populada em `actions[]` — ambos os caminhos são válidos; o core MUST tratar `actions`
do handshake como a lista de ações conhecida até aquele momento, não como a lista definitiva, e
MUST incorporar `ActionDeclaration`s adicionais entregues depois (ex.: dentro de `widget/get`) à
sua visão corrente do que o plugin oferece. Em nenhum dos dois caminhos o core infere ou constrói
uma `ActionDeclaration` por conta própria — toda ação exibida na UI MUST vir de uma declaração
explícita do plugin, entregue no handshake ou em uma atualização subsequente.

### 6.4 Versionamento

- Campo `protocol_version`: string no formato `"MAJOR.MINOR"` (ex.: `"0.1"`), declarada por **ambos
  os lados** — pelo core no request do handshake, pelo plugin no result. Não é um objeto estruturado
  — é um único campo string.
- **MAJOR** MUST incrementar em qualquer mudança incompatível de wire: remoção de um campo
  obrigatório, mudança de semântica de um campo existente, remoção de um método.
- **MINOR** MUST incrementar em uma adição compatível: um novo campo opcional, um novo método
  opcional que um consumidor mais antigo pode simplesmente ignorar.
- **`PATCH` não existe nesta versão do wire.** Correção de bug de implementação não é uma mudança de
  contrato — não há uma terceira componente na versão declarada no handshake. Um binding de
  linguagem específico (ex.: um crate publicado) MAY ter seu próprio esquema de versionamento de
  publicação; isso é versionamento do binding, não do protocolo, e as duas coisas não precisam
  coincidir.
- **Algoritmo de compatibilidade**, avaliado pelo core ao receber `HandshakeHelloResult`:

  ```
  se plugin.MAJOR == 0 (série pré-1.0):
      compatível ⟺ plugin.protocol_version == core.protocol_version   # igualdade exata
  senão (plugin.MAJOR >= 1):
      compatível ⟺ plugin.MAJOR == core.MAJOR  E  core.MINOR >= plugin.MINOR
  ```

  Na série `0.x` (onde esta versão do protocolo nasce), a convenção semver de que "`0.x` não carrega
  garantia de compatibilidade nem entre MINORs" é aplicada literalmente: exige-se igualdade exata de
  string. A partir de `1.0`, a regra geral (MAJOR igual, core conhecendo tudo que o plugin fala) MUST
  valer — nenhuma mudança de algoritmo é necessária para essa transição; o caso `MAJOR == 0` é
  tratado como um sub-caso estrito do mesmo algoritmo, não como uma regra separada.
- Em caso de incompatibilidade, o core MUST recusar a inicialização deste plugin, MUST transicionar
  a conexão para um estado terminal de "indisponível por versão incompatível" e MUST exibir uma
  mensagem legível citando ambas as versões declaradas, por exemplo:
  `"plugin 'git-local' declara protocolo 0.2; este core suporta 0.1 — versões incompatíveis"`.
- Esta especificação (v0.1) fixa `protocol_version = "0.1"` como o único valor válido esperado de
  ambos os lados enquanto esta for a versão vigente do protocolo.

## 7. Orçamentos de timeout

Toda requisição JSON-RPC enviada pelo core MUST estar sujeita a um orçamento de timeout — o core
MUST NOT esperar indefinidamente por uma resposta do plugin. Esta especificação define **dois
orçamentos distintos**, não um único orçamento genérico, porque `handshake/hello`/`widget/get` e
`action/invoke` têm perfis de latência incompatíveis:

### 7.1 `RPC_TIMEOUT_CONTROL` — 5 segundos (default)

Aplica-se a `handshake/hello` (§6) e a `widget/get` (§5.2). Ambos são IPC local sobre pipe — o
plugin não faz I/O de rede para responder a nenhum dos dois; na pior hipótese ele lê/computa estado
já disponível localmente. Um orçamento curto é apropriado: 5 segundos é folgado para uma resposta
local, e um estouro aqui é o sinal primário de que o plugin está travado (§8), não apenas lento.

Consequência do estouro:

- No handshake: a conexão vai para um estado terminal de "indisponível por não responder" — nenhum
  widget é registrado, mesmo efeito de UI de uma falha de versão incompatível (§6.4).
- Num ciclo de refresh (`widget/get`): contribui para a mesma detecção de plugin travado descrita em
  §8 — um plugin vivo no nível do sistema operacional, mas que não responde ao ciclo de vida básico
  (handshake/refresh), é tratado como indisponível.

Este orçamento é um parâmetro de implementação do core (não é, ele mesmo, negociado por campo do
protocolo) — nenhum valor numérico é normativo neste documento além do default aqui descrito.

### 7.2 `RPC_TIMEOUT_ACTION` — 120 segundos (default), ou `timeout_hint_ms` da ação

Aplica-se exclusivamente a `action/invoke` (§5.3). Ao contrário de `handshake/hello`/`widget/get`,
uma ação PODE legitimamente envolver I/O de rede (ex.: uma ação que contata um serviço remoto) e por
isso PODE demorar muito mais do que uma chamada de controle local sem que isso indique travamento —
um orçamento curto e compartilhado com o controle geraria falso-positivo de "ação travada" em
condições de rede lentas ou operações grandes.

- Default: **120000ms (120 segundos)**.
- Um plugin MAY declarar, por ação individual, um campo opcional `timeout_hint_ms` em sua
  `ActionDeclaration` (§6.3, `handshake.schema.json`/`action.schema.json`), sugerindo ao core o
  orçamento a aplicar especificamente àquela ação. Quando presente, o core MUST respeitar essa
  sugestão em vez do default de 120s. Este é o mesmo padrão "plugin sugere, core respeita, default
  na ausência" já usado por `WidgetDeclaration.suggested_refresh_interval_ms` (§6.3) — ele evita que
  o core precise adivinhar o custo de ações de plugins que não conhece de antemão.

Consequência do estouro: **diferente do orçamento de controle**, estourar `RPC_TIMEOUT_ACTION`
MUST NOT, por si só, marcar a conexão como indisponível. Uma ação genuinamente lenta (rede ruim, alvo
grande) não é o mesmo problema que um plugin travado no ciclo de vida básico. O core reporta o
estouro como um erro pontual daquela invocação específica, no mesmo formato de erro estruturado de
§8 (`data.reason: "action_timeout"`, código `-32002`, ver §8.2) — distinto do timeout de
`RPC_TIMEOUT_CONTROL` num ciclo de refresh, que é o sinal usado para marcar a conexão como
indisponível (§8).

## 8. Erros

Toda resposta de erro segue a forma JSON-RPC 2.0 padrão de um objeto `error`:

```jsonc
{
  "code": -32001,
  "message": "git fetch falhou",
  "data": {
    "reason": "fetch_failed",
    "detail": "fatal: unable to access '...': Could not resolve host"
  }
}
```

- `code` (integer, obrigatório): identificador numérico do erro.
- `message` (string, obrigatório): mensagem legível para humano/log — não é destinada a ser
  parseada por código; código MUST distinguir tipos de erro por `code`/`data.reason`, nunca por
  conteúdo de `message`.
- `data` (objeto, opcional): informação estruturada adicional. Quando presente e o erro for um dos
  códigos de domínio Farol (§8.2), `data.reason` (string) MUST estar presente e identificar a causa
  específica dentro da tabela de §8.2. Campos adicionais dentro de `data` variam por `reason` — ver
  exemplos na tabela.

Forma exata (JSON Schema) do objeto de erro: `error.schema.json`.

### 8.1 Faixa reservada JSON-RPC padrão (`-32700`..`-32600`..`-32603`)

Os códigos `-32700` (parse error), `-32600` (invalid request), `-32601` (method not found),
`-32602` (invalid params) e `-32603` (internal error) seguem a reserva padrão do JSON-RPC 2.0. Eles
são usados apenas para erros de protocolo genéricos — por exemplo, `method not found` se o core
chamar um método que o plugin não implementa. Não são o caminho de erro principal desta
especificação; os erros de domínio (§8.2) cobrem os casos esperados de operação normal.

### 8.2 Faixa de domínio Farol (`-32000` a `-32099`)

Esta faixa é reservada para códigos específicos de aplicação do ecossistema Farol. Esta versão do
protocolo define os seguintes:

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32000` | `protocol_version_incompatible` | resposta de `handshake/hello` (caminho de recusa do lado do plugin, §6.1) | O plugin recusa a versão de protocolo proposta pelo core. |
| `-32001` | `fetch_failed` | resposta de `action/invoke` | A execução subjacente da ação falhou (ex.: um `git fetch` retornou código de saída não-zero). `data.detail` MAY carregar a saída de erro/mensagem da execução subjacente. |
| `-32002` | `action_timeout` | sintetizado pelo core, não pelo plugin — quando `RPC_TIMEOUT_ACTION` (§7.2) estoura numa invocação pontual de `action/invoke` | A invocação de ação não respondeu dentro do orçamento; não implica, por si só, que a conexão esteja indisponível (§7.2). |
| `-32003` | `exec_unavailable` | resposta de `handshake/hello` ou `action/invoke` | Um binário/executável do qual o plugin depende para uma capacidade declarada (ex.: `exec`) não está disponível no ambiente. O plugin reporta isso como falha pontual daquela operação, sem encerrar seu próprio processo. |
| `-32004` | `scan_root_unreadable` | resposta de `widget/get` | Um caminho configurado que o plugin precisa ler existe mas não é acessível (erro de permissão) — distinto de "não existe/vazio", que MUST ser tratado como sucesso com dado vazio (ex.: `items: []`), nunca como erro. |

Esta tabela é normativa para o plugin de referência `git-local`. Um plugin futuro MAY reservar
outros valores de `reason` dentro da mesma faixa de `code` (`-32000`–`-32099`), desde que
documentados na seção específica desse plugin (fora do escopo desta versão do documento formalizar
um mecanismo de registro central de `reason`s — apenas a faixa de `code` está reservada aqui para
evitar colisão futura).

### 8.3 Regra geral: nenhum erro desta tabela derruba um processo

Nenhum dos erros de §8.2 MUST resultar em encerrar o processo do plugin ou do core.

- `-32000` (`protocol_version_incompatible`), quando emitido pelo plugin em vez de um `result` no
  handshake, resulta na mesma transição de "indisponível por versão incompatível" descrita em
  §6.4 — mas o processo do plugin em si continua vivo (ele simplesmente não é mais consultado pelo
  core).
- `-32001`, `-32002`, `-32003` e `-32004` são erros pontuais de uma única chamada (`widget/get` ou
  `action/invoke`) — exibidos associados ao item/ação correspondente, sem afetar o estado de
  disponibilidade geral da conexão com o plugin. Em particular, `-32003` (`exec_unavailable`) MUST
  NOT ser tratado como "plugin indisponível": o processo do plugin continua vivo e respondendo
  normalmente a outras operações; apenas a operação específica que dependia do executável ausente
  falha.
- O plugin MUST continuar aceitando e respondendo a requisições subsequentes depois de emitir
  qualquer um destes erros — nenhum deles é uma condição terminal para o processo do plugin.

## 9. Detecção de indisponibilidade (informativo aqui; normativo via §7/§8)

Esta especificação distingue dois mecanismos, independentes entre si, pelos quais o core determina
que não pode mais contar com um plugin — ambos convergindo para o mesmo tipo de estado observável
("indisponível"), mas com gatilhos diferentes:

1. **O processo do plugin termina** (crash ou saída, detectável no nível do sistema operacional,
   independentemente de haver alguma requisição em voo naquele instante).
2. **O processo continua vivo mas não responde** dentro do orçamento `RPC_TIMEOUT_CONTROL` — seja no
   handshake inicial, seja em algum ciclo de `widget/get` (§7.1).

O estouro isolado de `RPC_TIMEOUT_ACTION` (§7.2) MUST NOT, por si só, alimentar este mecanismo —
ele é sempre reportado como erro pontual de uma ação (`-32002`, §8.2), nunca como indisponibilidade
da conexão inteira. Os detalhes de implementação de como um core observa a saída do processo (ex.:
aguardar a finalização do processo concorrentemente à leitura de mensagens) são decisão de cada
implementação de core; esta especificação normatiza apenas o contrato de mensagens sobre o qual essa
decisão opera (§7, §8), não a arquitetura interna de nenhum lado.

## 10. Extensibilidade e compatibilidade futura

- Um novo campo opcional em qualquer objeto desta especificação, ou um novo método opcional que um
  consumidor mais antigo pode ignorar com segurança, MUST ser introduzido como um bump de MINOR
  (§6.4) — nunca de MAJOR.
- Remover um campo obrigatório, mudar a semântica de um campo existente, ou remover um método MUST
  ser introduzido como um bump de MAJOR.
- Um plugin MAY declarar capacidades (`capabilities`, §6.3) além de `"exec"`; o vocabulário de
  capacidades não é fechado por esta especificação. O core MAY simplesmente não reconhecer uma
  capacidade desconhecida — isso não é, por si só, um erro de protocolo.
- Um plugin MAY declarar um `kind` de widget (§6.3, §5.2) que o core não sabe renderizar; nesse
  caso o widget correspondente MUST ser ignorado silenciosamente (não renderizado), sem que isso
  afete o restante da conexão.
- Novos `reason`s de domínio (§8.2) MAY ser adicionados dentro da faixa de `code` reservada
  (`-32000`–`-32099`) por especificações futuras ou por documentação própria de um plugin, desde que
  não reutilizem um `code` já definido em §8.2 com um significado diferente.

## 11. Relação com os JSON Schemas

Este documento e os quatro arquivos abaixo, em conjunto, são a especificação completa e normativa
do protocolo v0.1. Nenhum deles é suficiente isoladamente: este documento descreve a sequência,
o transporte, o framing, o versionamento e os orçamentos de timeout; os schemas descrevem, campo a
campo, a forma exata de cada mensagem.

| Arquivo | Cobre |
|---|---|
| `protocol/schema/v0.1/handshake.schema.json` | Request/response de `handshake/hello` (§6) — inclui `WidgetDeclaration`, `ActionDeclaration`, `ActionTarget`, `CapabilityManifest`. |
| `protocol/schema/v0.1/widget.schema.json` | Request/response de `widget/get` (§5.2) — inclui `GitRepository`, `RemoteStatus`. |
| `protocol/schema/v0.1/action.schema.json` | Request/response de `action/invoke` (§5.3). |
| `protocol/schema/v0.1/error.schema.json` | O objeto de erro JSON-RPC (§8) usado por qualquer resposta de erro de qualquer um dos três métodos. |

Os quatro arquivos são JSON Schema Draft 2020-12 e são desenhados para serem carregados **em
conjunto** por um validador — cada um declara um `$id` estável, e os três primeiros referenciam
tipos uns dos outros (e todos referenciam `error.schema.json` para a forma do objeto de erro) via
`$ref` para esse `$id`, em vez de duplicar a definição. Uma ferramenta que valide apenas um dos
quatro arquivos isoladamente, sem registrar os demais pelo `$id` declarado, não conseguirá resolver
essas referências — os quatro arquivos MUST ser tratados como um conjunto único ao validar.

## 12. Referência rápida

```
Conexão:
  core spawna plugin
  core → plugin: handshake/hello           [RPC_TIMEOUT_CONTROL]
  plugin → core: result | error(-32000)
    incompatível/timeout → indisponível, nenhum widget registrado
    compatível           → conexão pronta

  a cada ciclo de refresh, para cada widget declarado:
    core → plugin: widget/get              [RPC_TIMEOUT_CONTROL]
    plugin → core: result | error(-32004, ...)
      timeout → contribui para "indisponível"
      erro pontual → item mantém último estado conhecido, erro exibido

  sob demanda, ao acionar uma ActionDeclaration com enabled=true:
    core → plugin: action/invoke           [RPC_TIMEOUT_ACTION, ou timeout_hint_ms]
    plugin → core: result | error(-32001|-32002|-32003, ...)
      nunca contribui para "indisponível"; sempre erro pontual daquela ação
```
