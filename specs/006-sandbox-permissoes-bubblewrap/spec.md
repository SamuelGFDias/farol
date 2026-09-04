# Feature Specification: Sandbox de Plugins via Bubblewrap e Aplicação Real do Manifesto de Capacidades

**Feature Branch**: `006-sandbox-permissoes-bubblewrap`

**Created**: 2026-09-03

**Status**: Draft

**Input**: User description: "Fase 3 do roadmap: Sandbox e permissões. Objetivo: fazer o manifesto
de capacidades declarado por cada plugin (`CapabilityManifest`, já existente em `farol-protocol`
desde a feature 002) deixar de ser apenas exibido/decorativo e passar a ser REALMENTE aplicado pelo
core antes de dar qualquer acesso ao plugin — hoje qualquer plugin roda como subprocess comum, com
acesso total de rede, filesystem e execução de comando do usuário que roda o Farol, independente do
que declarou no handshake. Duas frentes centrais, ambas resolvendo o Princípio IV da constitution
(Permissões Explícitas por Manifesto): (1) isolamento via bubblewrap (`bwrap`, já disponível em
`/usr/bin/bwrap` na máquina de dev) — negar rede por padrão, restringir filesystem a um read-only
bind do necessário mais um diretório gravável específico do plugin, mediar a capability `exec`;
(2) vault de segredos — já existe parcialmente (`secrets_store.rs`), avaliar se precisa evoluir
junto com o sandboxing e fechar como conforme ou como precisando de ajuste. Fora de escopo: allowlist
de rede por host individual (fica liga/desliga nesta fatia), UI de aprovação de permissões pelo
usuário, qualquer coisa de registry/instalação de plugin de terceiro."

## Clarifications

### Session 2026-09-03

Todas as respostas desta sessão foram **resolvidas pelo orquestrador sem input do usuário, conforme
instrução da sessão** (modo silencioso: "se no final achar que a implementacao está ambigua, rode um
clarify e voce mesmo responda as perguntas com base na implementacao atual e nas escolhas feitas até
agora"). Base de cada decisão: leitura direta do código atual (`plugin_worker.rs`, `config_store.rs`,
`secrets_store.rs`, `plugins/git-local/config.py`) feita antes de escrever esta spec.

- Q: O que o core faz quando `bwrap` não está instalado na máquina? → A: falha fechada — o(s)
  plugin(s) MUST NOT rodar sem sandbox como fallback silencioso; o core recusa iniciar o plugin e
  reporta um estado de erro claro e distinto dos já existentes (`FailedToStart`/`Crashed`/etc.).
  Rodar sem isolamento contradiz o próprio objetivo da feature — um "fallback" aqui seria a mesma
  falha de segurança que a feature existe para fechar.
- Q: A capability `network` (com `allowed_hosts`) já existente em `farol-protocol` ganha
  granularidade real por host dentro do sandbox nesta fase? → A: não — nesta fatia o sandbox trata
  `network` como liga/desliga (declarada = rede liberada por completo; ausente = rede negada por
  completo). `allowed_hosts` continua existindo no tipo Rust e sendo exibido na UI, mas não é
  aplicado como allowlist real pelo bwrap ainda. Essa divergência entre o texto do manifesto e o que
  é de fato imposto é uma redução deliberada de escopo desta fatia (ver Out of Scope), registrada
  como débito técnico rastreável (issue), não como bug silencioso.
- Q: Como o sandbox concilia com `git-local`, que hoje é o único plugin que precisa ler e escrever
  fora do próprio diretório de código (`scan_root`, `~/dev` por padrão, configurável via
  `~/.config/farol/plugins/git-local/config.toml`, lido pelo próprio plugin — não pelo core, ao
  contrário dos demais plugins)? → A: caso especial nomeado nesta fase, não capability nova
  genérica: o core replica a mesma leitura de `scan_root` (mesmo default, mesmo caminho de config)
  do lado Rust só para calcular o bind adicional de leitura/escrita daquele diretório para esse
  plugin especificamente. Generalizar isso para uma capability de filesystem declarável por
  qualquer plugin fica fora de escopo (ver Out of Scope).
