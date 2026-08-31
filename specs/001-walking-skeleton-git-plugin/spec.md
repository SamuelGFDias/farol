# Feature Specification: Walking Skeleton — Core, Protocolo de Plugin e Plugin de Referência Git Local

**Feature Branch**: `001-walking-skeleton-git-plugin`

**Created**: 2026-08-31

**Status**: Draft

**Input**: User description: "Walking skeleton — core + protocolo de plugin + plugin de referência Git local. Fatia vertical fim-a-fim que prova os contratos estruturais do Farol antes de qualquer feature de produto, para validar o contrato de plugin com um consumidor real. O core Farol (Rust + iced) inicia e abre uma janela única; inicia UM plugin como processo filho separado e se comunica com ele por JSON-RPC sobre stdin/stdout; core e plugin fazem handshake de inicialização negociando versão do protocolo, o plugin se identifica e declara quais widgets oferece, e incompatibilidade de versão falha de forma explícita e legível; o plugin devolve dados declarativos de widget (nunca desenha nem emite markup) e o core renderiza e atualiza periodicamente; o usuário dispara uma ação exposta pelo plugin a partir da UI do core (a ação de referência é `git fetch` no repositório selecionado), o core invoca via JSON-RPC, o plugin executa e devolve resultado/erro, e o core reflete o novo estado; se o processo do plugin travar ou morrer, o core não pode cair — deve sinalizar o plugin como indisponível na UI e seguir funcionando; o plugin declara um manifesto de capacidades (neste escopo apenas declarado/exibido, sem enforcement), exercitado pela capacidade `exec` que o plugin Git precisa para rodar o binário `git`. Plugin de referência: Git local — varre repositórios git sob um diretório configurado e reporta, por repo, caminho/nome, se há mudanças pendentes (working tree suja) e situação ahead/behind em relação ao remoto, escolhido por ter a menor superfície de permissão possível (sem rede autenticada, sem credencial, sem keyring). Fora de escopo: enforcement de permissões/sandbox, espaços/workspaces, paleta de comandos Ctrl+K, registry de plugins no GitHub, qualquer outro plugin, empacotamento/distribuição, tray icon/notificações/background."

## Clarifications

### Session 2026-08-31

- Q: O contrato de handshake do FR-006 cobre apenas a declaração de widgets pelo plugin, mas FR-015/FR-016 fazem a UI do core expor e invocar uma ação (`git fetch`), e a entidade "Ação (Fetch)" em Key Entities já fala em "ação declarada pelo plugin" sem nenhum requisito cobrindo essa declaração. Como o plugin deve declarar suas ações ao core? → A: O plugin MUST declarar suas ações no handshake, simetricamente aos widgets. Cada ação declarada carrega, no mínimo, um identificador estável, um rótulo legível para exibição e o alvo sobre o qual opera (ex.: um repositório específico). O core MUST NOT inferir nem hardcodar ações de plugin — só expõe na UI ações que o plugin declarou. Justificativa: pelo Princípio VI da constitution, a paleta de comandos (Ctrl+K) agrega toda ação de todo plugin ativo — isso só é possível se ações forem declaradas e enumeráveis no protocolo, como os widgets; congelar o protocolo com ação implícita ou hardcodada obrigaria a retrofitar a declaração quando a paleta chegar, quebrando todo plugin já escrito.
- Q: Qual o intervalo do ciclo de refresh periódico do widget — fixo e hardcoded nesta feature, ou configurável pelo usuário? → A: Default fixo de 30 segundos. O protocolo MUST permitir que o plugin sugira seu próprio intervalo de atualização no handshake — o core respeita a sugestão do plugin quando ela existir, caso contrário aplica o default de 30s. Tornar o intervalo configurável pelo usuário fica fora de escopo desta feature.
- Q: Qual é o diretório raiz varrido pelo plugin Git nesta feature — um valor default fixo, uma variável de ambiente, ou um arquivo de configuração lido pelo plugin? → A: Um arquivo de configuração do próprio plugin, com valor default `~/dev` quando o arquivo não existe ou não define o campo. Justificativa: já exercita, no walking skeleton, o padrão de configuração por plugin, em vez de cimentar um caminho pessoal no binário.
- Q: O que o plugin deve reportar para um repositório sem remoto configurado, e o que acontece com a ação de fetch nesse caso? → A: O plugin MUST reportar explicitamente o estado "sem remoto" para esse repositório (não omitir o campo silenciosamente), e o core MUST exibir esse estado de forma distinguível de "0 ahead / 0 behind". A ação de fetch vem declarada como desabilitada pelo próprio plugin (coerente com a declaração de ações acima — o core não decide isso sozinho, nem esconde a ação por conta própria; ele respeita o estado declarado). O core exibe a ação em estado desabilitado, não a omite.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ver o estado dos repositórios git ao abrir o Farol (Priority: P1)

