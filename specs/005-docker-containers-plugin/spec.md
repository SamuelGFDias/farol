# Feature Specification: Plugin de Containers Docker

**Feature Branch**: `005-docker-containers-plugin`

**Created**: 2026-09-03

**Status**: Draft

**Input**: User description: "Feature 005: plugin de referência Docker para o Farol. Expõe o estado
de todos os containers Docker locais (rodando, parado, e os demais estados relevantes do daemon;
nome e imagem de cada um) como um widget declarativo do core, seguindo o mesmo padrão de plugin
JSON-RPC dos plugins existentes (git-local, uptime-kuma, openfortivpn-vpn) — o plugin farol chama a
ferramenta Docker já instalada na máquina e traduz a saída para o protocolo farol-protocol, sem
duplicar nenhuma lógica de gerenciamento de container (mesma disciplina de FR-010 da feature 004).
Escopo conforme README.md: (Ver) o estado de todos os containers locais, incluindo os parados, com
ordem estável entre atualizações; (Agir) iniciar, parar e reiniciar um container diretamente pelo
widget, sem abrir terminal — o README cita 'reiniciar container' como exemplo do verbo Agir do
produto. O README também cita 'logs' na seção de integrações previstas: esta primeira versão deixa
logs explicitamente Fora de Escopo, porque exibir N linhas de log exige uma superfície de UI de
detalhe/drill-down que o core ainda não tem (todo widget hoje renderiza uma lista plana no painel) —
decisão de arquitetura de core independente desta integração, a ser tratada em feature própria.
Premissa/limitação a documentar: falar com o daemon Docker exige que o usuário do processo Farol
tenha acesso ao daemon (grupo docker ou daemon rootless); o Farol nunca pede senha nem escala
privilégio — uma falha de permissão chega como erro de domínio legível. A questão mais ampla de
'Ações privilegiadas (sudo sob demanda, polkit, ou daemon auxiliar)' do README § Decisões em aberto
fica fora do escopo desta feature."

## Clarifications

### Session 2026-09-03

Todas as respostas desta sessão foram **resolvidas pelo orquestrador sem input do usuário, conforme
instrução da sessão** (modo silencioso). A base de cada decisão está registrada em
`checklists/requirements.md` § "Clarificações resolvidas em 2026-09-03" (C1-C11).

- Q: Quais das três ações (iniciar/parar/reiniciar) ficam acionáveis em cada estado de container? →
  A: iniciar em `criado`/`parado`; parar em `rodando`/`reiniciando`/`pausado`; reiniciar em
  `criado`/`parado`/`rodando`/`reiniciando`/`pausado`; **nenhuma** acionável em `em remoção`,
  `morto` ou `desconhecido`.
- Q: Qual o limite de tempo da consulta de estado quando o daemon não responde? → A: no máximo 3
  segundos por consulta, para caber com folga dentro do orçamento de atualização do widget — um
  daemon travado vira erro pontual do widget, nunca "plugin travado".
- Q: O que acontece se o usuário acionar uma ação sobre um container que já tem outra ação em
  andamento, ou se o ciclo de atualização chegar no meio de uma ação? → A: as três ações **daquele**
  container ficam não acionáveis enquanto a operação está em andamento (os demais containers seguem
  acionáveis normalmente), e uma atualização da lista que chegue no meio da operação não apaga a
  indicação de "em andamento".

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ver o estado dos containers sem abrir terminal (Priority: P1)

Como desenvolvedor que roda a maior parte do seu ambiente local em containers, quero ver de relance
quais containers existem na minha máquina e quais estão rodando, parados ou em algum outro estado,
diretamente no painel do Farol — sem digitar `docker ps` num terminal nem manter uma aba aberta só
para isso.

**Why this priority**: É o verbo "Ver" da constitution do produto. Sozinho já entrega valor completo
(visibilidade do que está no ar e do que caiu), mesmo sem nenhuma capacidade de agir sobre os
containers. É também o pré-requisito de sentido da User Story 2: não dá para decidir reiniciar algo
sem antes ver que ele parou.

**Independent Test**: Com o Docker instalado e alguns containers em estados diferentes (ao menos um
rodando e um parado), abrir o Farol e conferir que o widget lista todos eles com nome, imagem e
estado corretos, sem nenhuma outra interação.

