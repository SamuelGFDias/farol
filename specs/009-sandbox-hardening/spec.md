# Feature Specification: Sandbox — Mediação Real de Exec via Seccomp e Allowlist de Rede por Host

**Feature Branch**: `009-sandbox-hardening`

**Created**: 2026-09-07

**Status**: Draft

**Input**: User description: "Hardening do sandbox de plugins (feature 006): fechar débito técnico das issues #12 (mediação de `exec` só por visibilidade de filesystem, não por seccomp real contra `execve`) e #13 (capability `network` tratada como liga/desliga, sem allowlist real por host)"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Bloqueio real de execução não autorizada por seccomp (Priority: P1)

Como usuário do Farol rodando um plugin de terceiro sem a capability `exec`, quero que o sandbox impeça de fato qualquer tentativa do plugin de executar um binário arbitrário — mesmo que ele escreva um payload executável num diretório gravável (como `/tmp`) e tente rodá-lo por caminho absoluto — para que a ausência de `exec` seja uma garantia real de segurança, não apenas uma restrição de visibilidade de arquivos.

**Why this priority**: É a issue de segurança mais concreta do débito técnico atual — existe hoje um caminho documentado (`specs/006-sandbox-permissoes-bubblewrap/research.md`, seção D3) para contornar `exec=false` escrevendo e executando um binário por caminho absoluto num `tmpfs` gravável. Sem corrigir isso, a capability `exec=false` não cumpre a garantia de segurança que promete.

**Independent Test**: Pode ser testado isoladamente rodando um plugin com `exec=false` que escreve um script executável em `/tmp` e tenta executá-lo por caminho absoluto — o teste de integração deve observar falha de execução (ex.: rejeição por `execve`), sem depender de nenhuma mudança relacionada a rede (User Story 2).

**Acceptance Scenarios**:

1. **Given** um plugin com `exec=false`, **When** o plugin escreve um binário/script em `/tmp` (ou outro diretório gravável do sandbox) e tenta executá-lo por caminho absoluto, **Then** a tentativa de execução falha por rejeição do kernel — não apenas por ausência do binário no `$PATH` ou por bind ausente.
2. **Given** um plugin com `exec=true`, **When** o plugin executa um binário presente em `/usr/bin`, `/bin` ou `/usr/local/bin`, **Then** a execução funciona normalmente, sem regressão do comportamento atual.
3. **Given** um plugin com `exec=true`, **When** o plugin tenta executar um binário fora dos diretórios liberados por bind, **Then** a execução continua falhando (comportamento já existente hoje) — o novo mecanismo de mediação não pode ser mais permissivo que o atual.

---

### User Story 2 - Allowlist de rede por host individual (Priority: P2)

Como usuário do Farol, quero que um plugin com a capability `network` só consiga se comunicar com os hosts que ele declarou explicitamente no manifesto — não com a rede inteira — para reduzir a superfície de exfiltração de dados por um plugin comprometido ou malicioso.

**Why this priority**: Reduz superfície de ataque real, mas depende de um mecanismo de enforcement mais complexo (namespace de rede + regra de firewall ou equivalente) e já existe hoje uma mitigação parcial (a rede pode ao menos ser desligada por completo) — prioridade menor que a User Story 1.

**Independent Test**: Pode ser testado isoladamente configurando um plugin com `network` declarando um único host e verificando que uma conexão a esse host funciona e uma conexão a qualquer outro host falha — sem depender de nenhuma mudança relacionada a `exec` (User Story 1).

**Acceptance Scenarios**:

1. **Given** um plugin que declara a capability `network` para um host específico, **When** o plugin tenta se conectar a esse host, **Then** a conexão é estabelecida normalmente.
2. **Given** o mesmo plugin, **When** ele tenta se conectar a qualquer host não declarado, **Then** a conexão falha (recusa ou timeout), sem expor ao plugin nenhuma rota de rede fora da allowlist.
3. **Given** um plugin sem nenhuma capability `network` declarada, **When** ele tenta qualquer conexão de rede, **Then** a conexão falha por completo — comportamento idêntico ao atual (nenhuma regressão).
4. **Given** um plugin com múltiplas capabilities `network` (múltiplos hosts declarados), **When** ele tenta se conectar a cada um dos hosts declarados, **Then** todas essas conexões funcionam.

### Edge Cases