Um desenvolvedor abre o Farol pela primeira vez. O core sobe uma janela única, inicia o plugin Git local como processo filho, os dois negociam a versão do protocolo no handshake, o plugin se identifica e declara o widget que oferece (status de repositórios) junto com seu manifesto de capacidades (`exec`). O plugin varre o diretório configurado, monta os dados declarativos do widget (lista de repositórios, cada um com working tree suja ou limpa, e situação ahead/behind) e o core renderiza esses dados na janela. O widget se atualiza sozinho em ciclos periódicos, sem o usuário precisar reabrir o app ou pedir refresh manualmente.

**Why this priority**: É o comportamento fundacional do walking skeleton — sem ele não existe fatia vertical para validar. Prova, de ponta a ponta, o par handshake → manifesto → widget declarativo → renderização pelo core, que é a base de qualquer outro plugin futuro (Princípios II, III e IV da constitution).

**Independent Test**: Pode ser testado sozinho abrindo o Farol com o plugin Git configurado apontando para um diretório com pelo menos um repositório git, e verificando que o status do(s) repositório(s) aparece corretamente na janela sem qualquer ação adicional do usuário — entrega valor "Ver" completo por si só, mesmo sem a ação de fetch da User Story 2.

**Acceptance Scenarios**:

1. **Given** o Farol não está em execução e existe pelo menos um repositório git sob o diretório configurado, **When** o usuário inicia o Farol, **Then** uma única janela abre e exibe um widget listando os repositórios encontrados, cada um com sua situação de working tree (suja/limpa) e ahead/behind em relação ao remoto.
2. **Given** o widget de status já está exibido, **When** o ciclo de refresh periódico ocorre, **Then** os dados exibidos são atualizados automaticamente, sem o usuário reabrir o Farol ou disparar a atualização manualmente.
3. **Given** o core e o plugin declaram versões de protocolo compatíveis, **When** o handshake de inicialização é concluído, **Then** o plugin é identificado pelo core, seu manifesto de capacidades (incluindo `exec`) fica registrado, e os widgets que ele declara oferecer ficam disponíveis para renderização.
4. **Given** o plugin declara uma versão de protocolo incompatível com a do core, **When** o handshake ocorre, **Then** o core não renderiza nenhum widget desse plugin, falha essa inicialização de forma explícita, e exibe uma mensagem legível ao usuário identificando a incompatibilidade — sem falhar silenciosamente e sem derrubar o restante do core.

---

### User Story 2 - Disparar `git fetch` a partir da UI do Farol (Priority: P2)

Com o widget de status de repositórios já visível (User Story 1), o desenvolvedor escolhe um repositório e aciona, pela própria UI do Farol, a ação de `git fetch` naquele repositório — sem abrir terminal. O core invoca a ação correspondente no plugin via JSON-RPC; o plugin executa `git fetch` no repositório selecionado e devolve o resultado (sucesso) ou um erro estruturado; o core reflete o novo estado (ahead/behind atualizado, ou uma indicação de erro) no mesmo widget.

**Why this priority**: Prova o round-trip inverso do protocolo — ação disparada pela UI do core, executada pelo plugin, com resultado refletido de volta — que é o segundo contrato estrutural essencial do Farol (o verbo "Agir"). Depende da User Story 1 já estar funcionando (não há onde disparar a ação sem o widget e o repositório visíveis).

**Independent Test**: Pode ser testado disparando a ação de fetch em um repositório já exibido pelo widget e verificando que (a) o `git fetch` de fato roda contra o repositório certo e (b) o estado ahead/behind exibido no Farol reflete o resultado — entrega valor "Agir" completo, verificável sem depender de nenhuma outra ação do plugin.

**Acceptance Scenarios**:

1. **Given** um repositório com remoto configurado está sendo exibido no widget, **When** o usuário aciona a ação de fetch para esse repositório pela UI do Farol, **Then** o core invoca a ação no plugin via JSON-RPC e, ao término, o estado ahead/behind exibido para aquele repositório reflete o resultado do fetch.
2. **Given** a ação de fetch foi disparada, **When** o `git fetch` falha no plugin (ex.: rede indisponível), **Then** o plugin devolve um erro estruturado ao core e o core exibe essa falha de forma legível associada ao repositório, sem travar a janela nem derrubar o core.

