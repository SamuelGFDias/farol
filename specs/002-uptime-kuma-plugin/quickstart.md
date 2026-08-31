# Quickstart: Validação do Plugin de Referência Uptime Kuma

**Feature**: `002-uptime-kuma-plugin` | **Data**: 2026-08-31

Guia para validar manualmente, ponta a ponta, as duas User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de
`specs/001-walking-skeleton-git-plugin/quickstart.md`.

## Pré-requisitos

- Rust estável ≥ 1.75 e `cargo`, atualizado para falar `protocol_version = "0.2"` (`farol-core`,
  `farol-protocol` — ver `plan.md` § Project Structure).
- Python 3.11+ no `PATH` (plugin `plugins/uptime-kuma`, D7 de `research.md`).
- **CLI `op` do 1Password instalado e autenticado** (sessão ativa) no ambiente onde `farol-core` e o
  processo filho do plugin rodam (D8) — confirmar com:

  ```bash
  op whoami
  ```

  Se isso falhar (não instalado ou sessão expirada), o Cenário 4 abaixo (`exec_unavailable`) ou o
  Cenário 3 (`not_configured`) são os observados, não o caminho feliz.
- Um item já existente no cofre 1Password referenciado pelo plugin
  (`op://Dev/UptimeKuma/API Keys/farol`, ver `contracts/uptime-kuma-plugin.md`), contendo a API Key
  ou credencial de usuário/senha do Uptime Kuma:

  ```bash
  op item get "API Keys" --vault Dev  # confirma que o item existe e é legível pela sessão op ativa
  ```

- Uma instância Uptime Kuma acessível pela rede, com:
  - ao menos um monitor cadastrado, em estado `UP` (para exercitar `status: "up"` e
    `response_time_ms` não-nulo);
  - opcionalmente, um monitor em `PENDING`/`MAINTENANCE`/`DOWN`, para exercitar os demais valores do
    enum `status` (FR-012);
  - autenticação HTTP Basic habilitada em `/metrics` (API Key gerada, ou usuário/senha da conta se
    nenhuma API Key existir ainda — Clarifications do spec).
- Arquivo de configuração do plugin apontando para essa instância
  (`contracts/uptime-kuma-plugin.md` § Configuração):

  ```bash
  mkdir -p ~/.config/farol/plugins/uptime-kuma
  cat > ~/.config/farol/plugins/uptime-kuma/config.toml <<'EOF'
  base_url = "https://monitor.example.com"
  EOF
  ```

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

(o plugin `uptime-kuma` é Python puro — nenhum passo de build necessário além de `chmod +x` no
entrypoint, se aplicável, mesmo padrão de `git-local`).

## Cenário 1 — Ver o estado dos monitores ao abrir (User Story 1, P1)

```bash
cargo run -p farol-core
```

**Esperado** (Acceptance Scenarios 1–3 de US1, SC-001/SC-002):
1. Uma única janela nativa abre, com o widget "Uptime Kuma" populado (`kind: "monitor-status-grid"`,
   `handshake-delta.md`).
2. Todos os monitores da instância configurada aparecem, cada um com nome, status
   (`up`/`down`/`pending`/`maintenance`) e tempo de resposta quando aplicável ao status
   (`widget-protocol-delta.md`).
3. Sem tocar em nada, após ~30s (ou o valor de `suggested_refresh_interval_ms` declarado, sempre
   `30000` neste plugin), os dados se atualizam sozinhos — confirmar alterando o estado de um monitor
   no próprio Uptime Kuma (ex.: pausar/retomar) e observando o próximo ciclo refletir a mudança sem
   reiniciar o Farol.
4. No handshake, confirmar (via log/diagnóstico do core, se disponível) que o manifesto de
   capacidades deste plugin inclui `network` (com o `host` da instância configurada) e `secret` (com
   a referência `op://Dev/UptimeKuma/API Keys/farol`) — capacidades nunca antes exercitadas por
   `git-local` (FR-005, FR-006).

## Cenário 2 — `base_url` não configurada (Acceptance Scenario 4 de US1, SC-005)

```bash
rm -f ~/.config/farol/plugins/uptime-kuma/config.toml
cargo run -p farol-core
```

**Esperado**: o widget "Uptime Kuma" reporta um estado explícito de "não configurado" —
distinguível de "0 monitores" (`error(-32005, not_configured)`, `error-model-delta.md`) — sem crash
do plugin, sem lista vazia silenciosa. Restaurar o arquivo de configuração ao final do teste.

## Cenário 3 — Credencial não resolvível via 1Password (FR-019, mesmo tratamento de "não configurado")

```bash
# temporariamente, invalidar a resolução da credencial (ex.: renomear/mover o item no cofre,
# ou revogar a sessão `op` — `op signout`) e então:
cargo run -p farol-core
```

**Esperado**: mesmo estado observável do Cenário 2 (`error(-32005, not_configured)`) — FR-019 exige
explicitamente o mesmo tratamento de "não configurado" já usado para `base_url` ausente, mesmo que a
causa raiz seja diferente (aqui, credencial; lá, URL). Restaurar a sessão `op`/o item do cofre ao
final do teste.

