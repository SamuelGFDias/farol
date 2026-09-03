# Feature Specification: Plugin de Status de VPN (openfortivpn-gui)

**Feature Branch**: `004-vpn-status-plugin`

**Created**: 2026-09-02

**Status**: Draft

**Input**: User description: "Feature 004: integração com openfortivpn-gui via sua nova interface CLI programática (issue #8 do openfortivpn-gui, fechada pelo commit 5748830, contrato em specs/001-add-cli-interface/contracts/cli-commands.md e status-schema.json daquele repo). Farol deve ganhar um plugin de referência que expõe o estado da VPN (conectado/desconectado/erro, perfil ativo) como um widget no core, seguindo o mesmo padrão de plugin JSON-RPC dos plugins existentes (git-local, uptime-kuma) — o plugin farol chama a CLI do openfortivpn-gui (não duplica lógica de conexão) e traduz a saída para o protocolo farol-protocol."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ver o estado da VPN sem trocar de janela (Priority: P1)

Como usuário do Farol que depende de uma VPN corporativa para trabalhar, quero ver se estou
conectado, desconectado ou com erro na VPN diretamente no painel do Farol, sem precisar abrir o
terminal ou a janela do `openfortivpn-gui` para descobrir.

**Why this priority**: É o verbo "Ver" da constitution do produto — resolve a dor central sem
exigir nenhuma ação nova do usuário. Sozinho já entrega valor completo (visibilidade), mesmo sem
nenhuma ação de conectar/desconectar pelo Farol.

**Independent Test**: Com o `openfortivpn-gui` instalado e uma conexão manual (fora do Farol)
ativa ou inativa, abrir o Farol e conferir que o widget reflete o estado real (conectado, com
perfil ativo, ou desconectado) sem nenhuma outra interação.

**Acceptance Scenarios**:

1. **Given** nenhuma conexão VPN ativa, **When** o usuário abre o Farol, **Then** o widget mostra
   estado "desconectado" e a lista de perfis disponíveis.
2. **Given** uma conexão VPN ativa por um perfil específico (iniciada pela GUI, pela CLI ou por
   uma sessão anterior do Farol), **When** o usuário abre o Farol, **Then** o widget mostra estado
   "conectado" e o nome do perfil ativo.
3. **Given** o widget já mostrando um estado, **When** a conexão muda de estado por fora do Farol
   (ex.: usuário desconecta pela GUI), **Then** o widget reflete o novo estado no próximo ciclo de
   atualização, sem exigir reiniciar o Farol.

---

### User Story 2 - Conectar e desconectar sem sair do Farol (Priority: P2)

Como usuário do Farol, quero iniciar e encerrar a conexão VPN a partir do próprio widget, sem abrir
a GUI nem o terminal, para tratar a VPN como qualquer outra ação do meu ambiente de trabalho.

**Why this priority**: É o verbo "Agir" da constitution. Depende do estado exposto pela User Story
1 para fazer sentido (o usuário precisa ver o estado antes de agir sobre ele), por isso vem em
segundo lugar — mas é independentemente testável e entrega valor próprio acima da User Story 1.

**Independent Test**: A partir do widget em estado "desconectado", disparar a ação de conectar a um
perfil e confirmar que o estado muda para "conectado" dentro da janela de tempo esperada; a partir
de "conectado", disparar desconectar e confirmar o retorno a "desconectado".

**Acceptance Scenarios**:

1. **Given** o widget em estado "desconectado" com ao menos um perfil disponível, **When** o
   usuário aciona conectar a um desses perfis, **Then** o widget passa a refletir "conectando" e,
   ao concluir com sucesso, "conectado" com o perfil correto.
2. **Given** o widget em estado "conectado", **When** o usuário aciona desconectar, **Then** o
   widget passa a refletir "desconectado".
3. **Given** uma tentativa de conexão que falha (perfil inexistente, timeout, permissão negada,
   já conectado, erro interno), **When** a falha ocorre, **Then** o widget mostra uma mensagem de
   erro legível sem travar nem exigir reiniciar o Farol, preservando o último estado conhecido.

---

### User Story 3 - Não perder o rastro de uma conexão VPN esquecida (Priority: P3)

Como usuário do Farol, quero que o widget deixe visível quando estou conectado à VPN há muito
tempo (ex.: horas), para eu lembrar de desconectar quando não preciso mais, evitando manter uma
sessão VPN aberta sem necessidade.

**Why this priority**: É o verbo "Lembrar" da constitution — reforça as duas primeiras User
Stories, mas é a de menor prioridade porque depende inteiramente delas já existirem e o ganho é
incremental (lembrete visual), não uma capacidade nova.

**Independent Test**: Com uma conexão VPN ativa há um tempo conhecido, abrir o Farol e confirmar
que o widget mostra há quanto tempo decorrido desde o início da sessão, de forma visível sem
precisar abrir detalhes adicionais.

**Acceptance Scenarios**:

1. **Given** uma conexão VPN ativa, **When** o usuário olha o widget, **Then** consegue ver há
   quanto tempo a sessão está ativa sem nenhuma ação adicional.

---

### Edge Cases

- O que acontece quando o `openfortivpn-gui` não está instalado ou não está no `PATH` da máquina?
  O widget deve indicar essa condição como um estado de erro claro, não como "desconectado".
- O que acontece quando nenhum perfil de VPN foi configurado ainda no `openfortivpn-gui` (lista de
  perfis vazia)? O widget deve indicar essa condição explicitamente, distinta de "desconectado com
  perfis disponíveis".
- O que acontece quando o usuário tenta conectar a um perfil que não existe (mais) na lista atual?
  O widget deve mostrar o erro correspondente sem derrubar a sessão do Farol.