---

### User Story 3 - Farol continua funcionando quando o plugin trava ou morre (Priority: P3)

Enquanto o Farol está em uso, o processo do plugin Git para de responder ou termina inesperadamente (crash). O core detecta isso, marca o plugin (e o widget associado) como indisponível na interface, e o restante da aplicação continua respondendo normalmente — a janela não fecha, não trava, e não exibe um estado quebrado.

**Why this priority**: É a prova do isolamento de falha (Princípio II) — o requisito de menor prioridade de valor imediato para o usuário, mas o que garante que o modelo de plugins é seguro o suficiente para sustentar plugins de terceiros no futuro. Depende de haver um plugin rodando (User Story 1) para poder falhar.

**Independent Test**: Pode ser testado matando o processo do plugin (ou simulando trava) enquanto o Farol está aberto e verificando que a janela do core permanece responsiva, o widget do plugin passa a indicar estado "indisponível", e nenhuma outra funcionalidade do core é afetada.

**Acceptance Scenarios**:

1. **Given** o Farol está em execução com o plugin Git ativo e seu widget exibido, **When** o processo do plugin termina inesperadamente (crash), **Then** o core continua em execução, sem fechar nem travar a janela.
2. **Given** o plugin acabou de falhar, **When** o core detecta a falha, **Then** o widget/estado do plugin é sinalizado como indisponível na UI, de forma distinguível de um estado normal "sem dados ainda" ou "carregando".
3. **Given** o plugin está travado (processo vivo mas não responde ao JSON-RPC), **When** uma requisição ao plugin não recebe resposta, **Then** o core não fica bloqueado esperando indefinidamente pela resposta — segue funcionando e eventualmente sinaliza o plugin como indisponível.

---

### Edge Cases

- O que acontece quando o diretório configurado para o plugin Git não existe ou não contém nenhum repositório git? (o widget deve refletir um estado vazio válido, não um erro.)
- O que acontece quando o processo do plugin falha ao iniciar (ex.: binário do plugin ausente ou não executável)? O core não pode cair; o plugin deve ser sinalizado como indisponível desde o início, sem widget renderizado para ele.
- O que acontece quando um repositório sob o diretório configurado não tem remoto configurado, e o usuário tenta disparar `git fetch` nele? O plugin declara o estado "sem remoto" para esse repositório e declara a ação de fetch como desabilitada; o core exibe a ação em estado desabilitado (não a omite), então não há como o usuário disparar essa ação nesse repositório.
- O que acontece quando o binário `git` não está disponível no sistema para o plugin executar? O plugin deve reportar isso como erro/capacidade indisponível ao core, sem derrubar o processo do plugin.
- O que acontece se o usuário disparar a ação de fetch novamente enquanto uma anterior no mesmo repositório ainda está em andamento? Não há requisito de concorrência definido nesta feature; assume-se que a UI não precisa suportar múltiplas execuções simultâneas da mesma ação no mesmo repositório.

## Requirements *(mandatory)*

### Functional Requirements

**Core, janela e ciclo de vida do plugin**

- **FR-001**: O core Farol MUST abrir exatamente uma janela nativa ao iniciar.
- **FR-002**: O core MUST iniciar o plugin de referência Git como um processo filho, separado do processo do core.
- **FR-003**: O core e o plugin MUST se comunicar exclusivamente via JSON-RPC trocado sobre stdin/stdout do processo do plugin — nenhum outro canal de comunicação é usado nesta feature.

**Handshake e manifesto**

