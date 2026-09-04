# Contrato: mapeamento CLI `docker` → protocolo Farol

Fonte normativa: a própria CLI `docker` instalada na máquina. Diferentemente da feature 004 (cuja
CLI de origem publica um contrato JSON versionado em outro repositório), aqui **não existe um
contrato publicado do formato de saída** — `docker ps --format '{{json .}}'` é um formato de
apresentação, estável na prática mas não versionado. Este documento é, portanto, o contrato que o
**Farol** fixa sobre aquela saída, e todo teste do plugin roda contra fixtures que o materializam.

Comportamento verificado empiricamente contra **Docker 29.6.2** (Linux, socket Unix) durante o
`/speckit-plan` desta feature. Onde a verificação importa para o desenho, está anotada.

## `widget/get` (widget `docker-containers`, kind `container-status-grid`)

### Sequência

1. Se `shutil.which("docker") is None` → resposta de erro `-32003`/`exec_unavailable`, **sem
   executar nada**.
2. Senão, executar, com timeout de **3 segundos** (FR-014, `research.md` D6):

   ```
   docker ps --all --no-trunc --format '{{json .}}'
   ```

   - `--all`: inclui containers não-executando (FR-001) — sem a flag, `docker ps` só lista
     `running`, o que quebraria o requisito central do verbo Ver.
   - `--no-trunc`: ID completo de 64 hex (FR-013). Verificado que **não** substitui a tag da imagem
     por digest: containers com imagem tagueada continuam reportando a tag; só imagens já sem tag
     aparecem como `sha256:...`.
   - `--format '{{json .}}'`: **uma linha JSON por container** (NDJSON), não um array JSON. Linhas
     vazias são ignoradas.

3. Classificação do resultado:

   | Resultado | Resposta |
   |---|---|
   | exit `0`, zero linhas | **Sucesso**, `items: []` (FR-011) — nunca erro |
   | exit `0`, N linhas JSON válidas | **Sucesso**, `items: [ContainerStatusItem; N]`, ordenados por `(name, id)` (D10) |
   | exit `0`, alguma linha não parseável como JSON | `-32010`, `condition: "cli_error"` |
   | timeout de 3 s estourado | `-32010`, `condition: "timeout"` (mata o subprocess) |
   | exit ≠ `0` | ver a tabela de classificação de stderr abaixo |

### Classificação de stderr quando `exit ≠ 0` (FR-010)

**A ordem dos testes é normativa** — `permission_denied` MUST ser testado antes de
`daemon_unreachable`, porque a mensagem de permissão negada também é, literalmente, uma falha de
conexão; inverter a ordem classificaria toda falta de permissão como "daemon parado", que é
exatamente a confusão que FR-010 existe para evitar (e a condição mais provável numa máquina
recém-configurada).

| # | Teste (case-insensitive, sobre stderr) | `condition` | Mensagem PT-BR |
|---|---|---|---|
| 1 | contém `permission denied` | `permission_denied` | "Sem permissão para falar com o daemon do Docker. Seu usuário precisa estar no grupo `docker`, ou usar um daemon rootless." |
| 2 | contém `failed to connect` **ou** `cannot connect` **ou** `is the docker daemon running` | `daemon_unreachable` | "O Docker está instalado, mas o daemon não está respondendo — verifique se o serviço está no ar." |
| 3 | (qualquer outra coisa) | `cli_error` | "Não foi possível consultar os containers: o Docker respondeu de forma inesperada." |

Mensagem para o caso `-32003`/`exec_unavailable` (FR-010a): **"O Docker não foi encontrado nesta
máquina."**

`data.detail.raw` carrega sempre o stderr bruto (truncado), inclusive nos casos classificados —
o diagnóstico real nunca se perde, independentemente de a classificação acertar.

**Saídas observadas em Docker 29.6.2** (para as fixtures de teste):

- permissão negada: `permission denied while trying to connect to the docker API at unix:///...`
- daemon fora do ar: `failed to connect to the docker API at unix:///...; check if the path is
  correct and if the daemon is running`