- Q: Investigação técnica durante o planejamento (não prevista ao escrever a primeira versão desta
  spec) achou mais dois casos de plugin de referência cujo funcionamento real depende de acesso que
  o manifesto atual não declara — o que fazer? → A: dois achados, duas respostas diferentes:
  1. `docker-containers` fala com o daemon Docker por um socket Unix local
     (`/var/run/docker.sock` ou o equivalente rootless), não por rede — é o **mesmo tipo de lacuna**
     de `git-local`/`scan_root` (acesso a um caminho de filesystem específico fora do próprio código
     do plugin, hoje não coberto por nenhuma capability declarada), não uma questão de rede. Tratado
     como um segundo caso especial nomeado, mesma disciplina do `scan_root` (FR-008 passa a cobrir
     os dois).
  2. `git-local` (ação `git.fetch`, contra um remote de verdade, não local) e `openfortivpn-vpn`
     (ação `vpn.connect`, para abrir o túnel de verdade) dependem de rede real para funcionar contra
     um remote/servidor genuíno — e nenhum dos dois declara a capability `network` hoje. Esta é uma
     inconsistência pré-existente do manifesto, exposta agora porque é a primeira vez que o
     manifesto passa a ser realmente aplicado — corrigi-la está dentro do escopo desta feature (é
     exatamente o problema que ela existe para resolver), não é scope creep: os dois manifestos
     passam a declarar `network` também (FR-013). **Ressalva de verificação**: a suíte automatizada
     de regressão dos dois plugins (fixtures/harness) não exercita rede real hoje (`git-local` testa
     contra remote local em disco; `openfortivpn-vpn` testa contra um binário fixture que não abre
     túnel de verdade) — então SC-002/FR-011 continuam verificáveis sem depender de rede real
     disponível no ambiente de teste; o cenário de uso real contra um servidor genuíno permanece
     validável só manualmente, mesma ressalva já registrada em `quickstart.md` da feature 004 para
     o Cenário 3.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Um plugin sem a capacidade de rede não alcança rede nenhuma (Priority: P1)

Hoje, instalar um plugin no Farol significa confiar cegamente nele: mesmo que o manifesto declare
só a capability `exec`, nada impede esse plugin de abrir uma conexão de rede e vazar dados ou baixar
algo indesejado. Esta história entrega a garantia central do Princípio IV: um plugin que não
declarou `network` no handshake simplesmente não consegue alcançar rede nenhuma, local ou remota,
porque o core aplica essa restrição no nível do sistema operacional antes mesmo do plugin rodar sua
primeira linha de código — não é uma convenção que o plugin poderia ignorar.

**Why this priority**: é o núcleo da feature — sem isolamento de rede real, o manifesto de
capacidades continua sendo decorativo (o problema que a feature existe para resolver) e a confiança
em qualquer plugin de terceiro futuro (fase 4, registry) fica sem base técnica nenhuma.

**Independent Test**: pode ser testado isoladamente rodando um plugin de referência que NÃO declara
`network` (ex.: `git-local`, `docker-containers`) e confirmando, de dentro do próprio processo
sandboxed, que qualquer tentativa de abrir socket de rede falha — sem depender de nenhuma outra
parte desta feature.

**Acceptance Scenarios**:

1. **Given** um plugin que não declara a capability `network`, **When** o core o inicia, **Then** o
   processo do plugin não consegue resolver DNS nem abrir conexão TCP/UDP para nenhum host, local ou
   remoto.
2. **Given** `uptime-kuma` (o único plugin de referência hoje com `network` declarada), **When** o
   core o inicia, **Then** ele continua alcançando o endpoint `/metrics` configurado normalmente,
   sem nenhuma regressão de comportamento face à feature 002.

---

### User Story 2 - Um plugin roda isolado do restante do filesystem do usuário (Priority: P2)

Hoje um plugin roda com a visão de filesystem completa do usuário que executa o Farol — pode ler
qualquer arquivo pessoal, não só o que precisa para funcionar. Esta história restringe cada plugin a
enxergar, no sandbox, só o necessário para rodar (interpretador, código do próprio plugin) mais
qualquer diretório específico que sua função declarada realmente exige (ex.: `git-local` e o
`scan_root` configurado) — nada além disso, e em particular nunca os arquivos de configuração/
segredo de outros plugins nem os do próprio.

**Why this priority**: depende do sandbox básico já estar de pé (US1) e é o segundo pilar do
Princípio IV, mas tem impacto prático menor no curto prazo que o isolamento de rede (nenhum plugin
de referência hoje tem motivo conhecido para acessar arquivo pessoal do usuário fora do que já
acessa).

**Independent Test**: pode ser testado isoladamente inspecionando, de dentro do processo sandboxed
de um plugin, quais caminhos do filesystem estão visíveis e com que permissão (leitura/escrita) —
sem depender de nenhuma ação de rede da US1.

**Acceptance Scenarios**:

1. **Given** qualquer plugin de referência rodando sob sandbox, **When** o processo tenta ler o
   arquivo `~/.config/farol/secrets.toml` ou o `config.toml` de outro plugin, **Then** o acesso
   falha (arquivo não existe do ponto de vista do processo sandboxed).
2. **Given** `git-local` rodando sob sandbox com `scan_root` configurado para um diretório
   específico, **When** o widget é atualizado e a ação `git.fetch` é invocada num repositório
   daquele diretório, **Then** a leitura da lista de repositórios e o `git fetch` continuam
   funcionando normalmente, sem regressão face à feature 001.

---

### User Story 3 - Segredos configurados nunca ficam expostos além do que o plugin já recebia (Priority: P3)

O core já resolve segredos (`secrets_store.rs`, arquivo `0600`) e os injeta como variável de
ambiente no spawn do processo filho — mas antes desta feature nada impedia esse processo filho de
também enxergar o arquivo de segredos inteiro por outro caminho (bind de filesystem incidental,
herança de working directory, etc.), caso o sandboxing fosse adicionado sem cuidado. Esta história
fecha essa lacuna: confirma (ou corrige, se necessário) que o único jeito de um plugin conhecer um
segredo continua sendo o valor específico que ele mesmo declarou precisar, resolvido pelo core e
injetado como variável de ambiente — nunca por acesso direto a arquivo.

**Why this priority**: prioridade mais baixa porque o comportamento correto (segredo só via env var)
já existe desde a feature 002 — esta história é sobre fechar uma regressão potencial introduzida
pelo próprio sandboxing (US1/US2), não sobre construir capacidade nova.

**Independent Test**: pode ser testado isoladamente configurando um segredo para `uptime-kuma`
(único plugin de referência hoje com segredo real, `API_KEY`) e confirmando que o processo sandboxed
consegue ler o valor via variável de ambiente, mas não consegue localizar nem ler o arquivo
`secrets.toml` original por nenhum caminho de filesystem.

**Acceptance Scenarios**:

1. **Given** `uptime-kuma` configurado com um segredo salvo, **When** o plugin sobe sob sandbox,
   **Then** `FAROL_PLUGIN_UPTIME_KUMA_API_KEY` chega ao processo normalmente (comportamento já
   existente, sem regressão).
2. **Given** o mesmo cenário, **When** o processo do plugin tenta localizar `secrets.toml` por
   qualquer caminho absoluto ou relativo dentro do seu filesystem visível, **Then** o arquivo não
   existe do ponto de vista desse processo.

### Edge Cases

- O que acontece quando `bwrap` não está instalado na máquina? → resolvido em Clarifications: falha
  fechada, plugin não inicia, estado de erro claro (não é um "modo degradado sem sandbox").
- Como o sandbox concilia com o caso especial de `git-local` e seu `scan_root` configurável, e com
  `docker-containers` e o socket Unix do daemon Docker? → resolvido em Clarifications: dois casos
  especiais nomeados, cada um com seu bind adicional específico.
- `git-local`/`openfortivpn-vpn` declaravam manifesto sem `network`, apesar de suas ações reais
  (`git.fetch` contra remote real, `vpn.connect`) dependerem de rede → resolvido em Clarifications:
  correção do manifesto (FR-013), dentro do escopo desta feature.
- Um plugin com a capability `exec` declarada (todos os 4 de referência hoje) precisa continuar
  conseguindo localizar e executar os binários externos específicos de que já depende (`git`,
  `docker`, `openfortivpn-gui`) de dentro do sandbox, sem regressão nas features 001/004/005.
- Um plugin sem `exec` declarada que tentasse iniciar um processo filho MUST falhar de forma
  isolada (o plugin recebe o erro do próprio sistema operacional ao tentar), sem derrubar o core
  nem os demais plugins — mesma disciplina de isolamento de crash já garantida pelo Princípio II.
- O tempo adicional de spawnar cada plugin dentro de `bwrap` (versus subprocess direto, como hoje)
  não pode degradar perceptivelmente o tempo de handshake nem o ciclo de refresh já observado nas
  features anteriores.
- Uma falha do próprio sandboxing (ex.: bind mount mal configurado impedindo o plugin de sequer
  encontrar seu interpretador) precisa ser diferenciável, na mensagem de erro exposta ao usuário, de
  uma falha genuína do plugin (`Crashed`) — senão o usuário não sabe se o problema é do plugin ou da
  infraestrutura de sandbox do Farol.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O core MUST spawnar todo processo filho de plugin dentro de um sandbox de isolamento
  de processo (`bwrap`) — nunca mais como subprocess direto do processo do Farol.