- **FR-004**: Ao iniciar, o core e o plugin MUST realizar um handshake de inicialização em que a versão do protocolo é declarada por ambos os lados.
- **FR-005**: Se a versão de protocolo declarada pelo plugin for incompatível com a versão suportada pelo core, o core MUST recusar a inicialização desse plugin de forma explícita, exibindo ao usuário uma mensagem legível que identifique a incompatibilidade — o core MUST NOT falhar silenciosamente nem travar.
- **FR-006**: Durante o handshake, o plugin MUST se identificar (nome/identidade) e declarar quais widgets ele oferece.
- **FR-006a**: Durante o handshake, o plugin MUST também declarar quais ações ele oferece, simetricamente à declaração de widgets. Cada ação declarada MUST carregar, no mínimo, um identificador estável, um rótulo legível para exibição e o alvo sobre o qual opera (ex.: um repositório específico); uma ação MUST poder ser declarada em estado habilitado ou desabilitado pelo próprio plugin.
- **FR-006b**: O core MUST NOT inferir nem hardcodar ações de plugin — o core só expõe na UI as ações que o plugin declarou no handshake, respeitando o estado (habilitada/desabilitada) declarado para cada uma.
- **FR-007**: O plugin MUST declarar um manifesto de capacidades durante a inicialização, incluindo a capacidade `exec` (necessária para rodar o binário `git`).
- **FR-008**: O core MUST registrar o manifesto de capacidades declarado pelo plugin e torná-lo consultável/visível para o usuário (exibição do manifesto declarado — sem qualquer enforcement de permissão nesta feature).

**Widgets declarativos**

- **FR-009**: O plugin MUST devolver o conteúdo do widget exclusivamente como dados declarativos (ex.: lista de repositórios com seus atributos) — o plugin MUST NOT emitir markup, pixels ou instruções de desenho de baixo nível.
- **FR-010**: O core MUST ser o único responsável por renderizar o modelo de dados declarativo recebido do plugin como elementos visuais na janela.
- **FR-011**: O core MUST atualizar periodicamente os dados exibidos do widget do plugin, sem exigir ação do usuário, em um intervalo com default fixo de 30 segundos; o protocolo MUST permitir que o plugin sugira seu próprio intervalo de atualização no handshake, e o core MUST respeitar essa sugestão quando presente, aplicando o default de 30s apenas na ausência dela.

**Plugin de referência Git — varredura e dados**

- **FR-012**: O plugin Git MUST varrer repositórios git localizados sob um diretório raiz lido de um arquivo de configuração do próprio plugin; quando esse arquivo não existe ou não define o campo, o plugin MUST usar `~/dev` como default. O caminho MUST NOT ser hardcoded no código do plugin.
- **FR-013**: Para cada repositório git encontrado, o plugin MUST reportar: caminho/nome do repositório, se a working tree tem mudanças pendentes (suja) ou não (limpa), e a situação ahead/behind em relação ao remoto configurado.
- **FR-014**: Para um repositório sem remoto configurado, o plugin MUST reportar explicitamente o estado "sem remoto" (não omitir o campo silenciosamente), e o core MUST exibir esse estado de forma distinguível de "0 ahead / 0 behind". Para esse repositório, o plugin MUST declarar a ação de fetch correspondente como desabilitada (conforme FR-006a); o core exibe essa ação em estado desabilitado, não a omite.

**Ação — `git fetch`**

- **FR-015**: A UI do core MUST expor, para cada repositório exibido, a ação de `git fetch` declarada pelo plugin para aquele repositório (conforme FR-006a), no estado (habilitado ou desabilitado) em que o plugin a declarou — o core não decide por conta própria quando essa ação fica disponível.
- **FR-016**: Ao ser disparada pelo usuário, a ação de fetch MUST ser invocada pelo core no plugin via JSON-RPC (round-trip: requisição da ação → execução no plugin → resposta ao core).
- **FR-017**: O plugin MUST executar `git fetch` para o repositório indicado e devolver ao core um resultado de sucesso ou um erro estruturado, sem derrubar o processo do plugin em caso de falha do `git fetch`.
- **FR-018**: Ao receber o resultado da ação, o core MUST atualizar o estado exibido do repositório (ahead/behind) em caso de sucesso, ou exibir o erro retornado em caso de falha.

**Isolamento de falha**

- **FR-019**: Se o processo do plugin travar, terminar inesperadamente ou parar de responder ao JSON-RPC, o core MUST detectar essa condição e continuar em execução — um crash do plugin MUST NOT derrubar o core.
- **FR-020**: Quando o plugin é detectado como indisponível (crash, término inesperado ou sem resposta), o core MUST sinalizar esse estado na UI de forma visível e distinguível de um estado normal de carregamento ou de "sem dados".
- **FR-021**: A falha do plugin MUST NOT impedir o restante da janela do core de continuar respondendo à interação do usuário.

### Key Entities

