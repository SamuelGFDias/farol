# Phase 0 Research: Walking Skeleton — Core, Protocolo de Plugin e Plugin Git Local

**Feature**: `001-walking-skeleton-git-plugin` | **Data**: 2026-08-31

Este documento resolve as decisões técnicas necessárias antes do design (Fase 1). Cada decisão
cita a restrição de arquitetura (do arquiteto) ou o requisito funcional (spec) que a motiva, e
registra alternativas descartadas. Nenhuma decisão aqui contradiz a stack fixada na constitution
(Rust + iced para o core; plugin como processo filho falando JSON-RPC via stdin/stdout).

---

## D1 — Fonte da verdade do protocolo: especificação agnóstica, não crate Rust

**Decisão**: A fonte da verdade do protocolo de plugin Farol é um par de artefatos versionados,
independentes de linguagem:

1. `protocol/SPEC.md` — documento prosa descrevendo o protocolo (framing, sequência de handshake,
   métodos JSON-RPC, regras de versionamento, modelo de erro).
2. `protocol/schema/v0.x/*.schema.json` — JSON Schema (Draft 2020-12) para cada forma de mensagem
   (`handshake/hello` request/response, `widget/get` request/response, `action/invoke`
   request/response, objeto de erro).

Um crate Rust (`crates/farol-protocol`, ver D-Estrutura) PODE existir como **um binding** dessa
especificação — tipos Rust + codec de framing gerados/mantidos manualmente a partir do
`protocol/schema/`, usados pelo core (e, opcionalmente, por futuros plugins Rust). Esse crate
NUNCA é onde uma forma de mensagem nasce; toda mudança de protocolo edita primeiro o schema/doc,
depois (se aplicável) o binding Rust é atualizado para acompanhar.

**Rationale**: Restrição de arquitetura #1 e Princípio II da constitution ("um plugin PODE ser
escrito em qualquer linguagem"). Se o crate Rust fosse a fonte da verdade, o protocolo seria
Rust-cêntrico na prática — um autor de plugin em outra linguagem dependeria de ler `struct`/`enum`
Rust (ou de um gerador de bindings) para saber o formato exato das mensagens, e qualquer
ambiguidade seria resolvida "olhando o código Rust", não a especificação. Uma especificação em
JSON Schema + prosa é legível e validável a partir de qualquer linguagem sem tooling Rust.

**Prova estrutural, não só declarativa**: para evitar que essa regra vire só uma frase no
documento, o plugin de referência Git (D3) é escrito numa linguagem que **não pode importar**
`farol-protocol` — ele só tem acesso ao `protocol/SPEC.md` e ao `protocol/schema/`. Isso força, na
prática, que a especificação seja suficiente por si só para implementar um plugin.

**Alternativas consideradas**:
- *Crate Rust como fonte única, com plugins não-Rust reimplementando os tipos por inspeção do
  código*: rejeitada — é exatamente o anti-padrão que a restrição #1 proíbe.
