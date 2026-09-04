# Feature Specification: Registry — Descoberta e Instalação de Plugins de Terceiros via GitHub

**Feature Branch**: `007-registry-instalacao-plugins-github`

**Created**: 2026-09-04

**Status**: Draft

**Input**: User description: "Fase 4 do roadmap: Registry — repo-índice, CI de validação, instalação
in-app, template de plugin. Princípio VII da constitution (Registry Federado sem Infra Própria):
descoberta e distribuição de plugins usa o GitHub como plataforma, sem infraestrutura própria do
Farol — repositório-índice central, publicação via pull request, instalação puxando releases
diretamente do repositório de cada plugin. Objetivo real desta feature: fechar a lacuna
arquitetural que hoje faz `known_plugins()` (`plugin_worker.rs`) ser uma lista fixa de 4 plugins
hardcoded no código Rust do core — instalar um plugin novo hoje exige editar o core e recompilar,
contradizendo o próprio propósito de um sistema de plugins extensível. Depois desta feature, o core
MUST descobrir plugins instalados dinamicamente (sem recompilar) e MUST oferecer um caminho de
instalação que puxa a release mais recente do repositório GitHub de um plugin de terceiro. Herda
diretamente uma consequência já prevista na feature 006 (`specs/006-sandbox-permissoes-bubblewrap/
research.md`, decisão D1): 'um plugin de terceiro instalado via registry não tem uma entrada de
código hardcoded em known_plugins() — resolver a fonte de verdade do sandbox para esse caso... é
trabalho da própria feature de registry, não daquela.' A fonte de verdade do perfil de sandbox de um
plugin instalado passa a ser o manifesto local gravado no disco no momento da instalação — nunca o
handshake em runtime do próprio plugin (mesma disciplina de D1, generalizada)."

## Clarifications

### Session 2026-09-04

Todas as respostas desta sessão foram **resolvidas pelo orquestrador sem input do usuário, conforme
instrução da sessão** (modo silencioso).

- Q: "Repo-índice" (repositório GitHub separado, central, listando plugins publicados) e "CI de
  validação" (pipeline que valida PRs nesse índice) são artefatos de infraestrutura **fora** deste
  repositório `farol` — criá-los/publicá-los de verdade está dentro do escopo desta feature? → A:
  não. Criar e publicar um repositório GitHub novo é uma decisão de infraestrutura externa (nome,
  organização, visibilidade, manutenção contínua) que exige decisão explícita do usuário, não uma
  chamada de implementação a tomar em modo silencioso. Esta feature entrega o que o **core** precisa
  para o registry funcionar quando esse índice existir — o formato do manifesto que um índice
  apontaria, e a instalação direta a partir do repositório de um plugin específico (`owner/repo`),
  sem depender de o índice já existir. O índice em si (e sua CI) fica documentado como Fora de
  Escopo, tratado como trabalho futuro fora deste código.
- Q: "Instalação in-app" — dentro da própria janela gráfica do Farol (iced), com campo de texto,
  botão, barra de progresso? → A: não nesta fase — MVP entrega uma subcommand de linha de comando no
  mesmo binário (`farol install <owner>/<repo>`), tratada em `main()` antes de entrar no loop
  `iced::application`, sem nenhum widget novo de UI. Uma instalação inteiramente dentro da UI
  gráfica (com progresso visual, campo de busca, etc.) é scope real de UX que merece sua própria
  iteração — registrada como débito técnico rastreável (issue), não implementada aqui. É uma leitura
  pragmática, não literal, de "instalação in-app": o comando roda no mesmo binário do app, só que
  antes da janela abrir, não dentro dela.
- Q: Plugin em qualquer linguagem (Princípio II) — a instalação via release do GitHub cobre um
  binário compilado ou só código-fonte interpretado? → A: só código-fonte desta fase — a instalação
  extrai o tarball de código-fonte que o próprio GitHub gera automaticamente para cada release/tag
  (`https://github.com/<owner>/<repo>/archive/refs/tags/<tag>.tar.gz`, sempre disponível, sem exigir
  que o autor do plugin suba um asset binário à parte). Funciona sem nenhum passo de build extra
  para plugins interpretados (Python, como os 4 de referência já existentes) — cobre exatamente o
  caso que os plugins de referência já provam. Instalar um plugin que exija compilação/asset binário
  próprio fica fora de escopo, rastreado como débito técnico.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Farol carrega plugins instalados sem recompilar o core (Priority: P1)

Hoje, o único jeito de o Farol "conhecer" um plugin novo é um desenvolvedor editar
`plugin_worker::known_plugins()` (Rust) e recompilar o core — o oposto de um sistema de plugins
extensível. Esta história resolve a lacuna raiz: o core passa a descobrir, a cada início, quais
plugins estão instalados localmente (lendo um manifesto por plugin de um diretório de dados do
usuário) e os inicia exatamente como já inicia os 4 plugins de referência hoje — handshake, sandbox
(feature 006), widgets, tudo igual, só a fonte de onde a lista de plugins vem que muda.