## Cenário 4 — Binário `op` ausente do `PATH` (`exec_unavailable`, distinto de `not_configured`)

```bash
# rodar farol-core com um PATH que não inclua `op`
PATH=/usr/bin:/bin cargo run -p farol-core   # ajustar conforme onde `op` normalmente vive
```

**Esperado**: `error(-32003, exec_unavailable)` em vez de `-32005 not_configured` — sinal de que o
ambiente está quebrado (ferramenta ausente), distinto de "ambiente correto, mas sem credencial
provisionada" (`contracts/uptime-kuma-plugin.md` § Credencial). O plugin não crasha; o processo
continua respondendo.

## Cenário 5 — Uptime Kuma inacessível ou resposta inválida (User Story 2, P2, SC-003)

1. Iniciar o Farol com configuração válida (Cenário 1) e o widget já populado.
2. Tornar a instância inacessível (desconectar a rede, apontar `base_url` para um host/porta
   fechada, ou parar o serviço Uptime Kuma) sem reiniciar o Farol.

**Esperado** (Acceptance Scenarios 1–2 de US2):
1. A janela do Farol continua aberta e responsiva.
2. No próximo ciclo de refresh, o widget passa a sinalizar erro (`metrics_unreachable`, `-32006`) de
   forma visível e distinguível de "0 monitores" e de "plugin indisponível" — o core mantém os
   últimos dados de monitores conhecidos (ou o estado de erro, se a instância nunca respondeu com
   sucesso desde que o plugin subiu).
3. Restaurar o acesso à instância: no próximo ciclo de refresh, os dados reais voltam a ser exibidos,
   sem intervenção manual do usuário nem reinício do Farol (Acceptance Scenario 3 de US2, SC-004).

## Cenário 6 — Resposta inválida (não é `/metrics` Prometheus reconhecível)

Simular (ex.: apontar temporariamente `base_url` para um servidor HTTP qualquer que não seja Uptime
Kuma, ou um endpoint que devolva HTML/JSON em vez de texto Prometheus) e observar o próximo ciclo de
refresh.

**Esperado**: `error(-32007, metrics_parse_error)` — mesmo tratamento de erro pontual do Cenário 5
(FR-016), sem crash do plugin.

## Cenário 7 — Instância acessível, sem nenhum monitor cadastrado

Apontar `base_url` para uma instância Uptime Kuma real, mas recém-instalada, sem nenhum monitor
configurado.

**Esperado**: `widget/get` responde com sucesso e `items: []` — estado válido, distinto de qualquer
um dos erros acima (Edge Case do spec, análogo a diretório sem repositórios git na feature 001).

## Cenário 8 — Plugin morre / trava (herdado sem modificação da feature 001, FR-018)

Mesmo procedimento de `specs/001-walking-skeleton-git-plugin/quickstart.md` Cenários 4/5, agora
aplicado ao processo `plugins/uptime-kuma`:

```bash
pgrep -f "plugins/uptime-kuma" | xargs kill -9        # crash
pgrep -f "plugins/uptime-kuma" | xargs kill -STOP      # trava; kill -CONT para limpar depois
```

**Esperado**: idêntico ao já provado pela feature 001 — a janela do Farol continua aberta e
responsiva, o plugin é sinalizado como indisponível (`Unavailable{Crashed}`/`Unavailable{Unresponsive}`,
D6 da feature 001) — nenhuma reespecificação necessária, este cenário só confirma que o mecanismo
genérico já provado se aplica também a este plugin.

## Cenário 9 — `git-local` (feature 001), inalterado, contra este core (documenta a quebra deliberada, D1/§ Complexity Tracking do `plan.md`)

Com o `farol-core` desta feature (falando `protocol_version = "0.2"`) e o plugin `git-local` da
feature 001 **sem nenhuma alteração**, configurar ambos os plugins simultaneamente (ou rodar
`git-local` isoladamente contra este core).

**Esperado**: `git-local` é recusado de forma limpa —
`PluginState = Unavailable{VersionIncompatible}`, mensagem legível citando `"0.1"` (plugin) vs.
`"0.2"` (core) — **sem crash do core**, mas o widget de repositórios git deixa de aparecer. Este
cenário **confirma o comportamento esperado e documentado** (`research.md` D1,
`contracts/framing-and-versioning-delta.md`), não um bug a corrigir nesta feature — a correção
(migrar `git-local`) é o débito técnico registrado em `plan.md` § Complexity Tracking, fora do
escopo desta feature.

## Critério de "feature 002 provada"

Todos os 9 cenários acima passam ⟹ o novo perfil de capacidade (rede + segredo, D1), o novo `kind`
de widget declarativo (`monitor-status-grid`, D4), a decoupling de I/O de rede em `widget/get`
(D6, FR-010), e a transição limpa de versão de protocolo (D1, Cenário 9) estão validados com um
consumidor real — objetivo desta feature (`spec.md`, Input). O Cenário 9, especificamente, é o que
torna visível — não apenas documentado — o débito técnico que precisa virar issue no GitHub antes de
esta feature ser considerada encerrada (`plan.md` § Constitution Check / § Complexity Tracking).
