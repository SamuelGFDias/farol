# Quickstart: Validação do Plugin de Referência Uptime Kuma

**Feature**: `002-uptime-kuma-plugin` | **Data**: 2026-08-31

Guia para validar manualmente, ponta a ponta, as duas User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de
`specs/001-walking-skeleton-git-plugin/quickstart.md`.

**Nota (sessão de automação, 2026-09-01)**: os Cenários 1–5, 7 (T036–T038, T040–T042 de `tasks.md`)
foram convertidos em testes permanentes em `crates/farol-core/src/e2e_tests.rs` — rodam de verdade
(`iced_test::Emulator`, processo de plugin real, HTTP real via fixture local determinística), sem
precisar de execução manual (ver `AGENTS.md` § Testes e as notas de execução de cada task em
`tasks.md`). Não é mais necessário reproduzir estes cenários à mão para validar a feature; este
documento continua sendo a especificação de referência de cada cenário. O Cenário 6 (T039) tem um
teste equivalente escrito, mas `#[ignore]`d de propósito — expõe um gap real de
`plugins/uptime-kuma/metrics_parser.py` (zero monitores é indistinguível de resposta inválida), ver
a nota de execução de T039.

## Pré-requisitos

**Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: os pré-requisitos abaixo substituem
versões anteriores deste documento, que exigiam o CLI `op` do 1Password instalado/autenticado e um
item já existente num cofre 1Password. Essa dependência foi removida — configuração e credencial
agora são providas pela tela de setup do próprio Farol (`research.md` D8, `data-model.md` §3.2), sem
nenhuma ferramenta externa.

- Rust estável ≥ 1.75 e `cargo`, atualizado para falar `protocol_version = "0.2"` (`farol-core`,
  `farol-protocol` — ver `plan.md` § Project Structure).
- Python 3.11+ no `PATH` (plugin `plugins/uptime-kuma`, D7 de `research.md`).
- Uma instância Uptime Kuma acessível pela rede, com:
  - ao menos um monitor cadastrado, em estado `UP` (para exercitar `status: "up"` e
    `response_time_ms` não-nulo);
  - opcionalmente, um monitor em `PENDING`/`MAINTENANCE`/`DOWN`, para exercitar os demais valores do
    enum `status` (FR-012);
  - autenticação HTTP Basic habilitada em `/metrics` (API Key gerada, ou usuário/senha da conta se
    nenhuma API Key existir ainda — Clarifications do spec) — anote a URL base e a API Key à mão,
    serão digitadas na tela de setup do Farol, não em nenhum arquivo/CLI.
- **Nenhuma configuração prévia de arquivo é necessária** para o caminho feliz (Cenário 1) — a
  primeira execução do Farol com o plugin `uptime-kuma` mostra a tela de setup automaticamente
  (`PluginState = Unavailable{NotConfigured}`, `data-model.md` §3.2), onde `base_url`/API Key são
  digitados. Só é preciso editar `~/.config/farol/plugins/uptime-kuma/config.toml`/
  `~/.config/farol/secrets.toml` manualmente se for necessário reproduzir um cenário específico sem
  passar pela UI (ex.: Cenário 2 abaixo).

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

(o plugin `uptime-kuma` é Python puro — nenhum passo de build necessário além de `chmod +x` no
entrypoint, se aplicável, mesmo padrão de `git-local`).

## Cenário 1 — Ver o estado dos monitores ao abrir, passando pela tela de setup (User Story 1, P1)

```bash
rm -f ~/.config/farol/plugins/uptime-kuma/config.toml ~/.config/farol/secrets.toml   # primeira execução limpa
cargo run -p farol-core
```

**Esperado** (Acceptance Scenarios 1–4 de US1, SC-001/SC-002/SC-005; revisado nesta sessão —
`research.md` D8):
1. Uma única janela nativa abre. Como nenhum `config.toml`/`secrets.toml` existe ainda para
   `uptime-kuma`, o Farol mostra a **tela de setup** deste plugin (`PluginState =
   Unavailable{NotConfigured}`, `data-model.md` §3.2) em vez do widget — um campo de texto rotulado
   "URL base da instância Uptime Kuma" e outro, mascarado, rotulado "API Key de métricas do Uptime
   Kuma" (rótulos = `description` de cada `required_config`, `handshake-delta.md`).
2. Preencher os dois campos com os valores anotados nos Pré-requisitos e confirmar. O Farol persiste
   os valores (`config.toml` para a URL, `secrets.toml` com permissão `0600` para a API Key) e
   reconecta o plugin — a tela de setup dá lugar ao widget "Uptime Kuma" populado (`kind:
   "monitor-status-grid"`).
3. Todos os monitores da instância configurada aparecem, cada um com nome, status
   (`up`/`down`/`pending`/`maintenance`) e tempo de resposta quando aplicável ao status
   (`widget-protocol-delta.md`).
4. Sem tocar em nada, após ~30s (ou o valor de `suggested_refresh_interval_ms` declarado, sempre
   `30000` neste plugin), os dados se atualizam sozinhos — confirmar alterando o estado de um monitor
   no próprio Uptime Kuma (ex.: pausar/retomar) e observando o próximo ciclo refletir a mudança sem
   reiniciar o Farol.
5. No handshake, confirmar (via log/diagnóstico do core, se disponível) que o manifesto de
   capacidades deste plugin inclui `network` (com o `host` da instância configurada) — capacidade
   nunca antes exercitada por `git-local` (FR-005, FR-006). A credencial não aparece mais como
   capacidade (revisão desta sessão — declarada via `required_config`, não via `capabilities`).
