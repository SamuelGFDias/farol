# Quickstart: Validação do Plugin de Containers Docker

**Feature**: `005-docker-containers-plugin` | **Data**: 2026-09-03

Guia para validar manualmente, ponta a ponta, as duas User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de `specs/004-vpn-status-plugin/quickstart.md`.

**Nota (sessão de automação, 2026-09-03)**: a feature está totalmente implementada (User Stories 1
e 2 completas — `cargo test --workspace`: 128 testes, 0 falhas [67 em `farol-core` (bin `farol`,
sob `xvfb-run`) + 61 em `farol-protocol`]; `python3 -m unittest` do plugin: 23 testes, 0 falhas).
Alguns dos 11 Cenários abaixo já têm equivalente automatizado permanente; outros dependem de um
Docker real para validação manual completa. Sendo honesto e específico, cenário a cenário:

- **Cenário 1** (ver todos os containers, inclusive parados, sem tela de setup): coberto por
  `crates/farol-core/src/e2e_tests.rs::docker_containers_reaches_ready_and_populates_the_
  container_grid` — `Emulator` real, processo Python real, fixture `tests/fixtures/fake-docker/`
  cenário `multi_state` com três containers (`running`/`exited`/estado desconhecido "borked" →
  `unknown`, FR-012) — e por `crates/farol-core/src/visual_snapshot_tests.rs::docker_widget_
  populated_screen_matches_snapshot`, que fixa a tela renderizada desse mesmo estado (nome, imagem,
  estado em PT-BR, botões habilitados/desabilitados por linha).