- **FR-002**: O sandbox MUST negar acesso de rede por padrão a qualquer plugin; só um plugin que
  declarou a capability `network` no handshake MUST ter acesso de rede liberado.
- **FR-003**: O sandbox MUST restringir a visão de filesystem do plugin a: (a) leitura do necessário
  para executar o interpretador/binário declarado no spawn, (b) leitura do diretório de código do
  próprio plugin, e (c) nenhum outro caminho do sistema do usuário, exceto a exceção nomeada em
  FR-008.
- **FR-004**: O core MUST NUNCA disponibilizar a um plugin, por bind de filesystem, o arquivo de
  segredos (`secrets.toml`) nem o `config.toml` de qualquer plugin (inclusive o dele mesmo) — os
  valores resolvidos continuam chegando só por variável de ambiente no spawn, como já ocorre hoje.
- **FR-005**: Um plugin sem a capability `exec` declarada MUST NOT conseguir iniciar processo filho
  nenhum de dentro do sandbox.
- **FR-006**: Um plugin com a capability `exec` declarada MUST continuar conseguindo executar os
  binários externos específicos de que sua funcionalidade já depende hoje (`git`, `docker`,
  `openfortivpn-gui`), sem regressão de comportamento face às features 001/004/005.
- **FR-007**: Se o binário `bwrap` não estiver disponível na máquina, o core MUST recusar iniciar
  o(s) plugin(s) afetado(s) e reportar um estado de erro claro e distinto dos já existentes
  (`FailedToStart`/`Crashed`/`Unresponsive`/`VersionIncompatible`) — MUST NOT executar o plugin sem
  sandbox como fallback silencioso.
- **FR-008**: dois casos especiais nomeados de acesso a filesystem além do próprio código do plugin
  MUST continuar funcionando dentro do sandbox, sem capability genérica nova: (a) `git-local` MUST
  continuar conseguindo ler e escrever (via `git fetch`) no diretório configurável `scan_root`
  (`~/dev` por padrão) — o core resolve esse diretório do mesmo jeito que o próprio plugin já faz
  hoje (mesmo caminho de config, mesmo default); (b) `docker-containers` MUST continuar conseguindo
  falar com o daemon Docker pelo socket Unix local (`/var/run/docker.sock` ou o equivalente
  rootless) — o core bind-monta esse socket especificamente para esse plugin.
- **FR-009**: O core MUST continuar detectando `Crashed`/`Unresponsive`/`FailedToStart` normalmente
  para um plugin rodando dentro do sandbox, e a mensagem de erro MUST diferenciar uma falha de
  configuração do próprio sandbox de uma falha genuína do plugin.
- **FR-010**: A capability `network` MUST, nesta fase, ser tratada como liga/desliga pelo sandbox
  (declarada = rede liberada por completo; ausente = rede negada por completo) — granularidade por
  host individual (`allowed_hosts`) fica fora de escopo desta fase (ver Out of Scope) e MUST ser
  registrada como débito técnico rastreável.
- **FR-011**: Os 4 plugins de referência existentes (`git-local`, `uptime-kuma`, `openfortivpn-vpn`,
  `docker-containers`) MUST continuar chegando a `Ready` e funcionando normalmente — todas as User
  Stories já entregues nas features 001/002/004/005 — rodando dentro do sandbox, sem nenhuma
  regressão de comportamento observável.
- **FR-012**: O core MUST continuar resolvendo e injetando segredos/config por variável de ambiente
  no spawn, exatamente como hoje (`secrets_store.rs`/`config_store.rs`) — esta feature não MUST
  alterar esse mecanismo, só garantir (US3) que o sandboxing não abre um caminho adicional de
  vazamento desses arquivos.
- **FR-013**: os manifestos de `git-local` e `openfortivpn-vpn` MUST passar a declarar também a
  capability `network` — achado do planejamento desta feature (não previsto na primeira versão desta
  spec): as ações `git.fetch` (contra um remote real) e `vpn.connect` (para abrir o túnel de verdade)
  já dependiam de rede antes desta feature, só que o manifesto nunca refletiu isso porque nunca foi
  aplicado de verdade. Corrigir essa inconsistência está dentro do escopo desta feature — é o
  problema central que ela resolve — e MUST acontecer antes ou junto da ativação do sandbox para
  esses dois plugins, para não regredir (FR-011) o uso real (não-fixture) dessas duas ações.

