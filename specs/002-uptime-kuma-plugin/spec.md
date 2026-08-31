# Feature Specification: Plugin de Referência Uptime Kuma — Leitura de Status via `/metrics`

**Feature Branch**: `002-uptime-kuma-plugin`

**Created**: 2026-08-31

**Status**: Draft

**Input**: User description: "Plugin de referência Uptime Kuma — leitura de status de monitores via endpoint /metrics (Prometheus), segunda fatia vertical do Farol após o walking skeleton, provando que o protocolo já implementado suporta um segundo plugin com um perfil de capacidade diferente do primeiro: a capacidade de rede por allowlist de host declarada no manifesto (Princípio IV), nunca exercitada pelo plugin git-local. O plugin roda como processo separado falando o mesmo protocolo JSON-RPC/NDJSON já existente; no handshake declara identidade, manifesto de capacidades incluindo rede com host/endpoint configurado, e o widget de status de monitores; lê periodicamente o endpoint /metrics de uma instância Uptime Kuma configurada pelo usuário (URL base configurável por arquivo, mesmo padrão do scan_root do git-local); parseia as métricas Prometheus relevantes e devolve dado declarativo com lista de monitores (nome, status, tempo de resposta quando aplicável); o core renderiza reaproveitando o vocabulário de widget existente; falha de rede é erro pontual da leitura, não derruba o plugin nem marca indisponibilidade por si só; isolamento de crash/trava do processo já é comportamento genérico do core, provado na feature 001, não reespecificado aqui; sem nenhuma ação (action/invoke) nesta feature, somente leitura. Fora de escopo: enforcement de allowlist de rede, autenticação/token de API do Uptime Kuma salvo se estritamente necessária, qualquer ação de escrita/gerenciamento de monitores, qualquer outro plugin, paleta de comandos, workspaces, registry, mudança de protocolo salvo lacuna genuína."

## Clarifications

### Session 2026-08-31

- Q: O `CapabilityManifest` normativo hoje (`protocol/SPEC.md` §6.3, `handshake.schema.json`) representa capacidades como lista simples de strings (ex.: `["exec"]`), sem campo para metadado por capacidade — como declarar, então, o host/endpoint de rede e a referência à credencial no keyring que esta feature precisa expor no manifesto? → A: O protocolo MUST evoluir de capacidades como lista de strings simples para uma lista de capacidades estruturadas, cada uma identificada por um `kind` (ex.: `exec`, `network`, `secret`) e podendo carregar campos adicionais específicos daquele `kind` (ex.: host/endpoint para `network`). Esta é uma decisão de rumo comportamental — o desenho exato do schema (nomes de campo, formato, estratégia de compatibilidade retroativa com o `exec` já existente) fica para a etapa de planejamento técnico (`/speckit-plan`) desta feature, registrado como uma decisão de design própria (D1–D8, no mesmo padrão da feature 001), e não é definido nesta especificação.
- Q: A rota `/metrics` do Uptime Kuma exige autenticação até para o caso feliz desta feature, ou é opcional? → A: Sim, exige na prática — não é opcional. O Uptime Kuma protege `/metrics` com HTTP Basic Auth: a credencial é uma API Key gerada no próprio Uptime Kuma (usada como senha, com usuário vazio/qualquer valor) ou, na ausência de qualquer API Key, o usuário/senha da conta do Uptime Kuma — assim que a primeira API Key é criada, a autenticação por usuário/senha é desativada permanentemente. Esta feature MUST tratar autenticação como requisito desde o caso feliz, e a credencial MUST vir do keyring do sistema (Princípio IV da constitution), nunca de um arquivo de configuração em texto plano gerenciado pelo plugin — diferente do padrão usado para a URL base (FR-007/FR-008, que continua vindo de arquivo). Fonte: wiki oficial do projeto (Prometheus-Integration.md) e PR #101 do repositório `louislam/uptime-kuma`.
- Q: Qual é o formato exato das métricas Prometheus relevantes expostas pelo Uptime Kuma em `/metrics` — nomes de métrica, labels de identificação do monitor e mapeamento de valores para os estados de status? → A: Confirmado contra a wiki oficial do projeto e o PR #101 (`louislam/uptime-kuma`). `monitor_status{monitor_name="...", monitor_type="...", ...}` é um gauge cujo valor mapeia os estados `1`=UP, `0`=DOWN, `2`=PENDING, `3`=MAINTENANCE; `monitor_response_time{monitor_name="...", ...}` é um gauge com o tempo de resposta em milissegundos. O plugin só precisa extrair essas duas métricas — outras métricas expostas pelo Uptime Kuma (`monitor_cert_days_remaining`, `monitor_uptime_ratio`) ficam fora do escopo desta feature. Os labels (incluindo `monitor_name`) passam por sanitização do próprio Uptime Kuma antes de chegar ao `/metrics` (caracteres não-alfanuméricos removidos, label nunca começa com número), então o nome do monitor visível no label PODE divergir do nome de exibição original configurado no Uptime Kuma — perda de informação que fica registrada em Assumptions, não é tratada como erro.