A formulação clássica de versões anteriores (`Cannot connect to the Docker daemon at ...; is the
docker daemon running?`) também casa a regra 2 — as três alternativas da regra existem justamente
para atravessar a reformulação que ocorreu entre versões maiores do cliente. Ver `research.md` D5.1
para a fragilidade reconhecida e o motivo de o `fallback` `cli_error` existir.

### Mapeamento de campos por linha JSON

| Campo da CLI | Campo do protocolo | Tratamento |
|---|---|---|
| `ID` | `id` | Direto (64 hex, garantido por `--no-trunc`) |
| `Names` | `name` | Primeiro nome quando a CLI reporta vários separados por vírgula. Docker sempre atribui ao menos um |
| `Image` | `image` | Direto. Pode ser tag (`ghcr.io/x/y:latest`) ou identificador de imagem (`sha256:...`) quando a imagem não tem tag — ambos são exibíveis, nunca vazio |
| `State` | `state` | Mapeamento 1:1 da tabela de `data-model.md` §1.2; **qualquer** valor fora do vocabulário → `unknown` (FR-012), sem invalidar as demais linhas |
| `Status` | `status_text` | Direto, como texto auxiliar. **Nunca** parseado para derivar estado |
| demais campos | — | Descartados (`data-model.md` §3) |

Ordenação final por `(name, id)` ascendente (D10) — o plugin **não** confia na ordem de saída do
`docker ps`, que não é contratual.

### Cálculo de `enabled` (FR-008)

O plugin — nunca o core — calcula os três `enabled` a partir de `state`, conforme a matriz normativa
de FR-008 (repetida em `data-model.md` §1.3, invariante 5). Resumo: `unknown`, `removing` e `dead`
produzem os três `false`; `created`/`exited` habilitam start e restart; `running`/`restarting`/
`paused` habilitam stop e restart.

## `action/invoke` — as três ações de ciclo de vida

Request comum: `target: {type: "docker-container", id: <ID completo de 64 hex>}`.

| `action_id` | Comando | Timeout do subprocess | `timeout_hint_ms` declarado |
|---|---|---|---|
| `docker.container.start` | `docker start <id>` | 15 s | `20000` |
| `docker.container.stop` | `docker stop <id>` | 30 s | `35000` |
| `docker.container.restart` | `docker restart <id>` | 40 s | `45000` |

Os orçamentos de `stop`/`restart` acomodam o **período de graça de 10 s** do `docker stop`
(`SIGTERM`, depois `SIGKILL`). O plugin **não** passa `--time` para encurtá-lo: a política de
desligamento do container é do usuário, e o Farol reporta o Docker sem alterá-lo (FR-015,
`research.md` D6).

O `timeout_hint_ms` fica sempre **acima** do timeout interno do subprocess, para que quem reporte um
estouro seja o **plugin** (erro de domínio traduzido, `-32011`/`timeout`) e não o core sintetizando
`-32002`/`action_timeout` genérico — mesma disciplina de `vpn.connect` na feature 004.

### Sequência

1. Se `shutil.which("docker") is None` → `-32003`/`exec_unavailable`.
2. Executar o comando correspondente com o timeout da tabela.
3. Exit `0` → **releitura pontual** daquele container:
   `docker ps --all --no-trunc --filter id=<id> --format '{{json .}}'` (timeout de 3 s).
   - Uma linha → **sucesso**, `result: {"container": <ContainerStatusItem mapeado>}` (D11 — o item
     inteiro, não só o estado, porque `enabled` mudou).
   - Zero linhas → `-32011`, `docker_condition: "container_gone"` (o container foi removido por
     outra via entre a operação e a releitura).
   - Falha da releitura → `-32011`, `docker_condition` classificado como no passo 4.