**Acceptance Scenarios**:

1. **Given** a máquina tem containers em estados diferentes (rodando e parados), **When** o usuário
   abre o Farol, **Then** o widget lista todos eles, cada um com nome, imagem e estado atual.
2. **Given** a máquina não tem nenhum container criado, **When** o usuário abre o Farol, **Then** o
   widget indica explicitamente "nenhum container" como um resultado normal, não como erro.
3. **Given** o widget já mostrando a lista, **When** um container muda de estado por fora do Farol
   (o usuário sobe ou derruba algo pelo terminal, ou um container morre sozinho), **Then** o widget
   reflete o novo estado no próximo ciclo de atualização, sem exigir reiniciar o Farol.
4. **Given** o widget já mostrando a lista, **When** o usuário compara duas atualizações
   consecutivas sem que nada tenha mudado na máquina, **Then** os containers aparecem na mesma
   ordem nas duas — a lista não embaralha entre atualizações.

---

### User Story 2 - Iniciar, parar e reiniciar um container pelo widget (Priority: P2)

Como desenvolvedor, quero iniciar, parar ou reiniciar um container a partir do próprio widget do
Farol, para consertar o caso mais comum do meu dia ("o container do banco caiu de novo") sem trocar
de janela nem lembrar o nome exato do container para digitar num terminal.

**Why this priority**: É o verbo "Agir" da constitution, e é literalmente o exemplo que o `README.md`
usa para esse verbo ("reiniciar container"). Depende do estado exposto pela User Story 1 para fazer
sentido, por isso vem em segundo lugar — mas é independentemente testável e entrega valor próprio
acima dela.

**Independent Test**: A partir do widget com um container parado, disparar a ação de iniciar e
confirmar que o container passa a aparecer como rodando; a partir de um container rodando, disparar
parar e reiniciar e confirmar os estados resultantes correspondentes.

**Acceptance Scenarios**:

1. **Given** um container parado listado no widget, **When** o usuário aciona iniciar, **Then** o
   container passa a aparecer como rodando, sem que o usuário precise atualizar nada manualmente.
2. **Given** um container rodando, **When** o usuário aciona parar, **Then** ele passa a aparecer
   como parado.
3. **Given** um container rodando, **When** o usuário aciona reiniciar, **Then** ele volta a
   aparecer como rodando ao fim da operação, e o widget deixa claro durante a espera que a ação está
   em andamento.
4. **Given** um container parado, **When** o usuário olha as ações oferecidas para ele, **Then**
   "parar" não é oferecida como ação acionável (e, simetricamente, "iniciar" não é oferecida para um
   container que já está rodando).
5. **Given** uma ação que falha (o container foi removido no intervalo, o daemon recusou a operação,
   permissão negada), **When** a falha ocorre, **Then** o widget mostra uma mensagem de erro legível
   sem travar nem exigir reiniciar o Farol, preservando o último estado conhecido da lista.

---

### Edge Cases

- O que acontece quando a ferramenta Docker não está instalada ou não está no `PATH` da máquina? O
  widget deve indicar essa condição como um estado de erro claro, nunca como "nenhum container".
- O que acontece quando a ferramenta existe mas o daemon do Docker não está rodando? O widget deve
  indicar isso com uma mensagem distinta de "ferramenta ausente" e distinta de "nenhum container" —
  são três remédios diferentes para o usuário (instalar, subir o serviço, criar um container).
- O que acontece quando o usuário do Farol não tem permissão para falar com o daemon (não está no
  grupo apropriado)? O widget deve mostrar isso como um erro específico de permissão, distinto de
  "daemon parado" — é a condição mais provável numa máquina recém-configurada, e o remédio é
  totalmente diferente.
- O que acontece quando o daemon demora demais ou trava ao responder? A consulta deve desistir dentro
  de um limite e reportar erro, sem deixar o widget (ou o Farol) pendurado esperando indefinidamente.
- O que acontece quando existem muitos containers (dezenas)? Todos são listados, em ordem estável;
  não há filtro nem paginação nesta versão (ver § Out of Scope).