### Key Entities

- **Perfil de Sandbox**: conjunto de regras de isolamento — acesso de rede permitido ou negado, e a
  lista de binds de filesystem (leitura ou leitura/escrita) — que o core resolve, no momento do
  spawn, a partir do `CapabilityManifest` declarado por aquele plugin no handshake mais o registro
  fixo do plugin em `known_plugins()`. Não é um novo tipo do protocolo `farol-protocol` (o manifesto
  já existe) — é a tradução, do lado do core, desse manifesto em argumentos concretos de `bwrap`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Um plugin sem a capability `network` declarada não consegue alcançar nenhum endereço
  de rede, local ou remoto, durante toda sua execução — verificável de forma automatizada.
- **SC-002**: Os 4 plugins de referência existentes continuam chegando a `Ready` e passando por
  todos os cenários de smoke já cobertos (`tests/integration/harness.sh`) rodando dentro do sandbox,
  sem nenhuma regressão de comportamento observável.
- **SC-003**: Um plugin sem a capability `exec` declarada não consegue iniciar processo filho
  nenhum.
- **SC-004**: O core recusa iniciar qualquer plugin — em vez de rodá-lo sem isolamento — quando o
  `bwrap` não está disponível na máquina, com um estado de erro distinguível dos demais.
- **SC-005**: Nenhum plugin consegue ler o arquivo de segredos do Farol nem o `config.toml` de
  qualquer plugin (inclusive o próprio) por acesso direto de filesystem, mesmo tentando.
- **SC-006**: O tempo entre o spawn de um plugin e ele alcançar `Ready` não aumenta de forma
  perceptível (mesma ordem de grandeza observada hoje sem sandbox) para nenhum dos 4 plugins de
  referência.

## Out of Scope

- Allowlist de rede por host individual dentro do sandbox — `network` fica liga/desliga nesta
  fatia (FR-010); refinar para granularidade por host é iteração futura, registrada como débito.
- UI para o usuário aprovar ou revisar as capabilities de cada plugin antes de ativá-lo — fica para
  a fase de polimento social (item 5 do roadmap do `README.md`).
- Qualquer coisa relacionada a registry ou instalação de plugin de terceiro — fase 4 do roadmap.
- Generalizar acesso a filesystem além do próprio código do plugin como uma capability declarável
  por qualquer plugin — o caso de `git-local`/`scan_root` é resolvido nesta fase como exceção
  nomeada (FR-008), não como sistema genérico.
- Sandboxing de rede em granularidade de porta ou protocolo (ex.: permitir só HTTPS de saída).
- Empacotamento de `bwrap` junto do binário do Farol para máquinas onde ele não está instalado —
  esta fase assume que já está disponível (ver Assumptions).

## Assumptions

- `bwrap` (bubblewrap) está instalado na máquina de desenvolvimento usada para implementar e
  validar esta feature (`/usr/bin/bwrap`, confirmado antes de escrever esta spec) — instalar ou
  empacotar o `bwrap` para outras máquinas não é responsabilidade desta fase (FR-007 cobre o caso de
  ausência com falha fechada, não com instalação automática).
- Os 4 plugins de referência existentes continuam sendo processos Python3 puro (`command: "python3"`
  em `known_plugins()`) — nenhum plugin novo é adicionado nesta feature.
- `git-local` (diretório `scan_root`) e `docker-containers` (socket Unix do daemon Docker) são os
  dois casos especiais nomeados de acesso a filesystem além do próprio código do plugin (FR-008);
  nenhum outro plugin existente hoje tem necessidade equivalente.
- A verificação automatizada desta feature (testes/harness) não depende de rede real disponível no
  ambiente: nenhuma suíte de regressão hoje exercita `git.fetch` contra um remote de verdade nem
  `vpn.connect` contra um túnel de verdade (ambos usam fixture/remote local em disco) — a correção
  do manifesto (FR-013) é validável automaticamente pela declaração em si (capability presente) e
  manualmente, por leitura de código, quanto ao uso real contra rede/servidor genuínos.
- Ambiente Linux com suporte a namespaces do kernel (user/mount/network) — pré-requisito do próprio
  `bwrap`, já coberto pelo Princípio I da constitution (app nativo Linux, sem navegador).
- O mecanismo de resolução de segredos/config por variável de ambiente no spawn (`secrets_store.rs`/
  `config_store.rs`, decisão já emendada na constitution v1.0.0) permanece o mecanismo de vault desta
  fase — não será substituído por keyring do sistema operacional nem por outro armazenamento.