**Why this priority**: sem isso, "instalar um plugin" (US2) não teria efeito nenhum observável — é
o alicerce que torna o resto da feature possível, e sozinho já resolve o problema mais nomeado do
roadmap ("Registry" existe para que instalar plugin não precise recompilar o core).

**Independent Test**: pode ser testado isoladamente colocando manualmente um manifesto válido (sem
passar pelo fluxo de instalação da US2) no diretório de dados esperado e confirmando que o Farol,
ao iniciar, tenta spawná-lo como qualquer outro plugin — sem depender de nenhuma ação de rede.

**Acceptance Scenarios**:

1. **Given** nenhum plugin de terceiro instalado, **When** o Farol inicia, **Then** só os 4 plugins
   de referência aparecem — comportamento idêntico ao de antes desta feature (regressão zero).
2. **Given** um manifesto válido de um plugin de terceiro presente no diretório de dados esperado
   (com seu próprio código-fonte ao lado), **When** o Farol inicia, **Then** esse plugin também é
   spawnado, passa pelo handshake e, se tudo correr bem, chega a `Ready` — mesma máquina de estados
   já usada pelos 4 plugins de referência, sem caminho especial.
3. **Given** um manifesto malformado (TOML inválido, campo obrigatório ausente, capability
   desconhecida) no diretório de dados, **When** o Farol inicia, **Then** esse plugin específico é
   ignorado com um aviso claro (log/diagnóstico) — o Farol continua subindo normalmente com os
   demais plugins, nunca trava nem recusa iniciar por causa de um manifesto de terceiro quebrado.

---

### User Story 2 - Instalar um plugin a partir do repositório GitHub dele (Priority: P2)

O usuário roda `farol install <owner>/<repo>` (ex.: `farol install someuser/farol-plugin-jira`); o
Farol baixa a release/tag mais recente do repositório indicado, confirma que ela contém um
manifesto de plugin válido, e deixa tudo pronto no diretório de dados local para a próxima vez que o
Farol abrir (User Story 1) já carregar esse plugin.

**Why this priority**: depende da US1 já existir (o manifesto só tem efeito se o core souber lê-lo);
é o mecanismo que de fato traz um plugin de fora para dentro da máquina do usuário, completando o
propósito do Princípio VII (instalação puxando release direto do repositório do plugin, sem infra
própria do Farol).

**Independent Test**: pode ser testado isoladamente rodando o comando contra um repositório GitHub
público real conhecido (ex.: um dos 4 plugins de referência, publicado num repo próprio para esse
fim de teste, ou um repositório de fixture criado especificamente para isso) e inspecionando o
diretório de dados resultante — sem depender de abrir a janela do Farol.

**Acceptance Scenarios**:

1. **Given** um repositório GitHub público válido, com ao menos uma release/tag, contendo um
   manifesto de plugin válido na raiz, **When** o usuário roda `farol install <owner>/<repo>`,
   **Then** o comando baixa o código-fonte dessa release, extrai para o diretório de dados local sob
   o nome do plugin declarado no manifesto, e termina com uma mensagem de sucesso e código de saída
   `0`.
2. **Given** um repositório sem nenhuma release/tag publicada, **When** o usuário tenta instalar,
   **Then** o comando falha com uma mensagem clara ("nenhuma release encontrada"), sem deixar
   nenhum diretório parcial/corrompido no diretório de dados.
3. **Given** uma release que não contém um manifesto de plugin válido na raiz do código-fonte,
   **When** o usuário tenta instalar, **Then** o comando falha com uma mensagem clara identificando
   o problema (manifesto ausente ou malformado), sem instalar nada.
4. **Given** um plugin já instalado anteriormente, **When** o usuário roda o mesmo comando de novo
   (ex.: para atualizar para uma release mais nova), **Then** a instalação anterior é substituída
   pela nova de forma limpa (sem misturar arquivos de duas versões diferentes).

---

### User Story 3 - Template de referência para escrever um plugin novo (Priority: P3)

Uma pessoa que queira escrever um plugin novo para o Farol encontra, neste repositório, um esqueleto
mínimo documentado (manifesto de exemplo + `main.py` mínimo respondendo `handshake/hello`) que ela
pode copiar como ponto de partida — sem precisar ler os 4 plugins de referência inteiros (que
carregam lógica de produto específica) só para descobrir o contrato mínimo do protocolo.

**Why this priority**: menor prioridade porque os 4 plugins de referência já servem, na prática,
como exemplo funcional — este template só reduz o atrito de "por onde eu começo", não desbloqueia
nenhuma capacidade nova do produto.