- O que acontece quando o Docker reporta um estado de container que o Farol não reconhece (por
  exemplo, um estado novo introduzido por uma versão futura do Docker)? Aquele container aparece na
  lista com estado "desconhecido" e sem ações acionáveis; os demais containers continuam sendo
  exibidos normalmente — um estado não reconhecido degrada uma linha, nunca a lista inteira.
- O que acontece quando o usuário aciona uma ação sobre um container que foi removido entre a última
  atualização da lista e o clique? A ação falha com mensagem legível e a lista se corrige no próximo
  ciclo de atualização, sem derrubar o Farol.
- O que acontece quando dois containers têm o mesmo nome de imagem, ou quando um container é
  renomeado entre duas atualizações? A ação disparada pelo usuário MUST recair sobre exatamente o
  container que ele selecionou, não sobre outro que apenas se parece com ele.
- O que acontece quando o usuário aciona a mesma ação duas vezes seguidas no mesmo container, ou
  aciona uma segunda ação enquanto a primeira ainda não terminou? As ações daquele container ficam
  não acionáveis durante a operação (FR-017) — não há duas operações concorrentes sobre o mesmo
  container disparadas pelo Farol.
- O que acontece se um ciclo de atualização periódica chegar no meio de uma ação em andamento? A
  lista é atualizada normalmente, mas a indicação de "operação em andamento" daquele container é
  preservada (FR-017) — o usuário não vê o indicador piscar e sumir.
- O que acontece se o estado mudar por uma via completamente fora do Farol entre dois ciclos de
  atualização? O widget reflete o que a consulta reportar no próximo ciclo — não há garantia de
  detecção instantânea, mesma limitação já aceita para os demais widgets do Farol.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O Farol MUST expor um widget de referência que lista **todos** os containers Docker
  locais, incluindo os que não estão em execução.
- **FR-002**: Cada container listado MUST exibir, no mínimo, seu nome, a imagem de origem e seu
  estado atual.
- **FR-003**: O estado exibido MUST distinguir, no mínimo, "rodando" de "parado", e MUST preservar
  os demais estados relevantes reportados pelo Docker (criado, reiniciando, pausado, em remoção,
  morto) como valores próprios — sem colapsá-los todos num genérico "outro".
- **FR-004**: A ordem dos containers na lista MUST ser estável entre atualizações consecutivas
  quando o conjunto de containers não muda, para que a lista não embaralhe sob o cursor do usuário.
- **FR-005**: O widget MUST atualizar seu conteúdo periodicamente, seguindo o mesmo modelo de
  atualização (polling) já usado pelos demais widgets do Farol, refletindo mudanças de estado
  iniciadas por qualquer origem (terminal, outro app, o próprio container, ou o próprio Farol).
- **FR-006**: Uma falha pontual ao consultar o estado dos containers MUST preservar a última lista
  conhecida do widget e mostrar uma indicação de erro, em vez de apagar ou zerar a lista.
- **FR-007**: O Farol MUST permitir iniciar, parar e reiniciar um container diretamente pelo widget,
  sem exigir abrir um terminal.
- **FR-008**: Cada uma das três ações MUST ser oferecida como acionável apenas quando fizer sentido
  para o estado atual daquele container, e a decisão de o que é acionável MUST vir do plugin, não
  ser inferida pelo core. A matriz normativa é:

  | Estado do container | iniciar | parar | reiniciar |
  |---|---|---|---|
  | criado | ✅ | — | ✅ |
  | rodando | — | ✅ | ✅ |
  | reiniciando | — | ✅ | ✅ |
  | pausado | — | ✅ | ✅ |
  | parado | ✅ | — | ✅ |
  | em remoção | — | — | — |
  | morto | — | — | — |
  | desconhecido (FR-012) | — | — | — |

  "reiniciar" é a ação de maior alcance porque também serve para subir um container parado; "em
  remoção" e "morto" são estados em que qualquer operação de ciclo de vida falharia, e
  "desconhecido" é o estado em que o Farol assumidamente não sabe o suficiente para decidir.
- **FR-009**: O sistema MUST traduzir qualquer falha de uma ação (container inexistente, recusa do
  daemon, permissão negada, tempo esgotado, erro interno) em uma mensagem legível para o usuário,
  sem expor o erro bruto da ferramenta como única informação.