- O que acontece quando a tentativa de conexão expira (timeout) antes de confirmar sucesso ou
  falha? O widget deve mostrar esse timeout como um erro distinto de uma falha imediata.
- O que acontece quando o usuário tenta conectar enquanto já existe uma conexão ativa, ou
  desconectar quando já está desconectado? O widget deve mostrar o erro correspondente sem
  interpretar como sucesso silencioso.
- O que acontece quando a permissão de sistema necessária para abrir o túnel VPN é negada? O widget
  deve mostrar isso como um erro específico de permissão, distinto de timeout ou perfil inexistente.
- O que acontece se o estado da VPN mudar por uma via completamente fora do Farol e do
  `openfortivpn-gui` (ex.: interface de rede derrubada externamente) entre dois ciclos de
  atualização? O widget reflete o que a consulta de estado reportar no próximo ciclo — não há
  garantia de detecção instantânea, mesma limitação já aceita para os demais widgets do Farol.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O Farol MUST expor um widget de referência que mostra o estado atual da conexão VPN
  gerenciada pelo `openfortivpn-gui`: desconectado, conectando, conectado ou erro.
- **FR-002**: Quando conectado, o widget MUST mostrar o nome do perfil ativo.
- **FR-003**: Quando desconectado, o widget MUST mostrar a lista de perfis disponíveis para conexão
  (ou indicar explicitamente que nenhum perfil está configurado, caso a lista esteja vazia).
- **FR-004**: O widget MUST atualizar seu estado periodicamente, seguindo o mesmo modelo de
  atualização (polling) já usado pelos demais widgets do Farol, refletindo mudanças de estado
  iniciadas por qualquer origem (GUI, CLI ou o próprio Farol).
- **FR-005**: Uma falha pontual ao consultar o estado da VPN (ex.: `openfortivpn-gui` ausente do
  `PATH`, erro interno reportado pela consulta) MUST preservar o último estado conhecido do widget
  e mostrar uma indicação de erro, em vez de apagar ou zerar o widget.
- **FR-006**: O Farol MUST permitir conectar e desconectar a VPN diretamente pelo widget (User
  Story 2 está no escopo desta feature), sem exigir abrir a GUI ou o terminal do
  `openfortivpn-gui`.
- **FR-007**: O sistema MUST traduzir cada código de erro de domínio retornado pela tentativa de
  conectar ou desconectar (perfil inexistente, já conectado, não conectado, timeout de conexão,
  permissão negada, erro interno) em uma mensagem legível para o usuário, sem expor o código bruto
  como única informação.
- **FR-008**: Quando existir mais de um perfil disponível, o widget MUST oferecer um seletor de
  perfil na própria interface do Farol, permitindo ao usuário escolher qual perfil conectar sem
  precisar abrir o `openfortivpn-gui` para trocar de perfil.
- **FR-009**: O widget MUST tratar a ausência do `openfortivpn-gui` no `PATH` da máquina como um
  estado de erro distinto de "desconectado", visível para o usuário sem ambiguidade.
- **FR-010**: O sistema NÃO MUST duplicar a lógica de conexão/desconexão VPN dentro do Farol — toda
  operação de estado ou de conexão passa pela interface já exposta pelo `openfortivpn-gui`.

### Key Entities

- **VpnConnectionStatus**: representa o estado atual da conexão VPN em um instante — se está
  desconectado, conectando, conectado ou em erro; qual perfil está ativo (quando aplicável); e há
  quanto tempo a sessão atual está ativa (quando conectado).
- **VpnProfile**: um perfil de conexão VPN nomeado, disponível para seleção quando desconectado.
- **VpnActionOutcome**: o resultado de uma tentativa de conectar ou desconectar — sucesso (com o
  novo estado resultante) ou um erro de domínio específico e legível.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: O usuário identifica corretamente o estado atual da conexão VPN (conectado,
  desconectado ou erro) olhando o painel do Farol, sem abrir nenhuma outra janela ou terminal.
- **SC-002**: Uma mudança real no estado da conexão VPN (por qualquer origem) aparece refletida no
  widget do Farol dentro do mesmo intervalo de atualização já usado pelos demais widgets do
  produto, sem exigir reiniciar o Farol.
- **SC-003**: 100% dos códigos de erro de domínio possíveis na consulta de estado da VPN resultam
  em uma mensagem visível e legível no widget, nunca em silêncio ou travamento do Farol.
- **SC-004**: A introdução deste widget não altera o comportamento observável dos widgets já
  existentes (git-local, uptime-kuma) — nenhuma regressão perceptível ao usuário.

## Assumptions

- O Farol roda na mesma máquina em que o `openfortivpn-gui` está instalado e configurado — não há
  controle remoto de VPN entre máquinas diferentes.
- O `openfortivpn-gui` trata autorização de sistema (ex.: privilégios elevados para abrir o túnel)
  de forma não interativa por conta própria; o widget do Farol nunca precisa coletar senha nem
  exibir prompt de autorização — uma falha de autorização chega como um erro de domínio já
  resolvido (não como um prompt pendente).
- Existe no máximo uma sessão VPN ativa por vez nessa máquina, coerente com o modelo já exposto
  pela interface do `openfortivpn-gui` usada por esta integração.
- A lista de perfis disponíveis é gerenciada inteiramente pelo `openfortivpn-gui` (criar, editar,
  remover perfil); esta feature apenas lê e opera sobre perfis já existentes, não os administra.
- Uma lista de perfis vazia é um estado legítimo (nenhum perfil configurado ainda), não um erro.