4. Exit ≠ `0` → `-32011`, com `docker_condition` classificado sobre stderr, **nesta ordem**:

   | # | Teste (case-insensitive) | `docker_condition` | Mensagem PT-BR |
   |---|---|---|---|
   | 1 | contém `no such container` | `no_such_container` | "O container não existe mais — ele pode ter sido removido enquanto a lista estava aberta." |
   | 2 | contém `permission denied` | `permission_denied` | "Sem permissão para operar containers no daemon do Docker." |
   | 3 | contém `failed to connect` \| `cannot connect` \| `is the docker daemon running` | `daemon_unreachable` | "O daemon do Docker não está respondendo — a operação não foi executada." |
   | 4 | (qualquer outra) | `cli_error` | "O Docker recusou a operação." |

   Timeout do subprocess → `docker_condition: "timeout"`, mensagem "A operação não terminou dentro
   do tempo esperado. O container pode ainda estar em transição."

   `no such container` vem antes de tudo porque é a falha **esperada e frequente** do Edge Case
   "container removido entre a exibição da lista e o clique" — merece a mensagem mais específica, e
   é a única das quatro cuja explicação diz ao usuário que a lista dele está desatualizada, não que
   algo está quebrado.

**Saída observada em Docker 29.6.2** (para as fixtures): `docker start|stop|restart` de um container
inexistente devolve exit `1` e stderr `Error response from daemon: No such container: <nome>`
(o `start` acrescenta uma segunda linha `failed to start containers: <nome>`).

## Casos de borda cobertos (rastreabilidade com `spec.md` § Edge Cases)

| Edge case do `spec.md` | Tratamento |
|---|---|
| Ferramenta Docker ausente do `PATH` | `-32003`/`exec_unavailable`, mensagem própria (passo 1) |
| Ferramenta presente, daemon parado | `-32010`, `condition: "daemon_unreachable"` — mensagem distinta |
| Permissão negada ao usuário | `-32010`, `condition: "permission_denied"` — testado **antes** de daemon_unreachable |
| Daemon travado / demora demais | `-32010`, `condition: "timeout"` em 3 s (FR-014, SC-006) |
| Máquina sem nenhum container | **Sucesso** com `items: []` (FR-011), nunca erro |
| Dezenas de containers | Todos listados, ordenados por `(name, id)`; sem filtro nem paginação |
| Estado desconhecido reportado pelo Docker | Aquela linha vira `state: "unknown"` com os três `enabled: false`; as demais linhas seguem normais (FR-012) |
| Container removido entre a lista e o clique | `-32011`, `docker_condition: "no_such_container"` (ou `container_gone`, se a operação sucedeu e a releitura veio vazia) |
| Dois containers com a mesma imagem / renomeados entre atualizações | `target.id` é o ID completo de 64 hex, nunca o nome (FR-013) — a ação recai sobre exatamente o container selecionado |
| Duas ações no mesmo container / refresh no meio da operação | Resolvido no **core**, não no plugin: `ContainerViewModel.action_in_flight` preservado pelo merge por `id` (FR-017, `data-model.md` §2.4). O plugin não tem estado entre chamadas |
| Mudança de estado por via externa ao Farol | Refletida no próximo `widget/get` — mesma limitação já aceita para os demais widgets (sem push) |

## Fixtures determinísticas de teste

Mesmo padrão de `tests/fixtures/fake-openfortivpn-gui/` (feature 004): um binário `docker` de teste
injetado à frente do `PATH` (`PathPrefixGuard`, `crates/farol-core/src/e2e_tests.rs`), com
comportamento controlado por variável de ambiente, cobrindo:

- lista com containers em vários estados (inclusive um `unknown`),
- lista vazia,
- os três modos de falha de `docker ps` (permissão, daemon, saída inesperada),
- travamento (para exercitar o timeout de 3 s),
- as quatro condições de falha de ação, mais o caminho feliz `start`/`stop`/`restart` seguido de
  releitura.

Assim os testes não dependem de um daemon Docker real nem alteram containers da máquina de quem roda
a suíte — restrição que a feature 004 já adotou e que aqui é ainda mais importante, porque as ações
desta feature são **mutantes** sobre recursos reais do usuário.