- **FR-010**: O sistema MUST distinguir, com mensagens diferentes e inequívocas, as três condições
  de indisponibilidade: (a) ferramenta Docker ausente da máquina; (b) ferramenta presente mas daemon
  inacessível/parado; (c) daemon acessível mas permissão negada ao usuário.
- **FR-011**: Uma máquina sem nenhum container criado MUST produzir um resultado de sucesso com
  lista vazia e uma indicação explícita de "nenhum container", nunca um erro.
- **FR-012**: Um estado de container não reconhecido pelo Farol MUST resultar naquele container
  sendo exibido com estado "desconhecido" e sem ações acionáveis, MUST NOT invalidar a leitura dos
  demais containers, e MUST NOT ser silenciosamente traduzido para "rodando" ou "parado".
- **FR-013**: Uma ação disparada pelo usuário MUST operar exatamente sobre o container que ele
  selecionou, mesmo que outros containers sejam criados, removidos ou renomeados entre a exibição da
  lista e o acionamento.
- **FR-014**: A consulta do estado MUST desistir em no máximo **3 segundos** quando o daemon não
  responde, reportando erro pontual do widget (FR-006/FR-010b), em vez de bloquear o ciclo de
  atualização. O limite MUST ficar com folga abaixo do orçamento que o Farol concede a uma
  atualização de widget, para que um daemon travado nunca seja confundido com um plugin travado.
- **FR-015**: O sistema MUST NOT duplicar dentro do Farol nenhuma lógica de gerenciamento de
  container — toda consulta de estado e toda operação de ciclo de vida passa pela ferramenta Docker
  já instalada na máquina.
- **FR-016**: O Farol MUST NOT solicitar senha, nem tentar elevar privilégio, para falar com o
  daemon Docker; uma falta de permissão MUST chegar ao usuário como erro de domínio legível
  (FR-010c), não como um prompt de autorização.
- **FR-017**: Enquanto uma ação estiver em andamento sobre um container, as três ações **daquele**
  container MUST ficar não acionáveis e o widget MUST indicar visualmente a operação em curso; as
  ações dos **demais** containers MUST permanecer acionáveis normalmente. Uma atualização periódica
  da lista que chegue durante a operação MUST NOT apagar essa indicação, e a indicação MUST
  desaparecer quando a operação termina (com sucesso ou erro) ou quando o container deixa de existir
  na lista.

### Key Entities

- **Container**: um container Docker local em um instante — sua identidade estável, o nome exibido,
  a imagem de origem e o estado atual (rodando, parado, criado, reiniciando, pausado, em remoção,
  morto ou desconhecido).
- **ContainerAction**: uma operação de ciclo de vida oferecida sobre um container específico —
  iniciar, parar ou reiniciar — com sua disponibilidade condicionada ao estado atual daquele
  container.
- **ContainerActionOutcome**: o resultado de uma tentativa de iniciar/parar/reiniciar — sucesso (com
  o novo estado resultante daquele container) ou um erro de domínio específico e legível.
- **DockerAvailability**: a condição da integração como um todo em um instante — disponível,
  ferramenta ausente, daemon inacessível ou permissão negada — distinta do estado de qualquer
  container individual.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: O usuário identifica corretamente quais containers da sua máquina estão rodando e
  quais estão parados olhando o painel do Farol, sem abrir nenhum terminal ou outra janela.
- **SC-002**: Uma mudança real de estado de um container (por qualquer origem) aparece refletida no
  widget do Farol dentro do mesmo intervalo de atualização já usado pelos demais widgets do produto,
  sem exigir reiniciar o Farol.
- **SC-003**: O usuário reinicia um container parado ou travado inteiramente pelo painel do Farol e
  vê o estado resultante refletido no widget, sem digitar nenhum comando e sem precisar atualizar a
  lista manualmente.
- **SC-004**: 100% das condições de falha previstas (ferramenta ausente, daemon inacessível,
  permissão negada, ação recusada, tempo esgotado, estado não reconhecido) resultam em uma
  indicação visível e legível no widget — nunca em silêncio, em lista vazia enganosa, ou em
  travamento do Farol.