- **Plugin (processo)**: representa a instância do plugin Git local em execução como processo filho do core. Atributos relevantes: identidade/nome declarado, versão de protocolo declarada, estado de conexão (inicializando / disponível / indisponível).
- **Manifesto de Capacidades**: conjunto de capacidades que o plugin declara precisar (nesta feature, ao menos `exec`), registrado pelo core no momento do handshake. Apenas declarativo — sem relação de enforcement nesta feature.
- **Widget (modelo declarativo)**: estrutura de dados que o plugin devolve ao core descrevendo o que deve ser exibido (ex.: uma lista/status-grid de repositórios), sem qualquer instrução de desenho.
- **Repositório Git**: unidade reportada pelo plugin dentro do widget. Atributos: caminho/nome, estado da working tree (suja/limpa), situação ahead/behind em relação ao remoto, presença ou ausência de remoto configurado.
- **Ação (Fetch)**: ação declarada pelo plugin no handshake (simetricamente a um widget), com identificador estável, rótulo legível e alvo (o repositório sobre o qual opera), e estado habilitado ou desabilitado definido pelo próprio plugin (ex.: desabilitada para um repositório sem remoto). Quando habilitada e disparada pelo usuário via UI do core, corresponde à invocação de `git fetch` sobre o repositório-alvo, com resultado sucesso (novo estado ahead/behind) ou erro estruturado.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% dos repositórios git existentes sob o diretório configurado aparecem no widget do Farol com seu estado de working tree e ahead/behind, sem o usuário precisar abrir um terminal ou qualquer outra janela.
- **SC-002**: O estado exibido no widget se atualiza automaticamente ao longo do tempo, sem exigir que o usuário reinicie o Farol ou peça atualização manual.
- **SC-003**: O usuário consegue disparar um `git fetch` e ver o resultado (novo estado ahead/behind ou erro) refletido na mesma janela do Farol, sem executar nenhum comando fora do aplicativo.
- **SC-004**: Em 100% das ocorrências de crash ou trava do processo do plugin observadas em teste, a janela do Farol permanece aberta e responsiva (nenhum crash do core é causado pela falha do plugin).
- **SC-005**: Em 100% dos casos de incompatibilidade de versão de protocolo entre core e plugin, o usuário recebe uma mensagem legível identificando a incompatibilidade, em vez de uma tela em branco, travada ou sem explicação.

## Out of Scope

- Enforcement de permissões/sandbox do manifesto (bubblewrap, allowlist de rede, keyring de segredos) — nesta feature apenas o formato declarativo do manifesto é exercitado; nenhuma restrição é de fato aplicada pelo core.
- Espaços/workspaces por contexto (Princípio V da constitution).
- Paleta de comandos Ctrl+K (Princípio VI da constitution).
- Registry federado de plugins no GitHub, instalação in-app e publicação via pull request (Princípio VII da constitution).
- Qualquer outro plugin além do plugin de referência Git local (ex.: VPN/openfortivpn, Uptime Kuma, GitHub issues/PRs, Docker).
- Empacotamento e distribuição do Farol (Flatpak, AppImage, binário estático) — decisão adiada para outra feature.
- Tray icon, notificações do sistema, e execução em background do core.
- Reinício/recuperação automática de um plugin que travou ou morreu — esta feature exige apenas sinalizar o plugin como indisponível e manter o core funcionando; qualquer mecanismo de restart automático fica para uma feature futura.
- Autenticação de rede e gerenciamento de credenciais para operações git (o plugin de referência foi escolhido justamente por não precisar de rede autenticada nem de segredos).
- Tornar o intervalo do ciclo de refresh periódico configurável pelo usuário — nesta feature o intervalo é um default fixo de 30 segundos, com possibilidade de o plugin sugerir seu próprio intervalo no handshake (ver FR-011).

## Assumptions

- O core Farol e o plugin de referência Git rodam na mesma máquina Linux, como processo filho local do core — não há execução remota de plugin nesta feature.
- O binário `git` está instalado e disponível no `PATH` do sistema onde o Farol roda; a capacidade `exec` declarada no manifesto cobre a invocação desse binário.
- Repositórios varridos pelo plugin usam remotos já configurados e acessíveis com as credenciais/configuração de rede já existentes no ambiente do usuário (SSH agent, credential helper do próprio git, etc.) — o Farol não gerencia nem armazena nenhuma credencial nesta feature.
- Um diretório raiz configurado sem nenhum repositório git é um estado válido (lista vazia), não um erro.
- Apenas um plugin roda por vez nesta feature (o plugin Git de referência); múltiplos plugins simultâneos não fazem parte deste walking skeleton, embora o protocolo não deva assumir isso como regra permanente para o futuro.
- Não há requisito de suportar múltiplas janelas do core nesta feature — apenas a janela única mencionada no Princípio I.