- **Cenário 2** (ordem estável `(name, id)` entre atualizações): parcialmente coberto — o mesmo
  teste do Cenário 1 confirma que a ordem de um único `widget/get` é `db, mystery, web`
  (alfabética, não a ordem de saída da fixture), mas nenhum teste automatizado dispara dois ciclos
  de `RefreshTick` sucessivos para confirmar que uma linha não "pula" de posição entre
  atualizações — esse reforço específico (e a variante "parar/iniciar fora do Farol não muda a
  posição") depende de validação manual.
- **Cenário 4** (iniciar pelo widget, passo de sucesso): coberto por `e2e_tests.rs::docker_
  container_start_action_succeeds_and_flips_the_row_to_running` — dispara `docker.container.start`
  via `Message::ActionInvokeRequested` (o mesmo shape que o clique real no botão monta),
  confirma a linha virando "rodando" com a matriz de `enabled` recalculada (iniciar desabilitado,
  parar/reiniciar habilitados) e que `sidecar` (outro container) permanece intocado (FR-009). **Não**
  cobre, porém: o indicador visual de "operação em curso" durante a espera (passo 1), a ação
  **parar** isoladamente no nível e2e (só há cobertura de unidade em `update.rs`, ex.
  `action_invoke_requested_for_docker_container_marks_action_in_flight_and_sends_invoke`), nem o
  segundo clique em "iniciar" após parar (passo 3) — essas partes dependem de validação manual.
- **Cenário 5** (reiniciar): **sem** equivalente e2e — só há cobertura de unidade indireta em
  `update.rs` (o mecanismo de `action_in_flight`/`ActionInvokeRequested` é genérico por
  `action_id`) e de CLI wrapper em `test_docker_cli.py::ActionSuccessTests::test_restart_success_
  rereads_and_recomputes_enabled_for_new_state`. O comportamento de tempo (acumular o período de
  graça do `stop`, `timeout_hint_ms: 45000`) e a confirmação de que o container foi de fato
  reiniciado dependem inteiramente de validação manual contra um Docker real.
- **Cenário 6** (botões indisponíveis conforme o estado): coberto pelo mesmo `docker_widget_
  populated_screen_matches_snapshot` do Cenário 1, que fixa quais controles aparecem habilitados
  para cada um dos três estados simulados.
- **Cenário 7** (container removido antes do clique, `no_such_container`): coberto por
  `e2e_tests.rs::docker_container_start_action_failure_shows_translated_error_without_dropping_
  other_containers` — `FAKE_DOCKER_ACTION_SCENARIO=no_such_container`, confirma a mensagem
  traduzida exata renderizada na linha ("Falha: O container não existe mais...") e que a lista
  inteira (o outro container, `sidecar`) continua visível e sem estado alterado (FR-009).
- **Cenários 8 e 9** (Docker ausente do `PATH`, daemon parado, permissão negada): cobertos apenas no
  nível da CLI wrapper por `plugins/docker-containers/test_docker_cli.py`
  (`ListContainersBinaryMissingTests`, `ListContainersFailureClassificationTests` — inclusive o
  teste mais sensível, `test_stderr_containing_both_permission_denied_and_connection_failure_is_
  permission_denied`, que valida a **ordem** de classificação exigida por `research.md` D5.1).
  **Não** há teste e2e que exercite esses caminhos através do app `farol-core` real (`PATH`
  reduzido + widget renderizado) — a fixture prova a tradução da CLI wrapper, não que uma
  instalação real de `docker` ainda emita aquelas strings de stderr (a mesma fragilidade
  reconhecida em `research.md` D5.1 para a feature 004). Estes dois cenários continuam dependendo
  de validação manual completa contra Docker real para fechar o ciclo.
- **Cenário 10** (máquina sem containers): coberto apenas no nível da CLI wrapper por
  `test_docker_cli.py::ListContainersSuccessTests::test_empty_list_is_success_not_error`. **Não**
  há teste e2e equivalente no app real (os três testes de `e2e_tests.rs` usam os cenários
  `multi_state`/`action_target` da fixture, nenhum deles vazio) — o estado vazio renderizado na
  tela (`"Nenhum container encontrado."`) depende de validação manual.
- **Cenário 3** (mudança de estado por via externa ao Farol) e **Cenário 11** (regressão dos
  widgets existentes): sem equivalente automatizado no nível deste plugin especificamente —
  Cenário 11 é coberto de forma indireta pelos testes e2e já existentes de `git-local` e
  `uptime-kuma` (que continuam passando sob protocolo `"0.4"`), mas nenhum teste dispara
  `docker stop` fora do processo do Farol para confirmar que um refresh capta a mudança sozinho.

Em resumo: os Cenários 1, 6 e 7 têm equivalente automatizado completo; os Cenários 2, 4, 5, 8, 9 e
10 têm cobertura parcial (CLI wrapper e/ou unidade, sem o caminho e2e inteiro); os Cenários 3 e 11
dependem de validação manual para a parte específica de mudança externa observada em tempo real.
Todos os cenários envolvendo múltiplos containers reais simultâneos, tempo de espera real do
`docker stop`/`docker restart`, ou o texto exato de stderr de uma instalação real de `docker`
continuam exigindo Docker de verdade — a fixture `tests/fixtures/fake-docker/` cobre o protocolo,
não a integração real (mesma ressalva já registrada abaixo, em "Automação equivalente").

**Diferença importante em relação às features 001-004**: as ações desta feature são **mutantes sobre
recursos reais do usuário** — parar ou reiniciar um container atinge um serviço de verdade da
máquina. Por isso:

- Os cenários que envolvem ações (4-7) devem ser feitos contra **containers descartáveis criados só
  para o teste** (a seção Pré-requisitos traz os comandos), nunca contra containers de trabalho.
- A suíte automatizada **nunca** toca em um daemon Docker real: toda cobertura roda contra a fixture
  determinística `tests/fixtures/fake-docker/` injetada à frente do `PATH` (`PathPrefixGuard`, mesmo
  padrão de `tests/fixtures/fake-openfortivpn-gui/` da feature 004). Um teste que dependesse do
  daemon real poderia parar um container do desenvolvedor.

Consequência prática: os cenários abaixo continuam valendo como validação **manual** contra um Docker
real, mesmo depois que a automação equivalente existir — a automação prova o contrato, os cenários
provam a integração com o Docker de verdade.

## Pré-requisitos

- Rust estável e `cargo`, com `farol-core`/`farol-protocol` já falando `protocol_version = "0.4"`
  (`plan.md` § Project Structure, `research.md` D2) — incluindo `git-local`, `uptime-kuma` e
  `openfortivpn-vpn` migrados para `"0.4"` (mecânico, sem mudança de wire da parte deles).
- Python 3 no `PATH` (plugin `plugins/docker-containers`).
- `docker` instalado e no `PATH`, com daemon no ar, e o usuário do processo Farol com acesso ao
  daemon (grupo `docker` ou daemon rootless). O Farol **nunca** pede senha nem escala privilégio
  (spec.md § Assumptions).
- **Nenhuma configuração prévia é necessária** — `docker-containers` não tem `required_config`
  (`research.md` D9); não passa por tela de setup, fica `Ready` assim que o handshake completa.

Containers descartáveis para os cenários (imagens minúsculas, sem efeito colateral):

```bash
docker run -d  --name farol-demo-a  alpine sleep 100000   # ficará running
docker run -d  --name farol-demo-b  alpine sleep 100000
docker stop      farol-demo-b                              # ficará exited
docker create    --name farol-demo-c alpine sleep 100000   # ficará created, nunca executado
```

Limpeza ao final de todos os cenários:

```bash
docker rm -f farol-demo-a farol-demo-b farol-demo-c
```

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

(o plugin `docker-containers` é Python puro — nenhum passo de build necessário).

## Cenário 1 — Ver todos os containers, inclusive os parados (User Story 1, P1)

```bash
cargo run -p farol-core
```

**Esperado** (Acceptance Scenarios 1-2 de US1, SC-001, FR-001/FR-002):

1. O widget "Containers Docker" aparece populado sem tela de setup nenhuma.
2. Os **três** containers de demonstração aparecem, incluindo `farol-demo-b` (`exited`) e
   `farol-demo-c` (`created`) — não só os em execução. Este é o ponto central do verbo Ver: um
   `docker ps` sem `--all` mostraria só um dos três.
3. Cada linha mostra nome, imagem (`alpine`) e estado legível (`data-model.md` §1.2).

## Cenário 2 — Ordem estável entre atualizações (User Story 1, FR-004)

Com o Farol aberto do Cenário 1, aguardar ao menos dois ciclos de refresh
(`suggested_refresh_interval_ms`, default 30000 ms) sem tocar em nada.

**Esperado** (Acceptance Scenario 4 de US1, FR-004): a ordem das linhas não muda entre atualizações —
ordenação por `(name, id)` decidida pelo plugin (`research.md` D10), **não** pela ordem de saída do
`docker ps`, que não é contratual. Nenhuma linha "pula" de posição.

Reforço opcional: parar e iniciar um container fora do Farol
(`docker restart farol-demo-a`) e confirmar que ele permanece na mesma posição da lista — a ordem
depende de nome/id, não de estado nem de horário de criação.

## Cenário 3 — Mudança de estado por via externa ao Farol (User Story 1, FR-005)

```bash
docker stop farol-demo-a     # fora do Farol, com o Farol aberto
```

**Esperado** (Acceptance Scenario 3 de US1, SC-002): dentro de um ciclo de refresh, a linha de
`farol-demo-a` passa a "parado" sozinha, sem nenhuma ação do usuário no Farol, e os controles daquela
linha se ajustam (iniciar habilitado, parar desabilitado — matriz de FR-008).

Restaurar antes de seguir: `docker start farol-demo-a`.

## Cenário 4 — Parar e iniciar um container pelo próprio widget (User Story 2, P2)

Pré-condição: `farol-demo-a` em execução.

**Esperado** (Acceptance Scenarios 1-2 de US2, SC-003):

1. Clicar em **parar** na linha de `farol-demo-a` (`docker.container.stop`, `contracts/
   docker-cli-mapping.md`): a linha indica operação em curso imediatamente
   (`ContainerViewModel.action_in_flight`, `research.md` D7) e a janela do Farol **não trava**
   durante a espera — que pode chegar ao período de graça de 10 s do `docker stop`.
2. Ao concluir, a linha mostra "parado" e os controles se invertem sem exigir refresh manual — o
   resultado da ação traz o `ContainerStatusItem` inteiro, com os três `enabled` já recalculados
   pelo plugin (`research.md` D11).
3. Clicar em **iniciar** na mesma linha: volta a "em execução" pelo mesmo caminho.

Confirmar por fora, num terminal: `docker ps --all --filter name=farol-demo-a` reflete o mesmo
estado que o widget mostra.

## Cenário 5 — Reiniciar um container (User Story 2, P2)

Pré-condição: `farol-demo-a` em execução.

**Esperado** (Acceptance Scenario 3 de US2, SC-003): clicar em **reiniciar** mantém a linha em operação em
curso durante o ciclo `stop`+`start` e termina em "em execução". Verificar por fora que o container
foi realmente reiniciado (o `Status` do `docker ps` volta a "Up X seconds").

Ponto de atenção do orçamento de tempo (`research.md` D6): esta é a ação mais longa
(`timeout_hint_ms: 45000`) justamente por acumular o período de graça do `stop`. Se a UI parecer
"travada", é sintoma — a operação deve ser assíncrona.

## Cenário 6 — Ações indisponíveis conforme o estado (User Story 2, FR-008)

Sem clicar em nada, inspecionar as três linhas do Cenário 1.

**Esperado** (Acceptance Scenario 4 de US2, FR-008): os controles refletem a matriz normativa de
FR-008 — em `farol-demo-c` (`created`) e `farol-demo-b` (`exited`), **parar** aparece indisponível;
em `farol-demo-a` (`running`), **iniciar** aparece indisponível. Nunca é oferecida uma ação sem
sentido para o estado, e a decisão vem do plugin, nunca do core (Princípio III da constituição,
`protocol/SPEC.md` §5.3).

## Cenário 7 — Container removido entre a exibição e o clique (US2 Acceptance Scenario 5, SC-004)

Com o Farol aberto mostrando os três containers:

```bash
docker rm -f farol-demo-c    # remove sem que o Farol saiba
```

Antes do próximo refresh, clicar em **iniciar** na linha de `farol-demo-c`.

**Esperado** (`contracts/docker-cli-mapping.md`, classificação de ação nº 1): mensagem legível em
PT-BR dizendo que o container não existe mais e que a lista está desatualizada
(`-32011`/`no_such_container`) — não um erro genérico, não um travamento, e o restante da lista
continua funcionando normalmente.

## Cenário 8 — Docker ausente do `PATH` (Edge Case, FR-010a, SC-004)

```bash
PATH=/usr/bin:/bin cargo run -p farol-core   # PATH sem o diretório do docker (ajuste se necessário)
```

**Esperado** (`research.md` D5): o widget mostra **"O Docker não foi encontrado nesta máquina."**
(`-32003`/`exec_unavailable`) — mensagem distinta de "daemon parado" e de "sem permissão". O
diagnóstico correto importa porque o remédio de cada caso é diferente (instalar / subir o serviço /
entrar no grupo).

## Cenário 9 — Daemon parado e permissão negada (Edge Cases, FR-010b/c, SC-004)

Daemon parado (requer privilégio no host; pule se não for possível parar o serviço com segurança):

```bash
sudo systemctl stop docker.socket docker.service
cargo run -p farol-core
sudo systemctl start docker.service
```

**Esperado**: mensagem de daemon fora do ar (`-32010`/`daemon_unreachable`), distinta da do Cenário 8.

Permissão negada — reprodutível sem parar nada, se o usuário estiver no grupo `docker`:

```bash
docker context create farol-test-nopriv --docker host=unix:///run/docker-inexistente.sock
```

Alternativa mais fiel: rodar o Farol como um usuário fora do grupo `docker`.

**Esperado** (`research.md` D5.1): a mensagem de permissão aparece como **permissão**, não como
"daemon parado". Esta é a distinção mais fácil de errar, porque a própria CLI reporta a falta de
permissão como uma falha de conexão — a ordem de classificação do `contracts/docker-cli-mapping.md`
existe exatamente para isso, e este cenário é o que a valida contra um Docker real.

## Cenário 10 — Máquina sem nenhum container (Edge Case, FR-011)

```bash
docker rm -f farol-demo-a farol-demo-b farol-demo-c   # deixando a máquina sem containers
cargo run -p farol-core
```

**Esperado** (Acceptance Scenario 2 de US1, FR-011): o widget mostra um estado vazio explícito ("nenhum container"), **não** um erro — lista
vazia é sucesso legítimo (`data-model.md` §2.3, campo `loaded`, que distingue "ainda não li" de "li e
está vazio").

## Cenário 11 — Regressão: widgets existentes continuam funcionando (SC-005)

```bash
cargo run -p farol-core
```

**Esperado**: `git-local`, `uptime-kuma` e `openfortivpn-vpn` chegam a `Ready` normalmente (agora
falando `"0.4"`, `research.md` D2) — nenhuma mudança de comportamento observável nesses três widgets.
É o cenário que fecha a decisão de migrar os três plugins **dentro desta feature**, em vez de deixar
um deles para trás como fez a feature 002 (débito #4).

## Automação equivalente (a preencher durante `/speckit-implement`)

Como nas features 001-004, os cenários acima devem ganhar equivalentes automatizados onde possível:

- `crates/farol-core/src/e2e_tests.rs` (Camada 1, `iced_test::Emulator`): o plugin real chegando a
  `Ready` e populando o widget a partir da fixture `tests/fixtures/fake-docker/` — cobrindo a lista
  multi-estado (Cenário 1), a lista vazia (Cenário 10) e ao menos um caminho de ação com releitura
  (Cenário 4).
- `crates/farol-core/src/visual_snapshot_tests.rs` (`insta`): a tela renderizada do novo widget com
  containers em estados diferentes, fixando quais controles aparecem habilitados (Cenário 6).
- `plugins/docker-containers/test_docker_cli.py`: a classificação de stderr das três condições de
  `widget/get` e das quatro de ação (Cenários 7-9), no nível da CLI wrapper — inclusive a
  **ordem** de classificação (permissão antes de daemon), que é a regra mais fácil de regredir.
- `tests/integration/harness.sh` (Camada 2): mais uma condição confirmando `docker-containers`
  chegando a `Ready` sob Xvfb.

Os cenários que **permanecem dependentes de validação manual** por natureza: 3 (mudança externa
observada num refresh real), 5 (reinício real, incluindo o comportamento de tempo), 9 (daemon parado
e permissão negada num daemon de verdade — a fixture prova a tradução, não que a CLI real ainda
emita aquelas strings; ver a fragilidade reconhecida em `research.md` D5.1) e 11 na parte visual.