- **SC-005**: A introdução deste widget não altera o comportamento observável dos widgets já
  existentes (git-local, uptime-kuma, openfortivpn-vpn) — nenhuma regressão perceptível ao usuário.
- **SC-006**: Com o daemon Docker travado ou não respondendo, o widget mostra a condição de erro em
  no máximo 3 segundos por tentativa e o restante do painel do Farol continua respondendo
  normalmente — o Farol nunca fica pendurado esperando o Docker.

## Out of Scope

Registrado explicitamente porque o `README.md` § "Integrações previstas" cita mais do que esta
primeira versão entrega:

- **Ver logs de container.** O `README.md` lista "Docker — containers up/down, **logs**". Logs ficam
  fora desta feature: exibir N linhas de log exige uma superfície de UI de **detalhe/drill-down**
  (um painel de texto rolável por container) que o core ainda não possui — hoje todo widget do Farol
  renderiza uma lista plana no painel principal, e nenhum tem conceito de "abrir um item". Essa é
  uma decisão de arquitetura do **core**, independente desta integração e reutilizável por qualquer
  plugin futuro (logs de serviço, corpo de issue, diff de repositório); resolvê-la de dentro de uma
  feature de integração misturaria dois problemas de design não relacionados. Logs entram
  naturalmente como extensão aditiva depois que o core tiver essa superfície, sem invalidar nada
  decidido aqui. Rastreado como issue #10.
- **Criar, remover, pausar/despausar containers, e operações de imagem/rede/volume.** O `README.md`
  delimita o escopo do verbo Agir para Docker em "reiniciar container"; esta feature entrega
  iniciar/parar/reiniciar e para aí. Ações destrutivas (remover) merecem um modelo de confirmação
  que o produto ainda não tem.
- **Filtro, busca ou agrupamento de containers no widget** (por nome, por projeto compose, por
  estado). Sem requisito concreto nesta versão: a lista completa em ordem estável (FR-004) é o
  comportamento correto e completo para o caso de uso de uma máquina de desenvolvimento. Filtro é
  extensão aditiva futura.
- **Docker Compose como unidade de primeira classe** (agrupar containers por projeto, subir/derrubar
  um projeto inteiro). Fora desta versão pelo mesmo motivo do item anterior.
- **Configuração de qual daemon usar.** Esta feature não expõe nenhuma tela de configuração; a
  ferramenta Docker instalada resolve isso pelo ambiente do usuário, como já faz para qualquer outro
  uso dela na máquina.

## Assumptions

- O Farol roda na mesma máquina em que o Docker está instalado — não há controle remoto de
  containers em outra máquina nesta versão. Se o ambiente do usuário apontar a ferramenta Docker
  para um daemon remoto por conta própria, o widget simplesmente refletirá o que aquela ferramenta
  reportar; o Farol não gerencia nem oferece essa configuração.
- O usuário do processo Farol já tem acesso ao daemon Docker (por pertencer ao grupo apropriado, ou
  por usar um daemon rootless próprio). O Farol nunca coleta senha nem escala privilégio: uma falta
  de permissão é reportada como erro legível (FR-010c/FR-016), e resolvê-la é uma ação do usuário
  fora do Farol.
- A questão mais ampla de **ações privilegiadas no produto** (`README.md` § Decisões em aberto —
  "sudo sob demanda, polkit, ou daemon auxiliar") permanece em aberto e **fora do escopo desta
  feature**: esta feature apenas assume o acesso já concedido e reporta claramente quando ele não
  existe, sem propor nem implementar nenhum mecanismo de elevação.
- O ciclo de vida completo dos containers (criar, remover, construir imagem, orquestrar com
  compose) é gerenciado pelo usuário por fora do Farol; esta feature apenas lê o estado e opera as
  três ações de ciclo de vida declaradas (FR-007) sobre containers que já existem.
- Uma lista vazia de containers é um estado legítimo (máquina sem nada criado ainda), não um erro
  (FR-011).
- O conjunto de estados de container tratados como conhecidos (FR-003) é o vocabulário estável
  publicado pelo Docker; a existência de FR-012 é justamente o reconhecimento de que esse
  vocabulário pertence a uma ferramenta externa que pode evoluir sem o Farol.
