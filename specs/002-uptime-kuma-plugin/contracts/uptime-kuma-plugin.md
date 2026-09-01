# Contrato: Plugin de Referência `uptime-kuma`

**Pré-requisito**: todos os contratos-delta desta pasta. Este documento cobre o que é específico do
plugin de referência `uptime-kuma` (não genérico ao protocolo Farol) — FR-007 a FR-020, análogo a
`specs/001-walking-skeleton-git-plugin/contracts/git-local-plugin.md`.

## Identidade

**Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: `capabilities`/credencial abaixo
substituem o desenho anterior deste documento, que usava CLI `op`/1Password. Ver `research.md` D8
para a arquitetura completa (core gerencia armazenamento/injeção, sem dependência de sistema externa).

- `plugin_name`: `"uptime-kuma"`.
- `protocol_version`: `"0.2"` (D1 de `research.md`).
- `capabilities.capabilities`: `[]` quando `base_url` ainda não resolvido; `[{"kind": "network",
  "host": ..., "port": ...}]` quando resolvido (ver `handshake-delta.md`). **Não** declara mais
  `{"kind": "exec"}` (não invoca nenhum binário externo) nem `{"kind": "secret"}` (removido do
  vocabulário — a credencial é declarada via `required_config`, abaixo).
- `required_config`: `[{"name": "base_url", "secret": false, "description": "..."}, {"name":
  "api_key", "secret": true, "description": "..."}]`, sempre — independente de já haver valor
  armazenado (`research.md` D8, `data-model.md` §1.6.1).
- Linguagem de implementação: **Python 3.11+, apenas biblioteca padrão** (D7 de `research.md`,
  reafirma D3 da feature 001) — nenhuma dependência em `farol-protocol` nem em qualquer código Rust
  do core, nenhuma dependência via `pip`.
- **Sem dependência de sistema externa nesta revisão** — diferente de versões anteriores deste
  documento, que exigiam o binário `op` (1Password CLI) instalado e autenticado. O plugin só lê
  variáveis de ambiente já resolvidas pelo core (`research.md` D8); nenhum binário externo é invocado.

## Widget oferecido

- Um único widget: `id: "uptime-kuma-monitors"`, `kind: "monitor-status-grid"`,
  `title: "Uptime Kuma"`.
- `suggested_refresh_interval_ms`: **enviado, sempre `30000`** (diferente de `git-local`, que
  deliberadamente omitia este campo para exercitar o ramo "default do core" — esta feature exercita
  o outro ramo de FR-009, "sugestão do plugin", já coberto o ramo default pela feature 001; não há
  necessidade de repetir a cobertura do mesmo ramo numa segunda feature). Este valor **é o mesmo**
  usado internamente como cadência da thread de polling (D6) — fonte única, não dois conceitos de
  intervalo desacoplados.

## Configuração e credencial (FR-007, FR-008, FR-019) — revisado (`research.md` D8)

**Revisão desta sessão**: `base_url` e a credencial (`api_key`) deixam de ter mecanismos de leitura
diferentes entre si (arquivo TOML vs. CLI `op`) — ambas são declaradas pelo plugin via
`required_config` (`handshake-delta.md`) e lidas do mesmo jeito: variável de ambiente injetada pelo
core no spawn do processo. O plugin **não** lê nenhum arquivo de configuração, nem invoca nenhum
binário externo.

- `plugins/uptime-kuma/main.py` declara, no handler de `handshake/hello`, sempre:

  ```jsonc
  "required_config": [
    { "name": "base_url", "secret": false, "description": "URL base da instância Uptime Kuma" },
    { "name": "api_key", "secret": true, "description": "API Key de métricas do Uptime Kuma" }
  ]
  ```

- `config.py`/`secrets.py` (ver Project Structure em `plan.md` — as duas responsabilidades podem
  colapsar num único módulo, já que o mecanismo de leitura é idêntico para os dois campos) leem, uma
  única vez no arranque do processo:

  ```python
  import os

  def resolve(name: str) -> str | None:
      env_var = f"FAROL_PLUGIN_UPTIME_KUMA_{name.upper()}"  # research.md D8 — convenção fixa,
      return os.environ.get(env_var) or None                # aplicada igual pelo core ao injetar

  base_url = resolve("base_url")
  api_key = resolve("api_key")
  ```

  O core (Rust, `plugin_worker.rs`) aplica a mesma transformação de nome ao injetar
  (`Command::env(...)`) — nenhum nome de variável de ambiente trafega no protocolo, é derivado
  independentemente pelos dois lados a partir de `plugin_name` (`"uptime-kuma"`) + `name`
  (`"base_url"`/`"api_key"`).