- O que acontece se um host declarado na allowlist não resolver via DNS no momento em que o sandbox é montado (host temporariamente fora do ar)?
- Como o sistema se comporta se o IP de um host declarado mudar depois que o sandbox já foi montado (TTL de DNS expira durante a execução do plugin)?
- Como o sistema se comporta numa máquina cujo kernel não suporta o mecanismo de mediação de `exec` escolhido?
- Como o sistema se comporta numa máquina sem a ferramenta de sistema exigida pelo mecanismo de allowlist de rede escolhido (ex.: ausência de `nftables`/`iptables`)?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O sandbox MUST aplicar um filtro de kernel real que rejeite as chamadas de sistema `execve`/`execveat`/`fexecve` sempre que o profile do plugin não conceder `exec` — fechando o caminho de execução via `tmpfs` gravável hoje possível apesar de `allow_exec=false`.
- **FR-002**: Quando `allow_exec=true`, o comportamento atual de bind read-only de `/usr/bin`, `/bin` e `/usr/local/bin` MUST permanecer sem alteração — nenhuma regressão do comportamento hoje existente.
- **FR-003**: O sandbox MUST implementar o filtro de `exec` usando a crate `seccompiler` (geração de filtro BPF em Rust puro, sem depender de `libseccomp` do sistema), aplicado ao processo do plugin via `bwrap --seccomp FD`.
- **FR-004**: O sandbox MUST negar a construção do ambiente do plugin (fail closed) se o mecanismo de mediação de `exec` escolhido não puder ser aplicado no sistema atual, em vez de silenciosamente cair de volta para mediação apenas por filesystem.
- **FR-005**: Quando um plugin declarar a capability `network` com um ou mais hosts específicos, o sandbox MUST permitir tráfego de saída apenas para os hosts declarados.
- **FR-006**: O sandbox MUST negar toda conexão de rede de saída para hosts não declarados quando o plugin tiver ao menos uma declaração de capability `network` — allowlist restritiva, não mais liga/desliga total.
- **FR-007**: Quando o plugin não declarar nenhuma capability `network`, o sandbox MUST continuar negando toda rede por completo — comportamento idêntico ao atual.
- **FR-008**: O sandbox MUST implementar a allowlist de rede por host resolvendo os hosts declarados para IP no momento em que o sandbox é montado e aplicando regras de firewall (`nftables`, com fallback para `iptables` se `nftables` não estiver disponível) dentro do namespace de rede criado pelo `bwrap`, liberando egress apenas para esses IPs.
- **FR-009**: A resolução de host para IP (quando aplicável ao mecanismo escolhido em FR-008) MUST ocorrer no momento em que o sandbox do plugin é montado, antes do processo do plugin iniciar.
- **FR-010**: As mudanças desta feature MUST preservar 100% dos testes hoje existentes de sandbox (`sandbox_unit_tests` e `sandbox_integration_tests`) e da suíte completa do workspace, sem regressão.

### Key Entities

- **Filtro de Exec**: representa o conjunto de chamadas de sistema permitidas/bloqueadas aplicado ao processo de um plugin, condicionado ao `allow_exec` do seu profile de sandbox.
- **Allowlist de Rede**: representa o conjunto de hosts (e portas, quando declaradas) aos quais um plugin com `network` tem permissão de se conectar, derivado das capabilities `Network` declaradas no manifesto do plugin.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Um plugin com `exec=false` não consegue executar nenhum binário escrito por ele mesmo em diretório gravável do sandbox — verificável por teste de integração automatizado que hoje reproduz esse cenário e passa a falhar a execução do binário.
- **SC-002**: Um plugin com `network` e host declarado consegue se conectar exclusivamente aos hosts declarados; nenhuma tentativa de conexão a host não declarado tem sucesso — verificável por teste de integração automatizado.
- **SC-003**: 100% dos testes hoje existentes no workspace (incluindo os 15 testes de sandbox da feature 006) continuam passando após a mudança.
- **SC-004**: Toda nova dependência de sistema introduzida por esta feature (se houver) é detectada na inicialização do Farol e reportada ao usuário com mensagem de erro clara, em vez de falhar silenciosamente ou aplicar um fallback inseguro.

## Assumptions

- O `bubblewrap` instalado no sistema do usuário é assumido como suportando o mecanismo de mediação de `exec` escolhido (a ser confirmado na fase de plano) — nenhum teste de disponibilidade foi feito nesta especificação.
- O padrão de teste já usado em `sandbox.rs` (testes unitários validam só o `Vec<String>` de argumentos do `bwrap`; testes de integração rodam o `bwrap` de verdade) é reaproveitado para os novos testes desta feature.
- O termo "`allowed_hosts`" usado em specs/documentação anteriores é um apelido informal do campo `host: String` (singular) de cada entrada `KnownCapability::Network` do protocolo — múltiplos hosts são declarados como múltiplas entradas de capability, não como uma lista num único campo.
- A crate `seccompiler` (usada por projetos como Firecracker) é adotada como nova dependência Rust de `farol-core` — gera BPF em Rust puro, sem exigir `libseccomp-dev` instalado no sistema do usuário.
- `nftables` ou `iptables` são assumidos como já instalados no sistema do usuário (dependência de sistema nova desta feature); a ausência de ambos resulta em falha clara na montagem do sandbox (FR-004 aplicado por analogia à allowlist de rede), nunca em fallback silencioso para rede liga/desliga.

## Out of Scope

- Filtragem de conteúdo/payload de rede (deep packet inspection) — a allowlist opera por host/IP, não por conteúdo do tráfego.
- Sandboxing do próprio mecanismo de allowlist de rede, caso ele dependa de um processo auxiliar de longa duração.
- Suporte a granularidade adicional além de host/porta já modelada hoje em `KnownCapability::Network`.
- UI de aprovação/revisão de capabilities antes da instalação de um plugin — débito futuro já registrado no roadmap da feature 006 (item 5), fora do escopo desta feature.
