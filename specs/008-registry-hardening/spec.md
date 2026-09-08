# Feature Specification: Registry — Índice Central, Instalação com Build e Capability de Filesystem Genérica

**Feature Branch**: `008-registry-hardening`

**Created**: 2026-09-07

**Status**: Draft

**Input**: User description: "Hardening do registry de instalação de plugins (feature 007): fechar
débito técnico das issues #15 (instalação in-app via UI do iced, hoje é só subcommand de CLI), #16
(instalação não cobre plugin que exija passo de build ou asset binário próprio, MVP cobre só
extração direta de código-fonte interpretado), #17 (capability de filesystem genérica declarável
por um plugin de terceiro, hoje só network/exec) e #18 (criar e publicar de verdade um
repositório-índice GitHub central com CI de validação de PRs)."

## Clarifications

### Session 2026-09-07

- Q: O repositório-índice central (issue #18) deve ser criado e publicado de verdade nesta feature,
  ou a spec deve só ensinar o core a consumir um índice já existente? → A: criar e publicar de
  verdade — a spec cobre a criação e publicação de um repositório-índice GitHub real, com um
  pipeline de CI que valida pull requests de novos plugins antes de aceitá-los no índice. Nome e
  organização do repositório são decisões a tomar durante a implementação (dono da conta GitHub do
  usuário), não definidas nesta spec.
- Q: Qual mecanismo cobre instalação de plugin que exija build ou asset binário próprio (issue
  #16)? → A: comando `build` declarado no manifesto do plugin, executado localmente pelo instalador
  logo após extrair o código-fonte da release, antes de disponibilizar o plugin para descoberta
  (US1 da feature 007). Requer que o toolchain necessário (ex.: `cargo`, `go`) já esteja presente na
  máquina do usuário — ausência do toolchain resulta em falha clara e específica da instalação
  ("comando de build falhou: <ferramenta> não encontrada"), nunca uma tentativa silenciosa de
  contornar ou pular o build.
- Q: Que forma a capability de filesystem genérica para plugin de terceiro deve ter (issue #17)? →
  A: manifesto declara caminhos absolutos explícitos (modo leitura ou leitura/escrita) que o core
  valida contra uma denylist de caminhos sensíveis do sistema (ex.: `~/.ssh`, `/etc`, diretórios de
  segredo do próprio Farol) antes de conceder o bind — mesma disciplina de "negado por padrão,
  concedido explicitamente" já usada por `network`/`exec` (feature 006). Escolhida por ser a mais
  extensível a longo prazo: não exige que o core hardcode um novo padrão nomeado por plugin (como
  `scan_root`/socket Docker fazem hoje) e é compatível com uma futura UI de revisão/aprovação de
  capabilities antes de ativar um plugin instalado — já registrada como débito técnico na feature
  006 (roadmap item 5, "polimento social") — que funcionaria como camada extra de segurança acima
  da denylist.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Descobrir e instalar um plugin sem saber o repositório de antemão (Priority: P1)

Hoje `farol install <owner>/<repo>` exige que o usuário já saiba exatamente qual repositório GitHub
hospeda o plugin desejado. Esta história entrega o repositório-índice central: uma lista curada e
publicada de plugins de terceiros disponíveis, e um comando que permite instalar um plugin pelo
nome dele no índice, sem precisar saber `owner/repo` de antemão. Publicar um plugin novo no índice
passa por um pull request nesse repositório, validado automaticamente por CI antes de ser aceito.

**Why this priority**: sem um índice central de verdade, a feature 007 só resolve "instalar plugin
que eu já sei onde está" — não resolve "descobrir que plugins de terceiros existem". É o item mais
citado no roadmap original do Princípio VII (Registry Federado) e o único, dos quatro cobertos por
esta spec, que depende de uma decisão de infraestrutura externa (existência real do repositório).

**Independent Test**: pode ser testado isoladamente publicando um manifesto de plugin de teste no
repositório-índice (via PR real, passando pela CI), rodando `farol install <nome-do-plugin>` (sem
`owner/repo`) e confirmando que o comando resolve o nome para o repositório correto do índice antes
de seguir o fluxo de instalação já existente da feature 007 (US2).

**Acceptance Scenarios**:

1. **Given** um repositório-índice publicado contendo uma entrada válida para um plugin de teste,
   **When** o usuário roda `farol install <nome-do-plugin>`, **Then** o comando busca a entrada
   correspondente no índice, resolve o `owner/repo` de origem e instala exatamente como já faz hoje
   para um `owner/repo` explícito (feature 007, US2) — mesmo comportamento de sucesso/falha/edge
   case já coberto lá.
2. **Given** um nome de plugin que não existe no índice, **When** o usuário roda `farol install
   <nome-inexistente>`, **Then** o comando falha com uma mensagem clara distinguindo "não encontrado
   no índice" de qualquer outra falha (rede, manifesto inválido).
3. **Given** um pull request abrindo uma entrada nova no repositório-índice com um manifesto de
   plugin malformado (sintaxe inválida, campo obrigatório ausente, `plugin_name` colidindo com um já
   existente no índice), **When** a CI roda sobre esse PR, **Then** a validação falha e reporta o
   motivo específico diretamente no PR, impedindo o merge — nenhuma entrada inválida chega a ser
   publicada no índice.
4. **Given** um pull request com um manifesto de plugin válido e um `plugin_name` não colidente,
   **When** a CI roda sobre esse PR, **Then** a validação passa e o PR fica pronto para revisão
   humana antes do merge (a validação automática não substitui aprovação humana do PR).
5. **Given** `owner/repo` explícito continua funcionando (feature 007, US2) mesmo com o índice
   existindo, **When** o usuário roda `farol install <owner>/<repo>` diretamente, **Then** o
   comportamento é idêntico ao de antes desta feature — o índice é um caminho adicional de
   descoberta, nunca uma substituição obrigatória do caminho direto.

---

### User Story 2 - Instalar um plugin que exige build local antes de rodar (Priority: P2)

Um plugin de terceiro escrito em uma linguagem compilada (Rust, Go, etc.) declara, no seu
manifesto, um comando de build. `farol install` executa esse comando localmente logo após extrair o
código-fonte da release, antes de disponibilizar o plugin para descoberta — cobrindo o caso que a
feature 007 deixou de fora (só extração direta de código interpretado).

**Why this priority**: depende da US1 só para descoberta pelo índice, não para funcionar — pode ser
testada com `owner/repo` direto (já existente). Prioridade P2 porque amplia o universo de plugins
instaláveis (Princípio II — plugin em qualquer linguagem), mas não é pré-requisito de nenhuma outra
história desta spec.

**Independent Test**: pode ser testado isoladamente com um repositório de fixture cujo manifesto
declare um comando de build trivial (ex.: gerar um arquivo que o próprio comando de spawn do plugin
depois lê), sem depender do índice central da US1.

**Acceptance Scenarios**:

1. **Given** uma release cujo manifesto declara um comando de build, **When** o usuário roda `farol
   install <owner>/<repo>`, **Then** o comando extrai o código-fonte, executa o comando de build
   declarado no diretório extraído, e só disponibiliza o plugin para descoberta (feature 007, US1)
   se o build terminar com código de saída `0`.
2. **Given** o comando de build declarado falha (código de saída diferente de `0`, ou a ferramenta
   necessária não está instalada na máquina do usuário), **When** a instalação roda o build, **Then**
   a instalação inteira falha com uma mensagem específica ("comando de build falhou: <detalhe>"), e
   nenhum diretório parcial fica visível para a descoberta de plugins (mesma disciplina de
   atomicidade já exigida pela feature 007, FR-007).
3. **Given** um manifesto sem comando de build declarado (caso já coberto pela feature 007), **When**
   o usuário instala esse plugin, **Then** o comportamento é idêntico ao de antes desta feature —
   nenhuma etapa de build é tentada.

---

### User Story 3 - Plugin de terceiro declara acesso a um caminho específico do filesystem (Priority: P3)

Um plugin de terceiro instalado via registry (feature 007) hoje só pode receber as capabilities
`network`/`exec`. Esta história permite que o manifesto do plugin declare caminhos absolutos
específicos do filesystem do usuário (leitura ou leitura/escrita) necessários ao funcionamento do
plugin — o core concede esse acesso via sandbox (feature 006) só depois de validar cada caminho
contra uma denylist de caminhos sensíveis.

**Why this priority**: amplia o que um plugin de terceiro consegue fazer sem exigir uma nova
categoria de capability nomeada por caso (como `scan_root`/socket Docker hoje) — mas nenhuma das
outras histórias desta spec depende dela, e o risco de segurança envolvido justifica vir depois de
US1/US2 estarem estáveis.

**Independent Test**: pode ser testado isoladamente com um manifesto de plugin de teste declarando
um caminho de filesystem específico (ex.: um diretório temporário criado só para o teste) e
confirmando, via inspeção dos argumentos de sandbox gerados (mesmo padrão de teste unitário já usado
por `sandbox::build_bwrap_args`), que o bind aparece só quando o caminho passa na validação.

**Acceptance Scenarios**:

1. **Given** um manifesto de plugin de terceiro declarando um caminho absoluto válido (fora da
   denylist) em modo leitura, **When** o Farol inicia esse plugin, **Then** o sandbox (feature 006)
   concede bind de leitura desse caminho específico, sem acesso de escrita.
2. **Given** um manifesto declarando um caminho absoluto válido em modo leitura/escrita, **When** o
   Farol inicia esse plugin, **Then** o sandbox concede bind de leitura/escrita desse caminho
   específico.
3. **Given** um manifesto declarando um caminho que está na denylist de caminhos sensíveis (ex.:
   `~/.ssh`, `/etc`, diretório de segredos do próprio Farol), **When** o Farol tenta iniciar esse
   plugin, **Then** a inicialização desse plugin específico falha com um aviso claro identificando o
   caminho rejeitado — o Farol continua funcionando normalmente com os demais plugins (mesma
   disciplina de isolamento de falha da feature 007, FR-004).
4. **Given** um manifesto declarando um caminho relativo (não absoluto), **When** o Farol tenta
   carregar esse manifesto, **Then** o manifesto é rejeitado na validação (mesma disciplina de
   manifesto malformado da feature 007, FR-004) — só caminhos absolutos são aceitos, eliminando
   ambiguidade sobre a partir de onde resolver um caminho relativo.

---

### User Story 4 - Instalar um plugin de dentro da janela do Farol (Priority: P4)

Hoje `farol install` é uma subcommand de linha de comando, tratada antes de a janela `iced` abrir.
Esta história adiciona uma tela dentro da própria interface gráfica do Farol onde o usuário digita
um nome de plugin (do índice, US1) ou um `owner/repo` direto, aciona a instalação, e acompanha o
resultado (sucesso ou erro) sem sair da aplicação — reaproveitando o mesmo mecanismo de instalação
por trás, sem lógica de instalação duplicada.

**Why this priority**: é a de menor prioridade porque o comando de CLI já resolve o problema
funcional (instalar um plugin) — esta história é puramente de conveniência de UX, sem desbloquear
nenhuma capacidade nova do produto além do que US1/US2/US3 já entregam.

**Independent Test**: pode ser testado isoladamente abrindo o Farol, navegando até a tela de
instalação, digitando um nome/`owner/repo` de teste conhecido, e confirmando que o resultado exibido
na tela corresponde ao que o comando de CLI equivalente produziria — sem depender de nenhuma lógica
de instalação nova (só a UI é nova, o mecanismo por trás é o mesmo de US1/US2/feature 007).

**Acceptance Scenarios**:

1. **Given** o Farol aberto, **When** o usuário navega até a tela de instalação de plugin e digita um
   nome de plugin válido do índice (ou um `owner/repo` direto) e aciona a instalação, **Then** a tela
   exibe um indicador de progresso enquanto a instalação roda e, ao final, uma mensagem de sucesso
   clara — sem travar o restante da interface do Farol enquanto a instalação está em andamento.
2. **Given** a mesma tela, **When** a instalação falhar por qualquer motivo já coberto pelo comando
   de CLI (feature 007 FR-010, mais os novos desta spec), **Then** a tela exibe a mesma mensagem de
   erro específica que o comando de CLI produziria, não um erro genérico.
3. **Given** uma instalação concluída com sucesso pela tela, **When** o usuário reabre o Farol na
   próxima vez, **Then** o plugin recém-instalado é descoberto e iniciado normalmente (feature 007,
   US1) — mesmo efeito de uma instalação feita pela CLI.

### Edge Cases

- Dois plugins diferentes no repositório-índice central declaram o mesmo nome de instalação
  (distinto de `plugin_name` do manifesto, que já tem disciplina de colisão da feature 007) → a CI de
  validação do índice (US1) MUST rejeitar o PR que tentar introduzir esse nome duplicado, antes de
  chegar a `main` do índice.
- O comando de build de um plugin (US2) tenta acessar rede ou escrever fora do diretório de extração
  temporário → o comando de build roda fora do sandbox de execução do plugin em si (que só existe
  depois de instalado) — MUST ser documentado como responsabilidade do usuário revisar o manifesto
  antes de instalar um plugin de terceiro não confiável, mesma disciplina de confiança explícita já
  assumida pela feature 007 para o próprio código-fonte instalado.
- Um caminho de filesystem declarado (US3) existe mas não tem as permissões de sistema operacional
  necessárias para o modo solicitado (ex.: leitura/escrita pedida num caminho só de leitura para o
  usuário do SO) → a falha aparece no momento em que o plugin tenta de fato usar o caminho (erro de
  I/O do próprio SO dentro do sandbox), não uma validação antecipada do Farol — fora de escopo prever
  permissões de SO na validação do manifesto.
- Denylist de caminhos sensíveis (US3) precisa cobrir variação de localização por ambiente (ex.:
  `$XDG_CONFIG_HOME` custom, não só `~/.config`) → a denylist MUST resolver variáveis de ambiente
  relevantes (`$HOME`, `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`) no momento da validação, não comparar
  contra caminhos literais hardcoded que assumem localização default.
- Repositório-índice (US1) fica temporariamente inacessível (GitHub fora do ar, rate limit) →
  `farol install <nome-do-plugin>` MUST falhar com mensagem clara distinguindo essa causa de "nome
  não encontrado no índice" — nunca interpretar indisponibilidade do índice como "plugin não existe".

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: o sistema MUST manter um repositório-índice GitHub central publicado, listando
  plugins de terceiros disponíveis para instalação por nome (além da instalação direta por
  `owner/repo` já existente da feature 007).
- **FR-002**: o repositório-índice MUST ter um pipeline de CI que valida automaticamente todo pull
  request propondo uma entrada nova ou alteração de entrada existente — rejeitando manifesto
  malformado e nome de instalação colidente com uma entrada já existente, antes de permitir o merge.
- **FR-003**: `farol install <nome-do-plugin>` MUST resolver esse nome contra o repositório-índice
  para encontrar o `owner/repo` de origem, e então seguir exatamente o mesmo fluxo de instalação já
  definido pela feature 007 (FR-006 a FR-010) — nenhuma duplicação de lógica de instalação entre o
  caminho "por nome" e o caminho "por `owner/repo` direto".
- **FR-004**: `farol install <owner>/<repo>` (caminho direto, sem passar pelo índice) MUST continuar
  funcionando exatamente como definido na feature 007, sem nenhuma mudança de comportamento —
  regressão zero.
- **FR-005**: o manifesto de um plugin MAY declarar um comando de build; quando declarado, o
  instalador MUST executá-lo no diretório de código-fonte extraído, após a extração e antes de
  disponibilizar o plugin para descoberta (feature 007, FR-006).
- **FR-006**: se o comando de build declarado terminar com código de saída diferente de `0`, a
  instalação inteira MUST falhar com uma mensagem específica identificando que a falha veio do
  build (não da extração ou validação de manifesto) — e MUST NOT deixar nenhum diretório parcial
  visível para a descoberta de plugins (mesma disciplina de atomicidade da feature 007, FR-007).
- **FR-007**: o manifesto de um plugin MAY declarar uma ou mais capabilities de filesystem, cada uma
  com um caminho absoluto e um modo (`read` ou `read_write`).
- **FR-008**: o core MUST validar cada caminho de filesystem declarado contra uma denylist de
  caminhos sensíveis do sistema (resolvendo variáveis de ambiente relevantes como `$HOME`,
  `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME` no momento da validação) antes de conceder qualquer bind via
  sandbox (feature 006).
- **FR-009**: um manifesto declarando um caminho de filesystem que caia na denylist, ou um caminho
  relativo (não absoluto), MUST fazer o core rejeitar a inicialização daquele plugin específico com
  aviso claro — MUST NOT impedir os demais plugins de funcionar normalmente (mesma disciplina de
  isolamento de falha da feature 007, FR-004).
- **FR-010**: um caminho de filesystem aprovado na validação (FR-008) MUST ser concedido ao plugin
  exatamente no modo declarado (`read` nunca vira `read_write` por omissão, e vice-versa).
- **FR-011**: o Farol MUST oferecer uma tela dentro da própria interface gráfica (`iced`) para
  instalar um plugin (por nome do índice ou `owner/repo` direto), reaproveitando o mesmo mecanismo
  de instalação do comando de CLI, sem lógica de instalação duplicada.
- **FR-012**: a tela de instalação MUST exibir progresso enquanto a instalação roda e MUST NOT
  bloquear o restante da interface do Farol durante esse tempo.
- **FR-013**: a tela de instalação MUST exibir a mesma mensagem de erro específica que o comando de
  CLI equivalente produziria para cada causa de falha já definida (feature 007, FR-010, mais FR-006
  e FR-009 desta spec) — nunca uma mensagem de erro genérica sem contexto.
- **FR-014**: os 4 plugins de referência e o fluxo de instalação da feature 007 já entregue MUST
  continuar funcionando exatamente como hoje — regressão zero em todas as capacidades já entregues
  pelas features 001-007.

### Key Entities

- **Repositório-Índice**: repositório GitHub central, com CI própria, listando entradas de plugins
  de terceiros disponíveis por nome (nome de instalação, `owner/repo` de origem). Fonte de verdade
  para `farol install <nome-do-plugin>` (US1); não substitui a instalação direta por `owner/repo`.
- **Comando de Build** (novo campo do manifesto de plugin, feature 007): comando executado
  localmente pelo instalador, no diretório de código-fonte extraído, antes de disponibilizar o
  plugin para descoberta — presença opcional, ausência preserva o comportamento já existente.
- **Capability de Filesystem** (novo campo do manifesto de plugin, feature 007): lista de caminhos
  absolutos e seus modos (`read`/`read_write`) que um plugin de terceiro declara precisar, sujeita à
  validação contra a denylist de caminhos sensíveis antes de virar bind real via sandbox.
- **Denylist de Caminhos Sensíveis**: lista mantida pelo core de padrões de caminho (resolvendo
  variáveis de ambiente do usuário) que nunca podem ser concedidos a um plugin de terceiro via
  capability de filesystem, independente do que o manifesto declarar.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: um plugin de terceiro publicado no repositório-índice (via PR aprovado e mergeado) é
  instalável só pelo nome, sem o usuário precisar saber `owner/repo` de antemão.
- **SC-002**: um pull request introduzindo um manifesto malformado ou nome de instalação colidente no
  repositório-índice nunca chega a `main` sem a CI reportar o problema especificamente.
- **SC-003**: um plugin de terceiro escrito em linguagem que exija build (ex.: Rust, Go) chega a
  `Ready` depois de `farol install`, sem nenhuma intervenção manual além do próprio comando.
- **SC-004**: um plugin de terceiro consegue ler (ou ler/escrever) um caminho específico do
  filesystem do usuário declarado em seu manifesto, sem receber acesso a nenhum caminho fora do que
  foi explicitamente declarado e aprovado.
- **SC-005**: nenhum caminho da denylist de sensíveis é concedido a um plugin de terceiro, mesmo
  quando declarado explicitamente no manifesto — verificável automaticamente por teste.
- **SC-006**: um usuário consegue instalar um plugin inteiramente de dentro da janela do Farol, sem
  precisar abrir um terminal.
- **SC-007**: todos os cenários de smoke já cobertos pelas features 001-007 continuam passando sem
  nenhuma regressão de comportamento observável.

## Out of Scope

- Autenticação para repositórios privados no comando de instalação (mantido da feature 007) —
  índice central e instalação direta cobrem só repositórios GitHub públicos.
- Versionamento/pinning de versão específica na instalação (mantido da feature 007) — instala sempre
  a release/tag mais recente, tanto por nome do índice quanto por `owner/repo` direto.
- Sandboxing do próprio comando de build (US2) — o comando declarado roda com os mesmos privilégios
  do processo de instalação, não dentro do sandbox `bwrap` (que só se aplica ao plugin já instalado
  em execução, feature 006). Rastreado como possível débito técnico futuro caso um comando de build
  malicioso vire vetor de ataque relevante.
- UI de revisão/aprovação de capabilities antes de ativar um plugin instalado (mencionada nas
  Clarifications como direção futura) — esta spec entrega só a denylist como camada de segurança;
  uma tela de revisão explícita pelo usuário continua sendo débito técnico separado (já registrado na
  feature 006, roadmap item 5).
- Suporte a asset binário pré-compilado como alternativa ao comando de build (uma das opções
  descartadas nas Clarifications para a issue #16) — só a forma "comando de build local" está no
  escopo desta spec.
- Hot-reload de plugin recém-instalado pela tela in-app (US4) enquanto o Farol já está aberto —
  mesma disciplina já assumida pela feature 007: efeito só aparece na próxima abertura.

## Assumptions

- O usuário que publica um plugin no repositório-índice (US1) é o mesmo que decide, durante a
  implementação desta feature, o nome/organização GitHub real desse repositório — a spec não fixa
  esse valor.
- A CI do repositório-índice roda em GitHub Actions (mesma plataforma já usada pelo próprio
  ecossistema GitHub que todo o Princípio VII depende), sem introduzir uma plataforma de CI nova.
- O toolchain necessário para o comando de build de um plugin (US2) é responsabilidade do usuário
  instalar previamente — o Farol não gerencia nem baixa toolchains de build por conta própria.
- A denylist de caminhos sensíveis (US3) cobre, no mínimo, diretórios de credenciais/chaves SSH do
  usuário (`~/.ssh`), configuração de sistema (`/etc`), e o próprio diretório de segredos do Farol
  (`config_store`/`secrets_store`, feature 002) — lista exata a refinar em `plan.md`/`research.md`,
  não fixada nesta spec.
- O mecanismo de bind de filesystem por capability (US3) reaproveita a composição de argumentos de
  `bwrap` já existente (`sandbox::build_bwrap_args`, feature 006), sem exigir um mecanismo de sandbox
  paralelo.