- **Sem default seguro** para `base_url` (diferente do `scan_root` do `git-local`, que tem default
  `~/dev`) — não existe um host remoto default razoável para uma instância Uptime Kuma (FR-008, Edge
  Case do spec). Variável de ambiente ausente/vazia para `base_url` e/ou `api_key`: `not_configured`
  (ver `error-model-delta.md` — nesta revisão, salvaguarda; o caminho primário é o **core** nem chegar
  a chamar `widget/get`, ver abaixo).
- Ambos os valores são lidos **uma única vez**, no arranque do processo — sem re-leitura por chamada
  de `widget/get`, sem hot-reload dentro de um mesmo processo (decisão já existente, reafirmada). O
  caminho para "corrigir" um valor errado é a tela de setup do core (`data-model.md` §3.2): o usuário
  submete o formulário, o core persiste e **reinicia o processo do plugin** com as novas variáveis de
  ambiente — não um hot-reload dentro do processo já rodando.
- **MUST NOT**: nem `base_url` nem `api_key` MUST estar hardcoded em nenhum lugar do código do
  plugin. **MUST NOT**: a credencial (`api_key`) NUNCA é escrita em nenhum arquivo pelo próprio
  plugin, nunca aparece em log/stderr, nunca trafega por nenhuma mensagem do protocolo JSON-RPC — só
  a **declaração** de que ela é necessária (`required_config`, com `secret: true`) aparece no
  handshake, nunca o valor em si.
- **Provisionamento**: pela tela de setup do próprio Farol (`data-model.md` §3.2) — o usuário digita
  `base_url`/`api_key` numa janela do app, sem precisar editar arquivo nem instalar/autenticar
  nenhuma ferramenta externa (revisão desta sessão — versões anteriores deste documento exigiam um
  item já existente no cofre 1Password e uma sessão `op` autenticada; nenhum dos dois é mais
  necessário).
- **Onde o valor fica armazenado** (gerido inteiramente pelo core, não pelo plugin — `research.md`
  D8): `base_url` em `$XDG_CONFIG_HOME/farol/plugins/uptime-kuma/config.toml`; `api_key` em
  `$XDG_CONFIG_HOME/farol/secrets.toml` (permissão `0600`, seção `[uptime-kuma]`).

## Autenticação HTTP contra `/metrics` (FR-019)

HTTP Basic Auth — usuário vazio (ou qualquer valor), a `api_key` lida da variável de ambiente
(`research.md` D8, seção "Configuração e credencial" acima) como senha (Clarifications do spec):

```python
import base64
import urllib.request

def build_request(base_url: str, api_key: str) -> urllib.request.Request:
    credentials = base64.b64encode(f":{api_key}".encode()).decode()
    req = urllib.request.Request(f"{base_url.rstrip('/')}/metrics")
    req.add_header("Authorization", f"Basic {credentials}")
    return req
```

`urllib.request` (stdlib) é suficiente — nenhuma dependência externa é necessária para Basic Auth
(D7 de `research.md`).

## Leitura periódica de `/metrics` — thread de polling em background (FR-009, FR-010)

Ver `research.md` D6 para o desenho completo (cache, invariante de não-sobreposição, estado inicial).
Resumo operacional:

1. No arranque: se `not_configured` (`base_url`/`api_key` ausentes das variáveis de ambiente, D8), a
   thread **nunca inicia** — todo `widget/get` responde `error(-32005, not_configured)` para sempre
   (processo precisa ser reiniciado com configuração válida; na prática, o core normalmente já barra
   esta conexão antes de chegar a chamar `widget/get`, ver `error-model-delta.md`).
2. Caso contrário, uma `threading.Thread(daemon=True)` inicia um laço sequencial: a cada
   `suggested_refresh_interval_ms` (30000 default), faz a requisição HTTP (`build_request` acima),
   timeout de 10 segundos (`urllib.request.urlopen(req, timeout=10)`), parseia o corpo em sucesso
   (ver "Parsing" abaixo), atualiza o cache compartilhado (`threading.Lock`) com o resultado —
   sucesso vira `last_success`, qualquer falha (rede, HTTP não-2xx, parse) vira `last_error`, sem
   apagar um `last_success` anterior.
