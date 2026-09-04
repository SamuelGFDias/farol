# Research: Plugin de Containers Docker

Todas as incógnitas de produto do `spec.md` já foram resolvidas em `/speckit-specify` +
`/speckit-clarify` (C1-C11, ver `checklists/requirements.md`). Este documento cobre as decisões
técnicas necessárias para desenhar o protocolo e a integração — não há mais nenhum
`NEEDS CLARIFICATION` de produto, só decisões de implementação.

Segue a mesma forma de `specs/004-vpn-status-plugin/research.md` (Decisão / Rationale /
Alternativas consideradas por decisão numerada `Dn`).

---

## D1 — Fonte de dados: CLI `docker`, não a Engine API em `/var/run/docker.sock`

**Decisão**: o plugin consome o **binário `docker` já instalado na máquina** via `subprocess`, com
saída estruturada (`docker ps --all --no-trunc --format '{{json .}}'`, e
`docker start|stop|restart <id>` para as ações). O plugin **não** abre o socket Unix do daemon
(`/var/run/docker.sock`) nem fala a Engine API HTTP diretamente.

**Rationale**:

1. **Precedente e Princípio II da constitution.** É exatamente o que `git-local` faz com `git` e o
   que `openfortivpn-vpn` faz com `openfortivpn-gui` (D1 da feature 004): o plugin traduz um
   contrato de processo estável, sem importar biblioteca nem reimplementar cliente.
2. **FR-015 (não duplicar lógica de gerenciamento de container).** Falar a Engine API na mão
   obrigaria o plugin a reimplementar coisas que o cliente `docker` já resolve: resolução de
   `DOCKER_HOST`, `docker context` (o contexto ativo pode apontar para um socket rootless em
   `$XDG_RUNTIME_DIR/docker.sock`, para um daemon remoto via SSH, ou para o Podman em modo
   compatível), negociação de versão da API (`/v1.xx/containers/json`), e TLS quando o contexto usa
   `tcp://`. Reimplementar isso é literalmente duplicar lógica de cliente Docker dentro do Farol.