6. Fechar e reabrir o Farol (`cargo run -p farol-core` de novo, sem apagar `config.toml`/
   `secrets.toml` desta vez): o widget populado aparece direto, sem passar pela tela de setup de
   novo — confirma que a configuração persiste entre execuções.

## Cenário 2 — `base_url`/API Key não configurados (Acceptance Scenario 4 de US1, SC-005) — sem passar pela UI

```bash
rm -f ~/.config/farol/plugins/uptime-kuma/config.toml ~/.config/farol/secrets.toml
cargo run -p farol-core
# NÃO preencher a tela de setup que aparece — só observar o estado inicial
```

**Esperado**: o Farol mostra a tela de setup (`PluginState = Unavailable{NotConfigured}`,
`data-model.md` §3.2) em vez do widget ou de uma lista vazia silenciosa — sem crash do plugin. Esse é
o caminho **primário** de "não configurado" nesta revisão (versões anteriores deste documento tinham
cenários separados injetando o erro via credencial 1Password inválida ou via `op` ausente do `PATH`
— nenhum dos dois mecanismos existe mais; um único cenário cobre "qualquer item de `required_config`
sem valor", igual do lado do core quanto do lado do plugin). Confirmar também, via log/diagnóstico do
core se disponível, que uma chamada direta de `widget/get` a este plugin (contornando a UI)
devolveria `error(-32005, not_configured)` — salvaguarda descrita em `error-model-delta.md`.

## Cenário 3 — Corrigir um valor errado depois de configurado (regressão da tela de setup)

```bash
# com o Cenário 1 já concluído (widget populado), editar manualmente para um valor inválido:
sed -i 's/^base_url.*/base_url = "https:\/\/host-que-nao-existe.invalid"/' \
  ~/.config/farol/plugins/uptime-kuma/config.toml
# reiniciar o Farol para o processo do plugin ler o novo valor (D8 — sem hot-reload):
cargo run -p farol-core
```

**Esperado**: o widget passa a sinalizar `metrics_unreachable` (`-32006`, Cenário 4 abaixo) — **não**
`not_configured`, já que a variável de ambiente existe e tem um valor, só não é um host alcançável.
Confirma a distinção entre "não configurado" (Cenário 2, campo vazio) e "configurado com um valor que
não funciona" (erro pontual de leitura, tratado pelo mecanismo de US2).

## Cenário 4 — Uptime Kuma inacessível ou resposta inválida (User Story 2, P2, SC-003)

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

## Cenário 5 — Resposta inválida (não é `/metrics` Prometheus reconhecível)

Simular (ex.: apontar temporariamente `base_url` para um servidor HTTP qualquer que não seja Uptime
Kuma, ou um endpoint que devolva HTML/JSON em vez de texto Prometheus) e observar o próximo ciclo de
refresh.

**Esperado**: `error(-32007, metrics_parse_error)` — mesmo tratamento de erro pontual do Cenário 4
(FR-016), sem crash do plugin.

## Cenário 6 — Instância acessível, sem nenhum monitor cadastrado

Apontar `base_url` para uma instância Uptime Kuma real, mas recém-instalada, sem nenhum monitor
configurado.

**Esperado**: `widget/get` responde com sucesso e `items: []` — estado válido, distinto de qualquer
um dos erros acima (Edge Case do spec, análogo a diretório sem repositórios git na feature 001).

## Cenário 7 — Plugin morre / trava (herdado sem modificação da feature 001, FR-018)

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

## Cenário 8 — `git-local` (feature 001), inalterado, contra este core (documenta a quebra deliberada, D1/§ Complexity Tracking do `plan.md`)

Com o `farol-core` desta feature (falando `protocol_version = "0.2"`) e o plugin `git-local` da
feature 001 **sem nenhuma alteração**, configurar ambos os plugins simultaneamente (ou rodar
`git-local` isoladamente contra este core) — **revisão desta sessão**: com a correção C2 do checklist
de auditoria (`main.rs`/`plugin_worker.rs` suportando múltiplas `PluginConnection` simultâneas), rodar
os dois plugins ao mesmo tempo é o cenário mais representativo, já que é isso que um usuário real com
`git-local` já configurado veria ao atualizar para este core.

**Esperado**: `git-local` é recusado de forma limpa —
`PluginState = Unavailable{VersionIncompatible}`, mensagem legível citando `"0.1"` (plugin) vs.
`"0.2"` (core) — **sem crash do core**, mas o widget de repositórios git deixa de aparecer; se
`uptime-kuma` estiver configurado simultaneamente (ver Cenário 1), o widget dele continua funcionando
normalmente — as duas conexões são independentes. Este cenário **confirma o comportamento esperado e
documentado** (`research.md` D1, `contracts/framing-and-versioning-delta.md`), não um bug a corrigir
nesta feature — a correção (migrar `git-local`) é o débito técnico #4 do tracker do projeto, já aberto
(`tasks.md` § Débito técnico), fora do escopo de implementação desta feature.

## Critério de "feature 002 provada"

Todos os 8 cenários acima passam ⟹ o novo perfil de capacidade de rede (D1), o novo mecanismo de
configuração/segredo gerido pelo core com tela de setup (D8), o novo `kind` de widget declarativo
(`monitor-status-grid`, D4), a decoupling de I/O de rede em `widget/get` (D6, FR-010), e a transição
limpa de versão de protocolo (D1, Cenário 8) estão validados com um consumidor real — objetivo desta
feature (`spec.md`, Input). O Cenário 8, especificamente, é o que
torna visível — não apenas documentado — o débito técnico já registrado como issue #4 no GitHub
(`plan.md` § Constitution Check / § Complexity Tracking, `tasks.md` § Débito técnico).