3. O handler de `widget/get` (rodando na thread principal do loop NDJSON) só lê o cache sob o mesmo
   lock — nunca faz I/O de rede diretamente. Ver a lógica de decisão em `data-model.md` §2.3.

## Parsing e mapeamento de status (FR-011, FR-012, FR-016)

- Reconhece **apenas** duas famílias de métrica no corpo `/metrics` (texto plano Prometheus):
  `monitor_status{monitor_name="...", ...} <valor>` e
  `monitor_response_time{monitor_name="...", ...} <valor>` — ambas gauges de uma linha. Qualquer
  outra família (`monitor_cert_days_remaining`, `monitor_uptime_ratio`, etc.) e qualquer linha de
  comentário (`#`) são ignoradas.
- Extração por linha: bloco de labels entre `{` e `}` (pares `chave="valor"`, interessa
  `monitor_name`) + valor numérico (último token da linha).
- Mapeamento de `monitor_status` → `status` (FR-012): `1 → "up"`, `0 → "down"`, `2 → "pending"`,
  `3 → "maintenance"`.
- `monitor_response_time` (float, ms) → `response_time_ms` (int, arredondado).
- **Falha de parsing** (`metrics_parse_error`, `-32007`), invalidando a resposta **inteira** daquela
  tentativa (não item a item — leitura literal do Edge Case do spec):
  - Nenhuma linha `monitor_status{...}` encontrada em todo o corpo (sinal de que a resposta não é um
    `/metrics` reconhecível do Uptime Kuma); **ou**
  - Algum valor de `monitor_status` encontrado está fora de `{0, 1, 2, 3}`.
- Linhas individuais malformadas dentro das duas famílias reconhecidas (ex.: falta `monitor_name`)
  são puladas — tolerância parcial, não derruba o parse inteiro por uma linha ruim isolada (distinto
  do caso acima, que é sobre o valor de `monitor_status` estar fora do domínio esperado, não sobre a
  linha estar malformada).
- Nomes de monitor (`monitor_name`) são usados **como vêm** do label, já sanitizados pelo próprio
  Uptime Kuma antes de chegar ao `/metrics` — sem tentativa de dessanitizar ou mapear de volta ao
  nome de exibição original (Assumptions do spec).

## Erros e disponibilidade (FR-015, FR-016, FR-017, FR-018)

- Falha de rede (host inacessível, timeout de 10s, HTTP não-2xx) → `metrics_unreachable` (`-32006`),
  erro pontual daquela leitura — o plugin continua respondendo normalmente a chamadas subsequentes de
  `widget/get`, a thread de polling continua tentando no próximo ciclo (FR-015).
- Resposta não parseável como Prometheus válido → `metrics_parse_error` (`-32007`), mesmo tratamento
  de erro pontual (FR-016).
- Nenhum dos dois casos encerra o processo do plugin (FR-017) nem depende de qualquer ação do core
  além de continuar chamando `widget/get` no próximo ciclo — recuperação é automática assim que a
  instância volta a responder corretamente (FR-018, US2 Acceptance Scenario 3 do spec).
- Isolamento de falha do **processo** do plugin (crash, trava, não resposta) é responsabilidade
  genérica do core, herdada sem modificação da feature 001 (FR-018 do spec desta feature; D6/D7 da
  feature 001) — não reespecificado aqui.

## O que este plugin explicitamente NÃO faz (reforça `Out of Scope` do spec)

- Nenhuma ação (`action/invoke`) — `actions: []` sempre, em toda mensagem.
- Nenhuma escrita/gerenciamento de monitores no Uptime Kuma — leitura pura de `/metrics`.
- Nenhum enforcement de allowlist de rede — a capacidade `network` é apenas declarada; o core não
  restringe nem media o acesso real do plugin à rede (mesmo padrão "declarado, não aplicado" de
  `exec`/`network` na feature 001). Não há mais mediação de acesso a nenhum cofre externo de
  segredos (1Password ou qualquer outro) — revisão desta sessão, o core armazena a credencial
  diretamente (`research.md` D8).
- Nenhum hot-reload de configuração dentro de um processo já rodando — `base_url`/`api_key` são
  fixados no arranque do processo, pelo resto da vida daquele processo; uma correção passa por
  reiniciar o processo via a tela de setup do core (`data-model.md` §3.2), não por o plugin observar
  o ambiente mudar em tempo real.