3. **Modelo de permissão do manifesto (Princípio IV).** `capabilities: [{"kind": "exec"}]` já existe
   no vocabulário do handshake e já é a capability declarada por `git-local`. Falar direto com o
   socket exigiria uma capability nova ("acesso a socket Unix arbitrário" ou "leitura/escrita de
   caminho do sistema") que **não existe** no `CapabilityManifest` atual — ou seja, a alternativa
   custaria uma mudança de protocolo adicional só para piorar a granularidade de permissão declarada
   ao usuário (`exec` de um binário conhecido é mais legível do que "escreve num socket do sistema").
4. **Paridade exata com o terminal do usuário.** O que o widget mostra é, por construção, o que o
   `docker ps` do usuário mostraria — incluindo o contexto/daemon que ele configurou. Isso torna a
   Assumption do `spec.md` ("o Farol roda na mesma máquina em que o Docker está instalado") uma
   afirmação verificável pelo usuário com um comando, e não uma caixa-preta do Farol.

**Custo aceito**: a saída da CLI é um contrato textual (campos de `--format '{{json .}}'`) menos
estável que o JSON da Engine API, e cada consulta paga o custo de um processo novo (~50-80 ms em
máquina de desenvolvimento). Ambos são irrelevantes no orçamento de FR-014 (3 s) e no intervalo de
polling do produto (30 s default).

**Alternativas consideradas**:

| Alternativa | Por que foi rejeitada |
|---|---|
| **Engine API por HTTP sobre `/var/run/docker.sock`** (via `http.client.HTTPConnection` com socket Unix — factível só com stdlib) | Rejeitada pelos motivos 2, 3 e 4 acima. O ganho real seria JSON já estruturado e ~50 ms por consulta — nenhum dos dois é gargalo aqui. O custo é alto e concentrado justamente no ponto que a constitution protege (permissão declarada e não duplicação de lógica). |
| **Biblioteca `docker` do PyPI (docker-py)** | Rejeitada de saída: os três plugins de referência existentes são **stdlib pura**, deliberadamente (`plugins/*/main.py`). Introduzir a primeira dependência externa de plugin para economizar um `subprocess` inverteria uma convenção estabelecida em três features. |
| **`podman` como alvo alternativo na mesma feature** | Fora de escopo. O `podman` é compatível o suficiente para que a mesma abordagem funcione, mas suportá-lo explicitamente exigiria decidir precedência entre binários e nomear o widget de forma neutra — complexidade especulativa sem requisito (mesma disciplina de D3 da feature 004). O nome do plugin (D8) deixa espaço para um `podman-containers` separado no futuro. |

### D1.1 — Forma exata da consulta e campos consumidos

`docker ps --all --no-trunc --format '{{json .}}'` emite **uma linha JSON por container** (a mesma
disciplina NDJSON que o próprio protocolo do Farol usa — não é um array JSON único). Verificado
contra Docker 29.6.2 nesta máquina. Campos consumidos:

| Campo da CLI | Uso |
|---|---|
| `ID` | Identidade estável (FR-013). Com `--no-trunc` vem o ID completo de 64 hex; sem a flag, vem truncado em 12. |
| `Names` | Nome exibido (FR-002). Pode conter **mais de um nome separado por vírgula**; o plugin usa o primeiro. Docker sempre atribui ao menos um. |
| `Image` | Imagem de origem (FR-002). Normalmente a tag (`ghcr.io/x/y:latest`); quando a imagem foi **removida ou nunca teve tag**, a CLI devolve o identificador da imagem (`sha256:...` com `--no-trunc`). Nunca vazio — o campo é sempre exibível. |
| `State` | Estado canônico (FR-003). Vocabulário fechado do Docker: `created`, `restarting`, `running`, `removing`, `paused`, `exited`, `dead`. |
| `Status` | Texto humano complementar (`"Up 38 hours (healthy)"`, `"Exited (137) 43 hours ago"`). Consumido apenas como texto auxiliar de exibição — **nunca** parseado para derivar estado. |

**Por que `State` e não `Status`**: `Status` é prosa gerada pela CLI, sujeita a i18n e a mudanças de
formatação entre versões; `State` é o vocabulário do daemon. Derivar estado de `Status` seria
exatamente o antipadrão que `protocol/SPEC.md` §8 proíbe para `message` de erro ("callers MUST use
`code`/`data.reason` for programmatic handling, never parse `message`").

**Por que `--no-trunc`**: o ID de 12 caracteres é um prefixo, não um identificador — dois containers
podem, em princípio, colidir no prefixo, e a CLI aceita prefixos ambíguos com erro. FR-013 exige que
a ação recaia exatamente sobre o container selecionado; o ID completo é a única forma de garantir
isso. Verificado que `--no-trunc` **não** substitui a tag da imagem pelo digest: containers com
imagem tagueada continuam reportando a tag (só o ID do container e imagens já sem tag ficam
completos).

## D2 — Versão do protocolo: `0.3` → `0.4`, aditiva, com os **três** plugins migrados nesta feature

**Decisão**: bump de `protocol_version` para `"0.4"`. As mudanças são aditivas (novo `kind` de
widget `"container-status-grid"` + novos `$defs`; `ActionInvokeResult` ganha uma terceira opção no
`oneOf` já existente; duas entradas novas no catálogo textual de erro). Nenhum campo obrigatório
existente muda de forma — o wire já emitido por `git-local`, `uptime-kuma` e `openfortivpn-vpn`
continua validando contra os schemas novos sem alteração nenhuma da parte deles.

**Mas**: a série `0.x` exige **igualdade exata** de versão
(`ProtocolVersion::is_compatible_with`, `crates/farol-protocol/src/version.rs`). Assim que o core
passar a falar `"0.4"`, os três plugins existentes — que declaram `"0.3"` — ficam
`Unavailable{VersionIncompatible}`, mesmo sem nenhuma mudança de wire que os afete.

**Decisão explícita**: migrar a constante `PROTOCOL_VERSION` de **`git-local`, `uptime-kuma` e
`openfortivpn-vpn`** para `"0.4"` **dentro desta mesma feature** (mudança mecânica de uma linha em
cada, sem tocar em mais nada — nenhum deles usa nenhum campo novo).

**Rationale**: é exatamente a decisão D2 da feature 004, aplicada de novo e pelo mesmo motivo. A
feature 002 deixou `git-local` propositalmente não migrado no bump `0.1 → 0.2`, isso virou o "débito
técnico #4" (issue #4, fechada só sessões depois, na T050 daquela feature) e nada de bom veio disso:
puro custo de rastreamento adiado numa mudança que sequer *precisa* ficar pendente. A feature 004 já
corrigiu o padrão migrando os dois plugins de então junto com o bump; esta feature mantém o padrão
corrigido, agora com três.

**Consequência de teste**: subir a constante do core sem migrar os plugins na mesma leva quebra a
suíte em bloco, e por dois caminhos distintos:

1. `crates/farol-core/src/e2e_tests.rs` afirma `PluginState::Ready` para cada plugin de referência
   contra um `Emulator` real. Um plugin ainda declarando `"0.3"` contra um core em `"0.4"` é
   **incompatível** sob `is_compatible_with` (regime `MAJOR == 0`), então o estado observado vira
   `Unavailable { VersionIncompatible }` e o teste falha. A feature 004 viu exatamente isso — 8
   testes quebrando enquanto a migração dos plugins ainda não estava aplicada.
2. `crates/farol-protocol/tests/contract_schema_validation.rs` e `schema_boundaries.rs` fixam os
   4 `include_str!` e os 4 `$id` em `protocol/schema/v0.3/` por constante literal; eles precisam
   apontar para `v0.4/` na mesma fase.

Há ainda um terceiro caminho, cosmético mas que suja o diagnóstico: várias mensagens de asserção
citam a versão literal em texto ("handshake 0.3", "protocolo 0.2 desde 9d2fe77"). Não quebram nada,
mas passam a mentir; atualizá-las junto evita que a próxima migração leia um histórico errado.

O bump é, portanto, uma tarefa *foundational* que precisa vir antes de qualquer trabalho de User
Story, e não um detalhe de polimento.

**Alternativas consideradas**:

- Deixar os três plugins como dívida técnica (padrão da feature 002) — rejeitada pelo raciocínio
  acima; seria reintroduzir deliberadamente uma dívida que a feature 004 acabou de aposentar.
- Não bumpar a versão, encaixando o novo `kind` dentro de `"0.3"` — rejeitada: contradiz a convenção
  já estabelecida em três features de que qualquer adição de vocabulário de protocolo (mesmo
  aditiva) bumpa `MINOR`, para deixar rastreável no histórico qual `kind`/campo passou a existir em
  qual versão (`protocol/schema/v0.{1,2,3}/` documentam isso lado a lado).
- Relaxar `is_compatible_with` para aceitar `MINOR` diferente dentro de `0.x` — rejeitada: é uma
  mudança de política de compatibilidade do protocolo, de alcance muito maior que esta feature, e
  contradiz o texto normativo de `protocol/SPEC.md` §6.4 ("a série `0.x` não carrega garantia de
  compatibilidade nem entre MINORs"). Se algum dia for feita, é uma feature própria.

## D3 — Novo `kind` de widget: `"container-status-grid"`, lista de N itens com ações por item

**Decisão**: novo `WidgetDeclaration.kind = "container-status-grid"` (widget
`id: "docker-containers"`, `title: "Containers Docker"`), reportando `WidgetGetResult.items` como um
array de N `ContainerStatusItem` independentes — **um por container**, estruturalmente igual a
`status-grid`/`monitor-status-grid` e diferente de `vpn-status` (que é singleton por natureza do
domínio, D3 da feature 004). `WidgetItems` (Rust) ganha uma quarta variante
`Container(Vec<ContainerStatusItem>)`.

`ContainerStatusItem` carrega (forma exata em `data-model.md` §1.3):

- `id`: ID completo do container (64 hex) — identidade estável para FR-013, **não** o nome.
- `name`: nome exibido (FR-002).
- `image`: imagem de origem (FR-002), conforme D1.1.
- `state`: `ContainerState` (D3.1).
- `status_text`: texto humano auxiliar da CLI (`Status`), `Option<String>` — puramente informativo.
- `start_action`, `stop_action`, `restart_action`: três `ActionDeclaration` (D4).

**Por que uma lista de N itens com ações por item é o desenho certo**: o padrão "emparelhar um item
de dado com a `ActionDeclaration` que opera sobre ele" já existe desde a feature 001
(`WidgetItem { repo, fetch_action }`) — este widget é o mesmo padrão com **três** ações por item em
vez de uma. Não há nada estruturalmente novo no protocolo: nem `ActionDeclaration` nem `ActionTarget`
mudam de forma. É por isso que o delta de schema desta feature é pequeno apesar de a feature
introduzir o widget mais rico do produto até aqui.

**Nome do `kind`**: `container-status-grid` segue a família `status-grid` /
`monitor-status-grid` — sufixo `-grid` para os `kind`s que são lista de N linhas independentes,
reservando nomes sem sufixo (`vpn-status`) para os singleton. O nome carrega, portanto, informação
estrutural sobre o `kind`, não só o domínio.

**Alternativas consideradas**:

- Reaproveitar `status-grid` com um `WidgetItem` genérico o suficiente para servir a repositórios e
  containers — rejeitada: `WidgetItem` é `{repo: GitRepository, fetch_action}`, específico de Git;
  generalizá-lo agora significaria refatorar o wire de `git-local` (mudança **não** aditiva) para
  ganhar nada — o core renderiza por `kind` de qualquer forma.
- Um `kind` `"docker-containers"` (nomeado pela ferramenta, como o `plugin_name`) — rejeitada: o
  `kind` descreve a **forma de dado** que o core sabe renderizar, não a ferramenta de origem
  (`monitor-status-grid` não se chama `uptime-kuma-grid`). Um segundo plugin de containers (Podman,
  LXC) deveria poder reusar o mesmo `kind`.

### D3.1 — `ContainerState`: vocabulário fechado + `Unknown`

**Decisão**: enum com os sete estados publicados pelo Docker — `Created`, `Restarting`, `Running`,
`Removing`, `Paused`, `Exited`, `Dead` — **mais** uma oitava variante `Unknown`, à qual o plugin
mapeia qualquer valor de `State` que não reconheça (FR-012).

**Rationale — e por que isto é um desvio deliberado do precedente**: `MonitorStatus` (feature 002)
faz o **oposto**: um valor bruto fora de `{0,1,2,3}` invalida a leitura inteira
(`-32007`/`metrics_parse_error`), sem variante de escape. O desvio é justificado pela diferença de
**raio de dano**:

- O Uptime Kuma entrega um **documento de métricas único** que o plugin interpreta como um todo; um
  token inválido ali torna toda a leitura suspeita, e não há forma segura de dizer quais linhas do
  documento ainda valem.
- O Docker reporta **cada container de forma independente**, uma linha JSON por container. Um estado
  desconhecido numa linha não diz nada sobre as outras. Esvaziar o widget inteiro (ou marcá-lo em
  erro) porque uma versão futura do Docker introduziu um estado novo seria uma falha de
  disponibilidade autoinfligida, e justamente no cenário em que o usuário mais precisa ver o resto.

FR-012 fixa o comportamento: linha degradada (estado "desconhecido", **nenhuma** ação acionável, ver
a matriz de FR-008), lista preservada, e proibição explícita de traduzir silenciosamente o
desconhecido para "rodando"/"parado". A variante existe no **protocolo**, e não só na UI, porque é o
plugin — que viu o valor bruto — quem sabe que não reconheceu; o core não deve adivinhar.

**Alternativas consideradas**:

- Espelhar `MonitorStatus` e invalidar a leitura inteira — rejeitada pelo raio de dano acima.
- Carregar o valor bruto junto (`Unknown(String)`) para exibi-lo — rejeitada nesta versão: o
  `status_text` da CLI já é exibido e cobre o caso de diagnóstico, e um enum com payload
  complicaria o schema (`oneOf` de string vs. objeto) sem requisito concreto. Se um dia houver
  necessidade, é extensão aditiva.

## D4 — Três `ActionDeclaration` por item, `enabled` decidido pelo plugin

**Decisão**: cada `ContainerStatusItem` carrega três `ActionDeclaration` — `docker.container.start`,
`docker.container.stop`, `docker.container.restart` — todos com
`target: {type: "docker-container", id: <ID completo do container>}` e `enabled` calculado **pelo
plugin** conforme a matriz normativa de FR-008.

**Rationale**:

- `protocol/SPEC.md` §5.3 e o doc de `ActionDeclaration.enabled` já são normativos: "O core MUST NOT
  decidir isso por conta própria e MUST NOT permitir invocar uma ação com `enabled: false`". A
  matriz de FR-008 é conhecimento de domínio Docker; colocá-la no core violaria o Princípio III
  (core renderiza, plugin decide o que é dado).
- Os três campos são **sempre presentes** (nunca ausentes ou `null`), variando só `enabled`. Isso
  mantém a renderização estável: a linha do container tem sempre os mesmos três controles, alguns
  desabilitados — a UI não reflui quando o estado muda. É a mesma disciplina de
  `WidgetItem.fetch_action`, que existe sempre e fica `enabled: false` quando o repositório não tem
  remoto.
- `ActionTarget` já é genérico (`{type: String, id: String}`) — nenhuma mudança de schema.

**`target.id` é o ID do container, nunca o nome** (FR-013): nomes podem ser liberados e reatribuídos
entre dois ciclos de atualização; o ID não. Ver também D5 (o que fazer quando o container some entre
a exibição e o clique).

**`timeout_hint_ms` por ação** (D6): declarado explicitamente em cada uma das três, porque as três
têm orçamentos diferentes e nenhuma delas se parece com o default de 120 s do core.

**Alternativas consideradas**:

- Uma única `ActionDeclaration` genérica por item (`docker.container.lifecycle`) com a operação
  concreta viajando num campo novo do request — rejeitada pelo mesmo motivo de D4 da feature 004:
  `action/invoke` MUST ecoar o `target` literal de uma `ActionDeclaration` conhecida
  (`protocol/SPEC.md` §5.3); inventar um canal de parâmetro paralelo quebraria essa invariante. E,
  pior aqui do que lá, destruiria a expressividade de `enabled` — com uma ação só, o plugin não
  teria como dizer que "parar" é inválido mas "reiniciar" é válido para o mesmo container.
- Omitir as ações não aplicáveis (array de ações de tamanho variável) em vez de mandá-las com
  `enabled: false` — rejeitada: a UI passaria a reflui a cada mudança de estado, e o core perderia a
  informação de que a ação *existe mas não cabe agora* (que é o que FR-008/Acceptance Scenario 4 de
  US2 pede que o usuário perceba).

## D5 — Catálogo de erro: dois códigos de domínio novos (`-32010`, `-32011`)

O catálogo `-32000..-32009` está **inteiramente ocupado** (`protocol/schema/v0.3/error.schema.json`,
`protocol/SPEC.md` §8.2). Os dois códigos novos são os próximos livres.

**Decisão**:

- **`-32003` / `exec_unavailable`** — **reaproveitado**, sem código novo, para "binário `docker`
  ausente do `PATH`" (FR-010a). Mesmo uso que `git-local` faz para `git` e `openfortivpn-vpn` para
  `openfortivpn-gui`.
- **`-32010` / `docker_unavailable`** (novo) — erro de `widget/get`: o binário existe, mas a consulta
  não pôde ser satisfeita. `data.detail.condition` distingue a causa:
  `"daemon_unreachable"` (FR-010b), `"permission_denied"` (FR-010c), `"timeout"` (FR-014) ou
  `"cli_error"` (saída inesperada/não interpretável). `data.detail.raw` MAY carregar stderr
  truncado. O `message` do `ErrorObject` carrega a tradução legível e **distinta por condição**
  exigida por FR-010.
- **`-32011` / `container_action_failed`** (novo) — erro de `action/invoke`: `start`/`stop`/`restart`
  falhou. `data.detail` = `{"docker_condition": <ver contrato>, "raw": <stderr truncado>}`;
  `message` carrega a tradução legível (FR-009).

**Rationale — por que um código por *método* e não um por *causa***: segue o precedente já
estabelecido por `-32001`/`fetch_failed` (git-local) e `-32009`/`vpn_action_failed` (feature 004) —
"a operação subjacente falhou", com o motivo específico em `data`, não espalhado por códigos
distintos. FR-010 exige que as três condições de indisponibilidade sejam **distinguíveis pelo
usuário** (mensagens diferentes), não que o código numérico do protocolo seja granular: a
granularidade programática já existe em `data.detail.condition`, e nenhuma parte do core decide
*comportamento* diferente por causa — só exibe.

**Contraprecedente examinado e descartado**: `-32004`/`scan_root_unreadable` (git-local) *é* um
código dedicado a uma falha de permissão, o que poderia sugerir um `-320xx`/`docker_permission_denied`
separado. Ele não se aplica aqui: `-32004` distingue uma condição de **configuração do plugin** (um
caminho que o usuário configurou existe mas não é legível) de uma leitura normal, e é reportado num
ponto em que "não existe/vazio" tem tratamento oposto (sucesso com lista vazia). No caso do Docker,
"sem permissão" e "daemon parado" são duas causas **da mesma chamada**, com o mesmo tratamento em
todo o resto do sistema — exatamente o formato que `data.detail` existe para carregar.

**Alternativas consideradas**: três códigos novos, um por condição de FR-010 — rejeitada por inflar
o catálogo sem ganho programático (o catálogo é global ao protocolo, e cada código gasto é
permanente).

### D5.1 — Como as três condições de FR-010 são detectadas

Detecção verificada empiricamente contra Docker 29.6.2 (as mensagens de erro do cliente **mudaram**
entre versões maiores — a formulação clássica era "Cannot connect to the Docker daemon at ...; is
the docker daemon running?"):

| Condição | Detecção | Observado |
|---|---|---|
| (a) ferramenta ausente | `shutil.which("docker") is None` — **sem executar nada** | Determinístico, independente de versão. |
| (c) permissão negada | exit ≠ 0 **e** stderr contém a substring `permission denied` (case-insensitive) | `permission denied while trying to connect to the docker API at unix:///...` |
| (b) daemon inacessível | exit ≠ 0, stderr **não** casa (c), **e** casa `failed to connect` \| `cannot connect` \| `is the docker daemon running` (case-insensitive) | `failed to connect to the docker API at unix:///...; check if the path is correct and if the daemon is running` |
| (fallback) `cli_error` | exit ≠ 0 e nenhuma das acima | Garante que nenhuma saída inesperada vire silêncio (SC-004). |

**Ordem de teste é normativa**: (c) antes de (b). A mensagem de permissão negada também é uma falha
de conexão, então testar (b) primeiro classificaria erroneamente todo caso de permissão como daemon
parado — que é justamente a confusão que FR-010 existe para evitar, e a condição mais provável numa
máquina recém-configurada.

**Fragilidade reconhecida e mitigada**: classificar por substring de stderr é acoplamento a texto
não-contratual do cliente Docker. Mitigações: (1) a substring `permission denied` é a mais estável
das três (sobreviveu à reformulação da mensagem entre versões, porque descreve o `errno`); (2) o
`fallback` `cli_error` garante que uma reformulação futura degrada para "erro genérico legível", não
para silêncio nem para classificação errada silenciosa; (3) o `data.detail.raw` sempre carrega o
stderr bruto, então o diagnóstico real nunca se perde. Registrado no `plan.md` § Complexity Tracking
como acoplamento consciente, não como defeito.

## D6 — Orçamentos de tempo: consulta 3 s, ações com `timeout_hint_ms` explícito

**Decisão**:

| Chamada | Timeout do `subprocess` no plugin | `timeout_hint_ms` declarado |
|---|---|---|
| `docker ps` (dentro de `widget/get`) | **3 s** (FR-014) | N/A (`widget/get` usa `RPC_TIMEOUT_CONTROL`) |
| `docker.container.start` | 15 s | `20000` |
| `docker.container.stop` | 30 s | `35000` |
| `docker.container.restart` | 40 s | `45000` |

**Rationale**:

- **Consulta (3 s)**: `widget/get` é uma chamada de *controle*, com orçamento
  `RPC_TIMEOUT_CONTROL` = **5 s** (`protocol/SPEC.md` §7.1). O timeout do subprocess precisa ficar
  **estritamente abaixo** desse orçamento, com folga para serialização e ida/volta do NDJSON. Se
  ficasse igual ou acima, um daemon travado estouraria o orçamento do core e a conexão inteira do
  plugin viraria `Unresponsive` — o usuário veria "o plugin Docker travou" em vez de "o daemon
  Docker não respondeu", que são diagnósticos completamente diferentes. Três segundos é o número que
  FR-014/SC-006 fixam.
- **Ações**: `docker stop` tem um **período de graça de 10 s** por default (`SIGTERM`, depois
  `SIGKILL`), então uma parada legítima pode levar pouco mais de 10 s; `restart` é `stop` + `start`,
  logo mais ainda. O `timeout_hint_ms` é declarado **acima** do timeout interno do subprocess
  (mesma disciplina de D1/`connect` da feature 004) para que, quando a operação estourar, quem
  reporte seja o **plugin**, com um erro de domínio traduzido (`-32011`, `docker_condition:
  "timeout"`), e não o core sintetizando `-32002`/`action_timeout` genérico. O default de 120 s do
  core seria absurdamente frouxo aqui — o usuário ficaria dois minutos olhando um botão travado.

**Alternativas consideradas**: usar `docker stop --time 5` para encurtar a graça — rejeitada:
mudaria o comportamento de desligamento que o usuário configurou para o container (alguns
containers precisam da graça inteira para persistir estado). O Farol reporta o Docker, não altera a
política dele (FR-015).

## D7 — `"operação em andamento"` na UI: estado local por item, sobrevivente ao refresh

**Decisão**: a indicação de operação em curso (FR-017) vem de um campo de UI local no core —
`ContainerViewModel.action_in_flight: Option<ContainerActionKind>` — setado ao disparar
`action/invoke` e limpo ao receber a resposta. Mesmo padrão de `RepositoryViewModel.fetch_in_flight`
(feature 001) e `VpnWidgetViewModel.connect_in_flight` (feature 004), generalizado de "um booleano
global do widget" para "um campo por item da lista".

**O ponto sutil que FR-017 fixa**: o widget é repopulado a cada `widget/get` (default 30 s). Sem
cuidado explícito, um refresh que chegue no meio de um `restart` substituiria a lista inteira e
apagaria a marca de "em andamento" — o botão voltaria a ficar acionável e o usuário dispararia uma
segunda operação sobre um container em transição. A função de merge do core
(`update::merge_widget_items`) **já resolve exatamente esse problema** para a variante `Git`:
preserva `fetch_in_flight`/`last_error` de qualquer repositório presente nas duas listas, casado por
`repo.id`. A variante `Container` reusa o mesmo mecanismo, casando por
`ContainerStatusItem.id` — não é um mecanismo novo, é a extensão de um já validado.

**Efeito de `enabled` durante a operação**: enquanto `action_in_flight.is_some()` para um item, o
core MUST NOT permitir invocar nenhuma das três ações **daquele** item, mesmo que o plugin as tenha
declarado `enabled: true` na última leitura. Isso é uma restrição *adicional* do core sobre o que já
está `enabled`, não uma decisão de `enabled` pelo core — não conflita com §5.3 (o core pode recusar
invocar; o que ele não pode é *habilitar* o que o plugin desabilitou).

**Alternativas consideradas**: pedir ao plugin que reporte a operação em andamento no próprio
`ContainerStatusItem` (um campo `busy`) — rejeitada: o plugin processa um request por vez (mesma
premissa já assumida por `git-local`/`openfortivpn-vpn`, D6 da feature 004), então durante um
`action/invoke` ele nem chegaria a atender o `widget/get` que reportaria `busy`. O estado "o Farol
está esperando esta resposta" é, por definição, conhecimento do core.

## D8 — Nome do plugin, do widget e das ações

**Decisão**: `plugin_name: "docker-containers"`, diretório `plugins/docker-containers/`,
widget `id: "docker-containers"` / `kind: "container-status-grid"`, ações
`docker.container.start` / `docker.container.stop` / `docker.container.restart`.

**Rationale**: segue a convenção já estabelecida — o plugin é nomeado pela **ferramenta concreta que
envolve** mais o domínio (`openfortivpn-vpn`, `uptime-kuma`, `git-local`), não por um termo genérico
("containers"). Isso deixa espaço para um `podman-containers` futuro sem colisão de nome, e deixa
claro pelo nome qual integração é essa. Os IDs de ação seguem o padrão `<domínio>.<objeto>.<verbo>`
já usado por `git.fetch` e `vpn.connect`/`vpn.disconnect`, com o nível `container` explícito porque
um plugin Docker plausivelmente ganhará ações sobre outros objetos (imagem, volume) no futuro.

## D9 — Sem poller em background; sem tela de setup

**Decisão**: `widget/get` executa `docker ps` de forma síncrona a cada requisição, sem cache — mesmo
modelo de `git-local` e `openfortivpn-vpn`, não o de `uptime-kuma` (thread de poller + cache).
`required_config: []`, sem nenhuma tela de setup: o plugin fica `Ready` assim que o handshake
completa, sem passar por `Unavailable{NotConfigured}`.

**Rationale**: a chamada é local (subprocess na mesma máquina), não uma requisição de rede —
replicar a complexidade do poller de `uptime-kuma` (thread dedicada, cache, invalidação) não teria
benefício e seria complexidade especulativa. E não há configuração a pedir: "todos os containers
locais" é o padrão completo e correto (C6 de `checklists/requirements.md`); `git-local` tem
configuração apenas porque não existe raiz de varredura padrão sensata para repositórios.

## D10 — Ordenação estável (FR-004)

**Decisão**: o plugin ordena a lista por `(name, id)` ascendente antes de emitir `items` — não
confia na ordem de saída do `docker ps`.

**Rationale**: o `docker ps` emite em ordem de criação decrescente na prática, mas isso **não é
contrato documentado** e não sobrevive a mudanças de implementação do daemon. FR-004 exige ordem
estável entre atualizações consecutivas; um requisito só é testável se a ordem for determinística
por construção. Ordenar por nome também é a ordem mais previsível para o usuário (é como ele pensa
nos containers) e é insensível a recriação — um container recriado com o mesmo nome mantém a
posição, o que é exatamente o que o usuário espera ao rodar `docker compose up` de novo.

**Alternativas consideradas**: ordenar por `CreatedAt` (aproximando a ordem nativa da CLI) —
rejeitada: containers recriados pulam para o topo, que é o oposto da estabilidade que FR-004 pede; e
`CreatedAt` vem como string formatada com timezone local, exigindo parse para ordenar corretamente.

## D11 — O que o `action/invoke` devolve

**Decisão**: `ActionInvokeResult` ganha a variante `Container { container: ContainerStatusItem }` —
o item **inteiro**, pós-ação, incluindo as três `ActionDeclaration` recalculadas.

**Rationale**: é o `ContainerStatusItem` completo, e não só o novo estado, porque `enabled` das três
ações muda com o estado (FR-008) e o core **não pode** recalculá-lo por conta própria
(`protocol/SPEC.md` §5.3). Devolver só `state` obrigaria o core a derivar `enabled` da matriz de
FR-008 — exatamente o que D4 proíbe. Mesmo padrão de `git.fetch` devolvendo o `GitRepository`
pós-fetch e de `vpn.connect` devolvendo o `VpnStatusItem` pós-ação: o usuário vê o resultado sem
esperar o próximo ciclo de polling (SC-003).

**Como o plugin obtém o item pós-ação**: executa a operação (`docker start|stop|restart <id>`) e,
com sucesso, faz uma releitura pontual `docker ps --all --no-trunc --filter id=<id> --format
'{{json .}}'` daquele container só. Uma chamada extra, barata, contra o mesmo ID exato.

**Caso de borda — container removido durante a operação**: se a operação retorna sucesso mas a
releitura devolve vazio (o container foi removido por outra via no intervalo), o plugin responde
`-32011` com `docker_condition: "container_gone"` e mensagem legível dizendo que a operação foi
executada mas o container não está mais na lista. É honesto (não inventa um item que não existe
mais), e a lista se corrige sozinha no próximo ciclo (Edge Case correspondente do `spec.md`).

## D12 — `WidgetItems` untagged: a quarta variante ainda desambigua, mas por acaso

**Verificação feita** (não é uma decisão nova — é a validação de que D3 é seguro). `WidgetItems` é
`#[serde(untagged)]`; as variantes são tentadas em ordem de declaração
(`Git`, `Monitor`, `Vpn`, `Container`). Um array **não vazio** de `ContainerStatusItem`:

- **não** casa `Git`: `WidgetItem` exige `repo` e `fetch_action`, ausentes.
- **não** casa `Monitor`: `MonitorStatusItem` exige `status` com valor em
  `{up, down, pending, maintenance}`; o item de container não tem campo `status` (o texto auxiliar
  chama-se `status_text` **deliberadamente**, ver abaixo).
- **não** casa `Vpn`: `VpnStatusItem` exige `available_profiles` e `disconnect_action`, ausentes.

E, simetricamente, nenhum dos três tipos existentes casa `ContainerStatusItem` (que exige `id`,
`image`, `start_action`, `stop_action`, `restart_action`).

**Consequência normativa de desenho**: o campo de texto auxiliar chama-se `status_text`, e **não**
`status`, precisamente para não aproximar estruturalmente `ContainerStatusItem` de
`MonitorStatusItem`. Um campo `status` de string livre criaria dependência de ordem de variante para
a desambiguação — que funciona, mas por sorte.

**Array vazio** continua ambíguo (débito #5, issue #7): `items: []` desserializa sempre como a
primeira variante (`Git`). A correção existente — `update::normalize_widget_items`, que usa o `kind`
declarado no handshake para reinterpretar o array vazio — só precisa ganhar mais um braço para
`WidgetKind::Container`, sem mudança de abordagem. Isso é relevante nesta feature porque
**máquina sem nenhum container é um caso normal e comum** (FR-011), muito mais provável que o
equivalente das features anteriores.

**Dívida registrada**: a desambiguação por disjunção incidental de formato não escala — um quinto
`kind` terá de repetir esta análise, e um dia duas formas colidirão. A alternativa estrutural (um
envelope com tag explícita, `{"kind": ..., "items": [...]}`) resolveria de vez, mas é uma mudança
**não aditiva** do wire de todos os plugins, fora do escopo desta feature. Rastreado como **issue #9**
(Governance da constitution: dívida deliberada MUST virar issue).