**Independent Test**: pode ser testado isoladamente rodando o template como um plugin qualquer
(apontando `command`/`args` para ele manualmente) e confirmando que ele completa um handshake válido
— sem depender de nenhuma outra parte desta feature.

**Acceptance Scenarios**:

1. **Given** o template de plugin neste repositório, **When** alguém copia o diretório inteiro para
   fora do repo, ajusta o `plugin_name` no manifesto e no `main.py`, e o registra manualmente como
   plugin instalado (US1), **Then** o Farol consegue completar handshake com ele e chega a `Ready`
   (mesmo sem nenhum widget real — o template pode declarar zero widgets).

### Edge Cases

- Um manifesto de plugin de terceiro declara `plugin_name` igual ao de um dos 4 plugins de
  referência (`git-local`, `uptime-kuma`, `openfortivpn-vpn`, `docker-containers`) → resolvido em
  Clarifications/FR: nome colidindo com um plugin de referência é rejeitado (o de referência sempre
  vence), com aviso claro — nunca dois plugins com o mesmo nome rodando ao mesmo tempo.
- Dois plugins de terceiro instalados declaram o mesmo `plugin_name` entre si (não com um de
  referência) → mesma disciplina: o core detecta a colisão, mantém só o primeiro encontrado (ordem
  determinística de varredura do diretório) e ignora o segundo com aviso claro.
- Download interrompido a meio caminho (rede cai durante `farol install`) → o comando MUST falhar
  claramente e MUST NOT deixar uma instalação parcial visível para o core na próxima abertura (usar
  um diretório temporário e só "publicar" via rename atômico depois de tudo validado).
- Manifesto declara uma capability desconhecida (nem `exec` nem `network`) → mesma disciplina já
  usada pelo protocolo JSON-RPC para campos desconhecidos: o core ignora a capability desconhecida
  (não trava), mas também não concede acesso nenhum implícito por causa dela.
- Repositório GitHub privado ou inexistente → falha clara ("repositório não encontrado ou
  inacessível"), sem tentar autenticação nenhuma (instalação nesta fase cobre só repositórios
  públicos — ver Out of Scope).