- *IDL formal (Protobuf/Cap'n Proto) gerando bindings multi-linguagem*: rejeitada para o v0 — adiciona
  uma dependência de toolchain de geração de código a um walking skeleton cujo objetivo é provar o
  contrato mínimo; JSON Schema descreve JSON-RPC (que já é a decisão de wire format) sem exigir
  compilador de schema.

---

## D2 — Framing das mensagens: NDJSON (JSON delimitado por newline), não `Content-Length`

**Decisão**: Cada mensagem JSON-RPC é serializada em **uma única linha** de JSON compacto (sem
quebras de linha internas, sem pretty-print) e terminada por `\n` (LF). Não há cabeçalho
`Content-Length` nem qualquer prefixo binário. Codificação: UTF-8.

Essa regra é normativa e MUST estar em `protocol/SPEC.md` versionado: "toda mensagem MUST ser
serializada como JSON compacto de linha única — implementações MUST NOT usar serialização
'bonita' (pretty-printed) para o canal de transporte, porque isso introduziria bytes `\n` cruos
dentro de uma mesma mensagem."

**Rationale** (restrição de arquitetura #2 — decisão explícita e justificada):

| Critério | `Content-Length` (LSP) | NDJSON (MCP) |
|---|---|---|
| Robustez a newline embutido no payload | Robusto por construção (framing por contagem de bytes) | Robusto **condicionalmente**: só quebra se a mensagem for serializada com newline cru fora de uma string JSON — e serialização compacta de JSON válido nunca produz isso, porque todo `\n` dentro de uma string JSON já vem escapado como `\n` (duas chars: barra + n) pela própria gramática de string do JSON. Ou seja, para JSON *válido e compacto*, "conteúdo com newline embutido" nunca é um problema real — só seria se alguém serializasse errado. |
| Simplicidade de implementação em qualquer linguagem | Exige um parser de cabeçalho HTTP-like (`Content-Length: N\r\n\r\n`) antes do corpo — trivial em Rust/Go, mais cerimônia em shell/scripts | Trivial universalmente: qualquer linguagem com stdio consegue ler "uma linha por vez" (`readline`/`BufReader::read_line`/`sys.stdin.readline()`) sem parser dedicado |
| Alinhamento com precedente mais próximo do Farol | LSP negocia capacidades de editor, superfície muito maior | MCP é JSON-RPC + manifesto de capacidades + descoberta de "tools/resources" — modelo estrutural quase idêntico ao que o Farol quer (handshake, manifesto, ações declaradas) |
| Custo para o plugin de referência (Python stdlib, D3) | Precisaria implementar parsing de cabeçalho manualmente | `sys.stdin.readline()` + `json.loads()` — zero dependência extra |

Dado que a restrição #1 já exige que qualquer linguagem consiga implementar um plugin sem
tooling especial, e que o risco teórico do NDJSON (newline cru embutido) é neutralizado impondo
serialização compacta como regra do protocolo — não uma limitação de fato do formato JSON —,
NDJSON ganha em simplicidade de implementação sem perder robustez prática. **Decisão: NDJSON.**

**Alternativas consideradas**:
- *`Content-Length` (LSP)*: mais robusto a payloads com bytes arbitrários (ex.: binário
  base64-encoded gigante) sem depender de disciplina de serialização, mas essa vantagem não se
  aplica aqui — o protocolo do Farol só transporta JSON-RPC (nunca payload binário cru), e o
  walking skeleton prioriza baixa barreira de entrada para plugins poliglotas.
- *Framing por delimitador diferente de `\n` (ex.: `\0`)*: rejeitada — não oferece vantagem sobre
  `\n` e quebra compatibilidade com ferramentas de linha de comando padrão (`grep`, `jq -c`, pipes
  interativos usados para debugar o protocolo manualmente durante o desenvolvimento).

---

## D3 — Linguagem do plugin de referência: Python 3 (stdlib), não Rust

**Decisão**: `plugins/git-local` (o plugin de referência Git) é implementado em **Python 3.11+,
usando apenas a biblioteca padrão** (`json`, `subprocess`, `sys`, `tomllib` para o arquivo de
config, `pathlib`). Nenhuma dependência de `farol-protocol` nem de qualquer crate Rust.

**Rationale**: A spec (Input) diz explicitamente que o objetivo do walking skeleton é "validar o
contrato de plugin com um consumidor real". Um plugin de referência escrito em Rust, reusando
`farol-protocol`, provaria menos: o protocolo nunca seria exercitado por um consumidor que só
enxerga `protocol/SPEC.md` + JSON Schema, e um desalinhamento silencioso entre o crate Rust e a
especificação passaria despercebido (o crate sempre "bateria" com o core, porque os dois usam o
mesmo tipo Rust). Escrever o plugin de referência em Python:
- Força D1 a ser verdade na prática, não só na intenção.
- Python 3 é onipresente em máquina de dev Linux (Assumption da spec já situa o plugin rodando na
  mesma máquina do core).
- `subprocess` cobre a capacidade `exec` (rodar `git`) sem dependência externa.
- Sem etapa de build/compilação — reduz fricção para iterar no plugin de referência durante o
  desenvolvimento do walking skeleton.

**Alternativas consideradas**:
- *Rust, reusando `farol-protocol`*: rejeitada pelo motivo acima (não prova o princípio poliglota
  na prática; risco de o crate virar fonte da verdade de fato).
- *Shell script (bash) puro*: rejeitada — parsing de JSON em bash puro é frágil (sem parser JSON
  nativo confiável), e o objetivo é provar o protocolo, não testar os limites de bash.
- *Go*: também provaria o ponto, mas exige toolchain adicional (compilador Go) sem ganho sobre
  Python para este escopo; Python tem menor custo de setup no ambiente do usuário (script
  interpretado, sem binário a versionar).

---

## D4 — Runtime assíncrono: o executor nativo do iced (tokio), sem segundo runtime

**Decisão**: O core habilita a feature `tokio` do crate `iced` (`iced = { version = "0.13",
features = ["tokio"] }`), o que faz `iced::executor::Default` ser, internamente, um
`tokio::runtime::Runtime` multi-thread já instanciado e "entrado" (`enter()`) pelo próprio iced.
Toda gestão do processo filho do plugin e toda I/O assíncrona (`tokio::process::Command`,
`tokio::io::{AsyncBufReadExt, BufReader}`, `tokio::sync::mpsc`, `tokio::time::{timeout, interval}`)
roda **dentro** de `iced::Task`s e `iced::Subscription`s — nunca via
`tokio::runtime::Runtime::new()` chamado manualmente pelo core.

**Rationale** (restrição de arquitetura #3): iced já embarca um runtime tokio quando a feature
`tokio` está ativa — código assíncrono construído dentro de um `Task`/`Subscription` do iced já
executa "dentro" desse runtime (a função `enter()` do executor do iced garante que primitives
tokio-dependentes, como `tokio::process::Command::spawn()`, funcionem sem o erro clássico "there is
no reactor running"). Criar um segundo `tokio::Runtime` manualmente no mesmo processo geraria dois
runtimes concorrentes competindo por thread pool e, pior, faria com que futures tokio criadas fora
do runtime "certo" falhem ou paniquem — exatamente o problema que a restrição #3 pede para evitar.

**Alternativas consideradas**:
- *`std::thread::spawn` + runtime tokio próprio isolado para I/O de plugin, comunicando com o iced
  via canal*: tecnicamente funcionaria (dois runtimes em threads diferentes não colidem por si só),
  mas é complexidade desnecessária quando o executor do iced já é tokio — rejeitada por violar o
  espírito da restrição #3 ("não introduzir um segundo runtime concorrente") mesmo que
  tecnicamente isolável.
- *Executor não-tokio do iced (thread-pool `futures::executor`, sem feature `tokio`)*: rejeitada —
  não dá acesso a `tokio::process::Command` nem às primitivas tokio usadas para I/O do plugin sem
  reintroduzir um runtime tokio próprio (voltando ao problema acima).

---

## D5 — Não bloquear a UI: I/O de plugin fora do caminho `update`/`view`, resultado volta como `Message`

**Decisão**: A comunicação com o processo do plugin é isolada num **worker assíncrono** dedicado
por plugin, modelado como uma `iced::Subscription` de longa duração (stream) que:

1. Ao ser criada, faz `spawn()` do processo filho do plugin (`tokio::process::Command`) e guarda
   `stdin`/`stdout` como `tokio::process::{ChildStdin, ChildStdout}`.
2. Recebe pedidos de invocação (handshake, `widget/get`, `action/invoke`) por um
   `tokio::sync::mpsc::Receiver<PluginRequest>` cujo `Sender` fica no `Model` do core (`update`
   apenas faz `sender.try_send(...)` — operação não-bloqueante — nunca espera a resposta ali).
3. Escreve a requisição NDJSON em `stdin`, lê linhas de `stdout` num loop, faz parse e casa
   resposta com requisição pendente por `id` do JSON-RPC.
4. Emite cada resultado (sucesso, erro estruturado, timeout, morte do processo — ver D6) como um
   item do stream da `Subscription`, que o runtime do iced converte automaticamente em `Message`
   entregue a `update` no próximo ciclo.

`update` e `view` NUNCA chamam `.await` nem bloqueiam esperando o plugin: `update` só envia pela
`mpsc::Sender` (não-bloqueante) e reage a `Message`s de resultado que chegam depois, de forma
assíncrona — exatamente o ciclo "Elm": ação → `Task`/envio → tempo depois, `Message` → `update`
funde o resultado no `Model` → `view` re-renderiza.

**Rationale** (restrição de arquitetura #4): é a tradução direta do padrão "worker task + channel +
subscription" do iced para I/O de longa duração administrada externamente ao ciclo de vida de uma
única `Task` pontual — necessário aqui porque a conexão com o plugin (stdin/stdout do processo
filho) precisa sobreviver a múltiplos ciclos de `update` (handshake uma vez, depois N chamadas de
`widget/get` e `action/invoke` ao longo da vida do app), o que uma `Task` de execução única não
modela bem, mas uma `Subscription` de stream contínuo modela naturalmente.

**Alternativas consideradas**:
- *`iced::Task::perform` por chamada, sem worker persistente, reabrindo `stdin`/`stdout` handle a
  cada requisição*: inviável — stdin/stdout de um processo filho já em execução não são
  "reabertos"; precisam de um dono de longa duração. Rejeitada.
- *Bloquear `update` com um `futures::executor::block_on` local ao processar a ação*: rejeitada
  diretamente pela restrição #4 — bloquearia a UI durante qualquer I/O de plugin, inclusive um
  `git fetch` lento contra rede.

---

## D6 — Detecção de plugin morto vs. plugin travado: dois mecanismos independentes

**Decisão**: O worker (D5) trata as duas falhas de FR-019/FR-020 com mecanismos distintos, ambos
alimentando o mesmo estado final na UI (`PluginState::Unavailable { reason, detail }`):

1. **Processo morreu (crash / saiu)**: o worker mantém o `tokio::process::Child` e usa
   `child.wait()` numa tarefa concorrente (via `tokio::select!` junto com a leitura de
   `stdout`/recebimento de pedidos). Quando `wait()` resolve, o worker emite imediatamente
   `PluginState::Unavailable { reason: Crashed, exit_status }` — não depende de nenhuma requisição
   estar em voo.
2. **Processo vivo mas não responde (travado)**: toda requisição JSON-RPC enviada ao plugin é
   envolvida em `tokio::time::timeout(RPC_TIMEOUT, aguardar_resposta_por_id)`. Se o timeout
   expira, o worker trata aquela chamada como falha (retorna erro estruturado só para quem chamou,
   se for uma ação pontual) e, cumulativamente, se **o handshake inicial** ou **qualquer requisição
   de refresh do ciclo periódico** expira, o worker marca o plugin como
   `Unavailable { reason: Unresponsive }` — porque um plugin que não responde ao ciclo de vida
   básico (refresh) é, na prática, tão inútil quanto um plugin morto, mesmo que o processo SO ainda
   exista.

   `RPC_TIMEOUT` default: **5 segundos**. Não há requisito numérico na spec para esse valor; é uma
   decisão de engenharia deste plano (IPC local via pipe, sem rede — 5s já é folgado para
   `git fetch` local rodar ou falhar rápido; timeouts muito curtos gerariam falso-positivo de
   "travado" em repositórios grandes). Documentado aqui para ser revisitado se a experiência de uso
   mostrar que é curto/longo demais; não é normativo do protocolo (o protocolo em si não exige um
   timeout específico, só que o core não trave — o valor é um parâmetro de implementação do core).

Ambos os casos convergem no mesmo estado de UI "indisponível" (a spec, em FR-020, só exige que o
estado seja "visível e distinguível de carregando/sem dados" — não exige distinguir crash de
travamento na UI). A distinção interna (`reason`) é mantida para logs/diagnóstico, não é requisito
de UI desta feature.

**Rationale** (restrição de arquitetura #5): as duas falhas têm sinais completamente diferentes —
uma é observável no nível do SO (`exit status`), a outra só é observável pela ausência de resposta
dentro de um prazo no nível da aplicação. Um único mecanismo (ex.: só timeout) não detectaria um
crash imediato de forma mais rápida que o timeout cheio; um único mecanismo (só `wait()`) nunca
detectaria travamento porque o processo continua vivo. Por isso os dois mecanismos coexistem.

**Alternativas consideradas**:
- *Só timeout de RPC, sem observar `child.wait()` separadamente*: rejeitada — um crash real ficaria
  escondido atrás do `RPC_TIMEOUT` inteiro antes de ser sinalizado, mesmo quando o SO já sabe
  (via exit status) que o processo morreu no instante 0.
- *Heartbeat periódico dedicado (`ping`/`pong`) independente do ciclo de refresh*: rejeitada para o
  v0 — o próprio ciclo de refresh de 30s (FR-011) já funciona como heartbeat de fato (é uma
  requisição JSON-RPC regular); adicionar um método `ping` dedicado é complexidade extra sem
  benefício adicional neste escopo. Pode ser adicionado numa versão MINOR futura do protocolo sem
  quebrar compatibilidade.

---

## D7 — Versionamento do protocolo: `MAJOR.MINOR`, string, comparação assimétrica pós-v1

**Decisão**: A versão do protocolo é uma string `"MAJOR.MINOR"` (ex.: `"0.1"`), declarada por
**ambos os lados** no handshake (campo `protocol_version` tanto no request do core quanto no
response do plugin — ver `contracts/handshake.md`). Regras, já definidas para toda a vida do
protocolo (não só para o v0), registradas em `protocol/SPEC.md`:

- **MAJOR** incrementa em qualquer mudança incompatível de wire (remover campo obrigatório, mudar
  semântica de um campo existente, remover um método).
- **MINOR** incrementa em adição compatível (novo campo opcional, novo método opcional que um
  cliente antigo pode ignorar).
- **Comparação, caso geral (MAJOR ≥ 1)**: compatível se `plugin.MAJOR == core.MAJOR` **e**
  `core.MINOR >= plugin.MINOR` (o core precisa conhecer tudo que o plugin fala; o plugin pode
  conhecer menos do que o core sabe fazer, isso é sempre seguro).
- **Caso especial `MAJOR == 0`** (pré-1.0, que é onde este walking skeleton nasce): por convenção
  semver, a série `0.x` não carrega garantia de compatibilidade nem entre MINORs — então, enquanto
  `MAJOR == 0`, a regra é **igualdade exata** `plugin.protocol_version == core.protocol_version`.
  Assim que o protocolo amadurecer para `1.0`, a regra geral acima passa a valer automaticamente
  (nenhuma mudança de código é necessária — o algoritmo de comparação já é o mesmo, só o caso
  `MAJOR == 0` é tratado como sub-caso mais estrito).
- Em qualquer incompatibilidade, o core MUST recusar a inicialização desse plugin (FR-005) e expor
  uma mensagem legível citando as duas versões (ex.: `"plugin declara protocolo 0.2, core suporta
  0.1 — versões incompatíveis"`).

Este walking skeleton nasce declarando `protocol_version = "0.1"` em ambos os lados.

**Rationale** (restrição de arquitetura #6): FR-004/FR-005 exigem negociação com falha legível; a
spec não define o esquema de versão, então esta é uma decisão de plano. Um esquema `MAJOR.MINOR`
com regra de comparação definida desde já (em vez de "vamos decidir quando tivermos v1") evita
retrabalho: a regra pós-1.0 já está escrita, só dormente enquanto `MAJOR == 0`.

**Alternativas consideradas**:
- *Inteiro único incremental (`protocol_version: 1`)*: mais simples, mas colapsa "breaking" e
  "aditivo" na mesma dimensão — todo bump vira potencialmente breaking, o que penaliza evolução
  aditiva do protocolo (ex.: adicionar um método novo obrigaria replugar todo plugin existente).
  Rejeitada.
- *SemVer completo (`MAJOR.MINOR.PATCH`)*: `PATCH` nunca afeta o wire format por definição (é só
  correção de bug de implementação, não de contrato) — incluir `PATCH` na versão *declarada no
  handshake* adicionaria uma dimensão que a comparação de compatibilidade sempre ignoraria.
  Simplificado para `MAJOR.MINOR` no wire; o *crate* `farol-protocol` pode ter seu próprio SemVer de
  publicação (isso é versionamento de binding, não de protocolo — não precisam coincidir).

---

## D8 — Modelo de atualização do widget: polling core-iniciado, não push do plugin

**Decisão**: O core é quem inicia toda leitura de estado do widget, chamando o método
`widget/get` a cada ciclo de refresh (default 30s, ou o intervalo sugerido pelo plugin no
handshake — FR-011). O plugin nunca envia notificação JSON-RPC não solicitada (`push`) nesta
feature.

**Rationale**: FR-011 já descreve literalmente um "ciclo de refresh periódico" iniciado pelo core,
não um modelo de push do plugin. Polling mantém o protocolo estritamente request/response nesta
versão (mais simples de implementar em qualquer linguagem — não exige que o plugin saiba
*quando* mandar uma notificação nem que o core trate mensagens não solicitadas), e não fecha a
porta para push futuro: um método/notificação adicional (`widget/changed`) pode ser somado como
MINOR bump sem quebrar quem só faz polling.

**Alternativas consideradas**: *Push do plugin via notificação JSON-RPC sempre que o estado muda*
— mais responsivo (sem esperar até 30s para refletir uma mudança externa ao Farol), mas exige que
o plugin implemente detecção de mudança por conta própria (ex.: watch de filesystem) e que o core
trate mensagens JSON-RPC não solicitadas (sem `id` correlacionável) — complexidade que o walking
skeleton não precisa para provar o contrato. Adiada para uma feature futura.

---

## Resumo das decisões

| # | Decisão | Resolve |
|---|---|---|
| D1 | Fonte da verdade = `protocol/SPEC.md` + JSON Schema; crate Rust é só um binding | Restrição #1, Princípio II |
| D2 | Framing NDJSON, serialização compacta obrigatória | Restrição #2 |
| D3 | Plugin de referência em Python (stdlib), sem depender de `farol-protocol` | Restrição #1 (prova estrutural) |
| D4 | Executor do iced (feature `tokio`) é o único runtime async do processo core | Restrição #3 |
| D5 | Worker `Subscription` + canal `mpsc` isola I/O de plugin do `update`/`view` | Restrição #4 |
| D6 | `child.wait()` (morte) + timeout de RPC por chamada (travamento), 5s default | Restrição #5, FR-019/020 |
| D7 | `protocol_version` = `"MAJOR.MINOR"`, igualdade exata enquanto `MAJOR == 0` | Restrição #6, FR-004/005 |
| D8 | Widget atualizado por polling core-iniciado (`widget/get`), sem push do plugin | FR-011 |

Nenhum item da tabela acima permanece como `NEEDS CLARIFICATION`.