**Fontes da pesquisa**: wiki oficial do Uptime Kuma, `Prometheus-Integration.md` (https://github.com/louislam/uptime-kuma-wiki/blob/master/Prometheus-Integration.md); PR #101 do repositório `louislam/uptime-kuma` (implementação original do endpoint `/metrics` e sua autenticação).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ver o estado dos monitores Uptime Kuma ao abrir o Farol (Priority: P1)

Um desenvolvedor abre o Farol com o plugin Uptime Kuma configurado. O core inicia o plugin como processo filho, os dois negociam a versão do protocolo no handshake (mesmo mecanismo já provado pela feature 001), o plugin se identifica e declara, além do widget de status de monitores, seu manifesto de capacidades — desta vez incluindo uma capacidade de rede referente ao host/endpoint da instância Uptime Kuma configurada. O plugin consulta periodicamente o endpoint `/metrics` dessa instância, extrai os dados relevantes de cada monitor e devolve, como dado declarativo, uma lista de monitores (nome, status, tempo de resposta quando aplicável). O core renderiza essa lista e a mantém atualizada em ciclos periódicos, sem o usuário precisar pedir atualização manual.

**Why this priority**: É o comportamento fundacional desta feature — sem ele não há fatia vertical para provar que o protocolo já implementado suporta um plugin com perfil de capacidade diferente do `git-local` (rede, em vez de execução local de processo). Reaproveita diretamente o par handshake → manifesto → widget declarativo → renderização já provado na feature 001, mas exercitando pela primeira vez a declaração de capacidade de rede do Princípio IV.

**Independent Test**: Pode ser testado sozinho apontando o plugin, via seu arquivo de configuração, para uma instância Uptime Kuma acessível com pelo menos um monitor cadastrado, e verificando que a lista de monitores aparece corretamente na janela do Farol sem qualquer ação adicional do usuário — entrega valor "Ver" completo por si só.

**Acceptance Scenarios**:

1. **Given** uma instância Uptime Kuma acessível na URL configurada, com pelo menos um monitor cadastrado, **When** o usuário inicia o Farol, **Then** o widget exibe a lista de monitores, cada um com nome, status e tempo de resposta quando aplicável.
2. **Given** o widget de monitores já está exibido, **When** o ciclo de refresh periódico ocorre, **Then** os dados exibidos são atualizados automaticamente, sem o usuário reabrir o Farol ou disparar a atualização manualmente.
3. **Given** o core e o plugin declaram versões de protocolo compatíveis, **When** o handshake é concluído, **Then** o plugin é identificado pelo core, seu manifesto de capacidades — incluindo a capacidade de rede com o host/endpoint declarado — fica registrado e consultável, o widget de monitores fica disponível para renderização, e nenhuma ação (`actions`) é declarada por este plugin.
4. **Given** o arquivo de configuração do plugin não existe ou não define a URL base da instância Uptime Kuma, **When** o Farol inicia, **Then** o widget reporta um estado explícito de "não configurado" — distinguível de "0 monitores" — em vez de uma lista vazia silenciosa ou de um crash do plugin.

---

### User Story 2 - Farol permanece utilizável quando o Uptime Kuma está inacessível ou responde de forma inválida (Priority: P2)

Enquanto o Farol está em uso, a instância Uptime Kuma configurada fica temporariamente inacessível (rede indisponível, host errado, timeout) ou passa a devolver uma resposta que não é um `/metrics` Prometheus válido. O plugin trata isso como uma falha pontual daquela leitura específica — sem derrubar seu próprio processo e sem, por si só, fazer o core marcar o plugin inteiro como indisponível. O core mantém a última lista de monitores conhecida, sinaliza o erro de forma visível associada ao widget, e retoma a exibição normal assim que a instância volta a responder corretamente, no próximo ciclo de refresh, sem intervenção do usuário.

**Why this priority**: É o comportamento que efetivamente prova a capacidade de rede sob condição adversa — o `git-local` da feature 001 nunca precisou distinguir "erro pontual de leitura" de "indisponibilidade da conexão" para uma falha de rede, porque nunca fez uma chamada de rede. Depende da User Story 1 já estar funcionando (não há o que exibir sem um widget e uma leitura periódica funcionando no caminho feliz).

**Independent Test**: Pode ser testado apontando o plugin para uma URL inacessível (host errado ou porta fechada) e verificando que (a) o Farol continua respondendo normalmente, (b) o widget sinaliza o erro de leitura de forma distinguível de "0 monitores" e de um estado de "plugin indisponível", e (c) corrigir a URL/tornar o host acessível novamente faz o widget voltar a exibir os monitores reais no ciclo de refresh seguinte, sem reiniciar o Farol.

**Acceptance Scenarios**:

1. **Given** a URL configurada do Uptime Kuma está inacessível, **When** o ciclo de refresh tenta consultar `/metrics`, **Then** o plugin devolve um erro pontual daquela leitura, o core mantém os últimos dados de monitores conhecidos (ou o estado de erro, se nunca houve leitura bem-sucedida) e sinaliza o erro associado ao widget, sem tratar o plugin como indisponível por causa disso.
2. **Given** o endpoint `/metrics` responde, mas com um conteúdo que não é um `/metrics` Prometheus válido, **When** o plugin tenta parsear a resposta, **Then** o mesmo tratamento de erro pontual do cenário anterior se aplica.
3. **Given** o Uptime Kuma volta a responder corretamente após uma falha, **When** o próximo ciclo de refresh periódico ocorre, **Then** o widget volta a exibir os dados reais dos monitores, sem exigir reinício do Farol nem ação manual do usuário.

---

### Edge Cases

- O que acontece quando o arquivo de configuração do plugin não existe ou não define a URL base do Uptime Kuma? (Diferente do `scan_root` do `git-local`, que tem um default seguro `~/dev`, não existe um host remoto default razoável para o Uptime Kuma — o plugin MUST reportar um estado explícito de "não configurado", nunca presumir um host.)
- O que acontece quando a instância Uptime Kuma configurada está acessível, mas não tem nenhum monitor cadastrado? (Estado válido: lista de monitores vazia, análogo ao diretório sem repositórios git da feature 001 — não é um erro.)
- O que acontece quando o processo do plugin falha ao iniciar (ex.: binário do plugin ausente)? Mesmo comportamento genérico já especificado pela feature 001 para qualquer plugin: o core não pode cair, e o plugin é sinalizado como indisponível desde o início, sem widget renderizado para ele.
- O que acontece se o `/metrics` responder, mas com um valor de `monitor_status` fora do conjunto `{0, 1, 2, 3}` (UP/DOWN/PENDING/MAINTENANCE, ver FR-012)? Tratado como resposta não parseável como esperado, mesmo tratamento de erro pontual de FR-016.
- A instância Uptime Kuma exige autenticação para expor `/metrics` — não é opcional, nem no caso feliz (ver FR-019). A credencial (API Key ou usuário/senha) MUST vir do keyring do sistema, nunca de um arquivo de configuração em texto plano gerenciado pelo plugin, por força do Princípio IV da constitution.

## Requirements *(mandatory)*

### Functional Requirements

**Processo, protocolo e handshake**

- **FR-001**: O plugin Uptime Kuma MUST rodar como um processo separado do core, falando exclusivamente o mesmo protocolo JSON-RPC/NDJSON já especificado em `protocol/SPEC.md` e já provado pela feature 001 — nenhuma mudança de protocolo é assumida por esta feature, exceto onde uma lacuna genuína é sinalizada explicitamente abaixo.
- **FR-002**: No handshake, o plugin MUST se identificar (nome/identidade) e declarar a versão de protocolo que fala; o mecanismo de negociação e o tratamento de incompatibilidade de versão são comportamento genérico do core já especificado e provado pela feature 001 e não são reespecificados por esta feature.
- **FR-003**: No handshake, o plugin MUST declarar o widget de status de monitores que oferece, simetricamente ao padrão já estabelecido pela feature 001 para declaração de widgets.
- **FR-004**: Esta feature MUST NOT expor nenhuma ação (`action/invoke`) — o plugin Uptime Kuma, neste escopo, é somente leitura. O campo `actions` declarado pelo plugin MUST vir vazio (`[]`), tanto no handshake quanto em qualquer resposta de `widget/get` deste plugin.

**Manifesto de capacidades — capacidade de rede**

- **FR-005**: No handshake, o plugin MUST declarar em seu manifesto de capacidades uma capacidade de rede referente à instância Uptime Kuma que consulta, incluindo o host/endpoint configurado, e uma capacidade de segredo referente à credencial de autenticação usada contra `/metrics` (ver FR-019) — conforme o Princípio IV da constitution ("rede por allowlist de host declarada" e "segredos no keyring do sistema"). O host/endpoint e a referência à credencial declarados MUST vir da configuração/keyring do próprio plugin (ver FR-008, FR-019), nunca hardcoded. Para acomodar metadado por capacidade (host/endpoint, referência de credencial), o protocolo MUST evoluir de capacidades como lista de strings simples para uma lista de capacidades estruturadas, cada uma identificada por um `kind` (ex.: `exec`, `network`, `secret`) e podendo carregar campos adicionais específicos daquele `kind` — esta especificação define esse rumo comportamental; o desenho exato do schema (nomes de campo, formato, compatibilidade retroativa com a capacidade `exec` já existente) é decisão da etapa de planejamento técnico (`/speckit-plan`) desta feature, não desta especificação.
- **FR-006**: O core MUST registrar o manifesto de capacidades declarado por este plugin — incluindo a capacidade de rede — e torná-lo consultável/visível ao usuário, no mesmo padrão apenas declarativo (sem enforcement) já estabelecido pela feature 001 para a capacidade `exec` do `git-local`.

**Configuração**

- **FR-007**: O plugin MUST ler a URL base da instância Uptime Kuma a partir de um arquivo de configuração do próprio plugin — mesmo padrão de configuração por arquivo já estabelecido pelo `scan_root` do plugin `git-local` (feature 001). A URL MUST NOT ser hardcoded no código do plugin.
- **FR-008**: Quando o arquivo de configuração não existe ou não define a URL base, o plugin MUST reportar esse estado de forma explícita e distinguível de "0 monitores" — sem crash do processo e sem tentar adivinhar/presumir um host default (diferente do `scan_root`, que tem um default seguro `~/dev`, não existe um host remoto default razoável para uma instância Uptime Kuma).

**Leitura periódica do endpoint `/metrics`**

- **FR-009**: O plugin MUST consultar periodicamente o endpoint `/metrics` da instância Uptime Kuma configurada, seguindo o mesmo mecanismo de intervalo de refresh já especificado pela feature 001 (`suggested_refresh_interval_ms` sugerido pelo plugin no handshake, com default de 30 segundos do core na ausência dessa sugestão).
- **FR-010**: A resposta do plugin a uma requisição `widget/get` do core MUST NOT depender, de forma síncrona, da latência da chamada de rede ao Uptime Kuma além do orçamento de tempo já definido pelo protocolo para chamadas de controle locais (`RPC_TIMEOUT_CONTROL`, 5s por padrão) — o plugin MUST manter internamente a última leitura conhecida de `/metrics` (atualizada pelo seu próprio ciclo periódico de consulta de rede) e responder a `widget/get` a partir dela, em vez de disparar uma chamada de rede síncrona dentro do próprio atendimento de `widget/get`. Rationale: `protocol/SPEC.md` §7.1 assume que `handshake/hello` e `widget/get` são IPC local sem I/O de rede, e usa isso para justificar um orçamento curto (5s) como sinal primário de plugin travado; este plugin é o primeiro a precisar de uma chamada de rede para produzir os dados do seu widget, e sem esse desacoplamento uma rede lenta (mas não travada) geraria falso positivo de "plugin travado".

**Parsing e dados devolvidos (widget declarativo)**

- **FR-011**: O plugin MUST interpretar (parsear) o conteúdo do endpoint `/metrics`, no formato texto plano Prometheus exposto pelo Uptime Kuma, e extrair dele as métricas `monitor_status{monitor_name="...", monitor_type="...", ...}` (gauge) e `monitor_response_time{monitor_name="...", ...}` (gauge, tempo de resposta em milissegundos), obtendo por monitor ao menos: nome do monitor (label `monitor_name`), status, e tempo de resposta quando aplicável ao status daquele monitor. Outras métricas expostas pelo Uptime Kuma em `/metrics` (ex.: `monitor_cert_days_remaining`, `monitor_uptime_ratio`) estão fora do escopo desta feature — o plugin MUST NOT depender delas.
- **FR-012**: O status de cada monitor devolvido pelo plugin MUST refletir os estados que o `monitor_status` do Uptime Kuma expõe: valor `1` = UP, `0` = DOWN, `2` = PENDING, `3` = MAINTENANCE.
- **FR-013**: O plugin MUST devolver os dados do widget exclusivamente como dado declarativo (lista de monitores com seus atributos) — o plugin MUST NOT emitir markup, pixels ou instruções de desenho de baixo nível (Princípio III, mesma regra já provada pela feature 001).
- **FR-014**: O core MUST ser o único responsável por renderizar os dados declarativos de monitores recebidos do plugin, reaproveitando o vocabulário de `kind` de widget já existente (`status-grid`) quando ele for suficiente para representar uma lista de monitores com seus atributos de status — a introdução de um novo `kind` de widget, caso o vocabulário existente se mostre insuficiente, é uma decisão de design a ser tomada na fase de planejamento, não presumida por esta especificação.

**Erros e disponibilidade**

- **FR-015**: Falha de rede ao consultar o endpoint `/metrics` (endpoint inacessível, timeout, host incorreto) MUST ser tratada pelo plugin como um erro pontual daquela leitura específica — o plugin MUST continuar respondendo normalmente a requisições subsequentes, sem encerrar seu próprio processo, seguindo o mesmo padrão de "erro pontual vs. indisponibilidade" já estabelecido pela feature 001 (precedente mais próximo no vocabulário de erro existente: `-32004`/`scan_root_unreadable`, reaproveitável ou estendido com um novo `reason` de domínio Farol dentro da mesma faixa reservada de código, conforme `protocol/SPEC.md` §8.2/§10).
- **FR-016**: Uma resposta do endpoint `/metrics` que não seja parseável como Prometheus válido MUST receber o mesmo tratamento de erro pontual descrito em FR-015 — não é crash do plugin nem indisponibilidade da conexão.
- **FR-017**: Ao receber um erro pontual de leitura (FR-015/FR-016), o core MUST manter os últimos dados de monitores conhecidos e sinalizar o erro de forma associada ao widget, sem mudar o estado geral de disponibilidade do plugin — mesmo comportamento já especificado pelo protocolo para erros pontuais de `widget/get` (`protocol/SPEC.md` §5.2).
- **FR-018**: O isolamento de falha do processo do plugin (crash, trava, término inesperado, não resposta ao JSON-RPC) é responsabilidade genérica do core, já especificada e provada pela feature 001 para qualquer plugin — esta feature não reespecifica esse comportamento; o plugin Uptime Kuma herda essa proteção automaticamente por rodar sob o mesmo modelo de processo isolado (Princípio II).

**Autenticação (`/metrics`)**

- **FR-019**: A rota `/metrics` do Uptime Kuma requer autenticação HTTP Basic na prática, inclusive no caso feliz — não é opcional. O plugin MUST se autenticar contra `/metrics` via HTTP Basic Auth, usando como credencial uma API Key gerada no Uptime Kuma (senha, com usuário vazio ou qualquer valor) ou, na ausência de API Key, o usuário/senha da própria conta do Uptime Kuma. Essa credencial MUST vir do keyring do sistema — NUNCA de um arquivo de configuração em texto plano gerenciado pelo plugin, ao contrário do padrão usado para a URL base (FR-007/FR-008), que continua vindo de arquivo — por força do Princípio IV da constitution. Ausência de credencial configurada no keyring MUST receber o mesmo tratamento de estado explícito de "não configurado" já previsto em FR-008 para a URL base ausente, sem crash do plugin.

### Key Entities

- **Plugin (processo)**: instância do plugin Uptime Kuma em execução como processo filho do core. Atributos relevantes: identidade/nome declarado, versão de protocolo declarada, estado de conexão (inicializando / disponível / indisponível) — mesmo modelo já estabelecido pela feature 001.
- **Manifesto de Capacidades**: conjunto de capacidades estruturadas que o plugin declara precisar, incluindo, nesta feature, uma capacidade de rede referente à instância Uptime Kuma configurada (com host/endpoint) e uma capacidade de segredo referente à credencial de autenticação usada contra `/metrics` (FR-005, FR-019). Apenas declarativo — sem enforcement nesta feature. O desenho exato do schema de capacidade estruturada é decisão da etapa de planejamento técnico (`/speckit-plan`).
- **Widget (modelo declarativo)**: estrutura de dados que o plugin devolve ao core descrevendo a lista de monitores a exibir, sem qualquer instrução de desenho.
- **Monitor (Uptime Kuma)**: unidade reportada pelo plugin dentro do widget. Atributos: nome (do label `monitor_name`, possivelmente sanitizado em relação ao nome de exibição original — ver Assumptions), status (UP/DOWN/PENDING/MAINTENANCE, FR-012), tempo de resposta quando aplicável ao status.
- **Configuração do Plugin**: arquivo de configuração próprio do plugin contendo, no mínimo, a URL base da instância Uptime Kuma a consultar — mesmo padrão de configuração por arquivo do `scan_root` do `git-local`, sem default seguro (FR-008). A credencial de autenticação contra `/metrics` (FR-019) NÃO faz parte deste arquivo — vem do keyring do sistema.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% dos monitores existentes na instância Uptime Kuma configurada aparecem no widget do Farol com nome, status e tempo de resposta (quando aplicável), sem o usuário abrir um terminal ou qualquer outra janela.
- **SC-002**: O estado exibido no widget se atualiza automaticamente ao longo do tempo, sem exigir que o usuário reinicie o Farol ou peça atualização manual.
- **SC-003**: Em 100% das ocorrências de falha de rede ao consultar o Uptime Kuma (host inacessível, timeout, resposta não parseável) observadas em teste, a janela do Farol permanece aberta e responsiva, e o plugin não é marcado como indisponível apenas por causa dessa falha pontual.
- **SC-004**: Assim que a instância Uptime Kuma volta a responder corretamente após uma falha, o próximo ciclo de refresh reflete os dados reais dos monitores sem qualquer intervenção do usuário.
- **SC-005**: Quando a URL base não está configurada, 100% das vezes o usuário vê um estado explícito de "não configurado", nunca uma lista vazia indistinguível de "0 monitores cadastrados".

## Out of Scope

- Enforcement de allowlist de rede (sandbox real) — nesta feature apenas a declaração do manifesto é exercitada, igual ao tratamento dado à capacidade `exec` na feature 001; nenhuma restrição de rede é de fato aplicada pelo core.
- Qualquer ação de escrita ou gerenciamento de monitores (criar, pausar, editar) — esta feature é somente leitura de status (FR-004).
- Qualquer outro plugin (VPN/openfortivpn, Git local além do já provado, GitHub issues/PRs, Docker).
- Espaços/workspaces por contexto (Princípio V da constitution).
- Paleta de comandos Ctrl+K (Princípio VI da constitution).
- Registry federado de plugins no GitHub, instalação in-app e publicação via pull request (Princípio VII da constitution).
- Reespecificação do isolamento de falha do processo do plugin (crash/trava) — já coberto, como comportamento genérico do core, pela feature 001 (FR-018).
- Mudança de protocolo além do que é explicitamente sinalizado como lacuna genuína nesta especificação (FR-005) — qualquer extensão de fato do protocolo é uma decisão a ser tomada fora desta especificação, não assumida aqui.
- Empacotamento e distribuição do Farol, tray icon, notificações do sistema, execução em background do core — mesmos itens fora de escopo da feature 001.

## Assumptions

- O core Farol e o plugin Uptime Kuma rodam na mesma máquina Linux, o plugin como processo filho local do core — não há execução remota de plugin nesta feature (mesma suposição da feature 001).
- A instância Uptime Kuma consultada já está em execução e acessível pela rede a partir da máquina do usuário — o Farol não instala, configura nem gerencia o Uptime Kuma em si; ele apenas consome seu endpoint `/metrics`.
- O intervalo do ciclo de refresh periódico do widget segue o mesmo padrão já decidido na feature 001: default fixo de 30 segundos do core, respeitando `suggested_refresh_interval_ms` quando o plugin o declarar no handshake.
- Assim como no `git-local`, apenas uma instância de Uptime Kuma é configurada e consultada por vez nesta feature; múltiplas instâncias/múltiplos hosts simultâneos ficam fora de escopo.
- O vocabulário de `kind` de widget `status-grid`, já usado e renderizado pelo core desde a feature 001, é o candidato natural de reaproveitamento para a lista de monitores (Princípio III) — a confirmação final desse reaproveitamento, incluindo os atributos exatos de item que ele precisa suportar, é uma decisão de design a ser validada na fase de planejamento, não um requisito bloqueante desta especificação.
- Um `/metrics` acessível, mas sem nenhum monitor cadastrado na instância Uptime Kuma, é um estado válido (lista de monitores vazia), não um erro — análogo ao diretório sem repositórios git da feature 001.
- Os labels do `/metrics` do Uptime Kuma (incluindo `monitor_name`) passam por sanitização do próprio Uptime Kuma antes de serem expostos: caracteres não-alfanuméricos são removidos e o label nunca começa com número. O nome do monitor visível no label PODE, portanto, divergir do nome de exibição original configurado no Uptime Kuma — perda de informação inerente ao formato de origem, não um defeito do plugin.
- Esta é a primeira feature do projeto que exercita a parte "segredos no keyring do sistema" do Princípio IV da constitution (FR-019) — o plugin `git-local` da feature 001 não precisou de nenhuma credencial.