- Reinstalar por cima de um plugin que está com o Farol aberto no momento → fora de escopo lidar com
  hot-reload; o efeito só aparece na próxima vez que o Farol iniciar (mesma disciplina de "reiniciar
  para aplicar", sem pretensão de recarregar plugin em processo já rodando).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: o core MUST descobrir plugins instalados escaneando um diretório de dados do usuário
  (um manifesto por plugin) a cada início, além dos 4 plugins de referência já hardcoded — sem
  precisar recompilar para reconhecer um plugin novo.
- **FR-002**: o manifesto de um plugin instalado MUST declarar, no mínimo: nome do plugin, comando e
  argumentos para spawná-lo, e as capabilities concedidas (mesmo vocabulário de `network`/`exec` já
  usado pelo `CapabilityManifest` do protocolo).
- **FR-003**: a fonte de verdade do perfil de sandbox (feature 006) de um plugin instalado via
  registry MUST ser o manifesto lido do disco no momento da descoberta — nunca o `CapabilityManifest`
  que o próprio processo do plugin declara no handshake em runtime (mesma disciplina de D1 da feature
  006, generalizada para plugins não-hardcoded).
- **FR-004**: um manifesto malformado (sintaxe inválida, campo obrigatório ausente) MUST fazer o core
  ignorar só aquele plugin específico, com aviso claro — MUST NOT impedir os demais plugins
  (referência ou outros instalados) de funcionar normalmente.
- **FR-005**: um plugin instalado cujo `plugin_name` colida com um dos 4 plugins de referência MUST
  ser ignorado (o de referência sempre vence) — nunca dois plugins com o mesmo nome ativos ao mesmo
  tempo.
- **FR-006**: o core MUST oferecer um comando de instalação (`farol install <owner>/<repo>`) que
  baixa a release/tag mais recente do repositório GitHub indicado, valida que contém um manifesto de
  plugin válido, e só então disponibiliza esse plugin para descoberta (FR-001) na próxima abertura do
  Farol.
- **FR-007**: a instalação MUST ser atômica do ponto de vista de quem descobre plugins depois (FR-001)
  — uma instalação que falhar no meio (download incompleto, manifesto inválido) MUST NOT deixar
  nenhum estado parcial visível/descoberto pelo core.
- **FR-008**: reinstalar um plugin já presente (mesmo `owner/repo`) MUST substituir a instalação
  anterior de forma limpa, sem misturar arquivos de versões diferentes.
- **FR-009**: os 4 plugins de referência existentes MUST continuar funcionando exatamente como hoje,
  sem depender de manifesto nem de nenhum mecanismo desta feature — regressão zero (SC de todas as
  features 001-006).
- **FR-010**: o comando de instalação MUST reportar falha clara e distinta para cada uma das causas
  identificadas nos Edge Cases (sem release, manifesto ausente/inválido, repositório inacessível) —
  nunca uma mensagem genérica de erro sem contexto.

### Key Entities

- **Manifesto de Plugin** (`farol-plugin.toml`, novo formato desta feature): descreve um plugin
  instalável — nome, comando/argumentos de spawn, capabilities concedidas. Vive na raiz do
  código-fonte do plugin (tanto no repositório de origem quanto, depois de instalado, no diretório
  de dados local).
- **Diretório de Dados de Plugins Instalados**: local no disco do usuário onde cada plugin instalado
  vive em sua própria pasta (código-fonte + manifesto), escaneado pelo core a cada início.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: um plugin de terceiro com manifesto válido, colocado no diretório de dados esperado,
  é descoberto e chega a `Ready` pelo Farol sem nenhuma mudança de código do core.
- **SC-002**: os 4 plugins de referência existentes continuam chegando a `Ready` e passando por
  todos os cenários de smoke já cobertos, sem nenhuma regressão de comportamento observável.
- **SC-003**: `farol install <owner>/<repo>` contra um repositório GitHub público real, com uma
  release válida, deixa o plugin pronto para descoberta na próxima abertura — verificável sem
  intervenção manual além do próprio comando.
- **SC-004**: um manifesto malformado nunca impede o Farol de abrir nem afeta os demais plugins —
  verificável automaticamente.
- **SC-005**: uma instalação que falhar (rede, manifesto inválido, sem release) nunca deixa o
  diretório de dados num estado que o core interprete como um plugin válido.

## Out of Scope

- Criar e publicar de verdade um repositório-índice GitHub central (nome, organização, manutenção) —
  decisão de infraestrutura externa que exige o usuário, não modo silencioso (ver Clarifications).
- CI de validação de pull requests nesse índice — não existe índice ainda nesta fase.
- Instalação inteiramente dentro da UI gráfica do iced (campo de busca, barra de progresso visual) —
  MVP é uma subcommand de CLI no mesmo binário; a UI gráfica fica como débito técnico rastreável.
- Instalação de plugin que exija um passo de build ou um asset binário próprio (não-interpretado) —
  MVP cobre só extração direta do código-fonte da release/tag.
- Autenticação para repositórios privados — instalação cobre só repositórios GitHub públicos.
- Aprovação de capabilities pelo usuário antes de ativar um plugin instalado (UI de revisão de
  permissões) — mesma exclusão já registrada na feature 006, fase de "polimento social" (roadmap
  item 5).
- Hot-reload de um plugin recém-instalado/atualizado enquanto o Farol já está aberto — efeito só
  aparece na próxima abertura.
- Capability de filesystem genérica declarável por um plugin de terceiro (equivalente aos casos
  especiais nomeados `scan_root`/socket Docker da feature 006) — um plugin instalado via registry
  nesta fase só pode receber `network`/`exec`, nada de bind extra de filesystem.
- Versionamento/pinning de versão específica na instalação (`farol install owner/repo@v1.2.3`) — MVP
  sempre instala a release/tag mais recente disponível.

## Assumptions

- O usuário que roda `farol install` tem acesso de rede de saída para `github.com`/subdomínios
  (mesma disciplina de "sem infra própria" do Princípio VII — usa a API pública do GitHub, sem
  token de autenticação nesta fase, sujeito a rate limit não-autenticado da API do GitHub).
- O diretório de dados de plugins instalados segue a convenção XDG (`$XDG_DATA_HOME/farol/plugins/`,
  fallback `~/.local/share/farol/plugins/`) — distinto do diretório de configuração já existente
  (`~/.config/farol/`, usado por `config_store`/`secrets_store` desde a feature 002), porque guarda
  código/dados de aplicação instalados, não configuração do usuário.
- O sandbox (feature 006) já aplicado aos 4 plugins de referência se generaliza sem mudança de
  design para qualquer plugin descoberto por esta feature — só a fonte do `SandboxProfile` muda (do
  registro estático `known_plugins()` para o manifesto lido do disco), a composição dos argumentos
  de `bwrap` (`sandbox::build_bwrap_args`) permanece a mesma.
- Ferramentas de sistema já assumidas em features anteriores (`curl`/`tar`, tipicamente presentes em
  qualquer distro Linux) estão disponíveis para o comando de instalação baixar/extrair o tarball da
  release — sem adicionar uma dependência Rust nova (cliente HTTP) ao `farol-core`, mesma disciplina
  de minimalismo de dependências já seguida pelas features 001-006.
