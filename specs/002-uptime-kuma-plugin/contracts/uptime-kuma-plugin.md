# Contrato: Plugin de Referência `uptime-kuma`

**Pré-requisito**: todos os contratos-delta desta pasta. Este documento cobre o que é específico do
plugin de referência `uptime-kuma` (não genérico ao protocolo Farol) — FR-007 a FR-020, análogo a
`specs/001-walking-skeleton-git-plugin/contracts/git-local-plugin.md`.

## Identidade

- `plugin_name`: `"uptime-kuma"`.
- `protocol_version`: `"0.2"` (D1 de `research.md`).
- `capabilities.capabilities`: `[{"kind": "exec"}]` sempre; `+{"kind": "network", ...}` e
  `+{"kind": "secret", ...}` quando configurado (ver `handshake-delta.md`).
- Linguagem de implementação: **Python 3.11+, apenas biblioteca padrão** (D7 de `research.md`,
  reafirma D3 da feature 001) — nenhuma dependência em `farol-protocol` nem em qualquer código Rust
  do core, nenhuma dependência via `pip`.
- **Dependência de sistema** (não Python): binário **`op`** (1Password CLI), instalado e
  **autenticado** (sessão ativa) no ambiente onde o processo do plugin roda (D8) — mesma categoria de
  pré-requisito que o binário `git` já é para `git-local`.

## Widget oferecido

- Um único widget: `id: "uptime-kuma-monitors"`, `kind: "monitor-status-grid"`,
  `title: "Uptime Kuma"`.
- `suggested_refresh_interval_ms`: **enviado, sempre `30000`** (diferente de `git-local`, que
  deliberadamente omitia este campo para exercitar o ramo "default do core" — esta feature exercita
  o outro ramo de FR-009, "sugestão do plugin", já coberto o ramo default pela feature 001; não há
  necessidade de repetir a cobertura do mesmo ramo numa segunda feature). Este valor **é o mesmo**
  usado internamente como cadência da thread de polling (D6) — fonte única, não dois conceitos de
  intervalo desacoplados.

## Configuração (FR-007, FR-008)

- Arquivo de configuração próprio do plugin — **não é lido pelo core, nem faz parte do protocolo
  JSON-RPC**. Caminho: `$XDG_CONFIG_HOME/farol/plugins/uptime-kuma/config.toml` (fallback
  `~/.config/farol/plugins/uptime-kuma/config.toml` — mesma convenção XDG de `git-local`).
- Formato TOML, campo único usado nesta feature:

  ```toml
  # ~/.config/farol/plugins/uptime-kuma/config.toml
  base_url = "https://monitor.example.com"
  ```

- **Sem default seguro** (diferente do `scan_root` do `git-local`, que tem default `~/dev`) — não
  existe um host remoto default razoável para uma instância Uptime Kuma (FR-008, Edge Case do spec).
  Arquivo ausente, ou presente sem `base_url`: `not_configured` (ver `error-model-delta.md`).
- `base_url` é lido **uma única vez**, no arranque do processo — sem re-leitura por chamada de
  `widget/get`, sem hot-reload nesta feature (decisão explícita deste plano, análoga em espírito à
  ausência de qualquer requisito de reload na feature 001).
- **MUST NOT**: `base_url` MUST NOT estar hardcoded em nenhum lugar do código do plugin fora deste
  mecanismo de configuração.

## Credencial (FR-019)

- **Referência fixa**, não configurável pelo usuário nesta feature (só uma instância suportada por
  vez): `"op://Dev/UptimeKuma/API Keys/farol"` — formato de referência de secret do CLI `op` do
  1Password (`op://<vault>/<item>/<campo>`), documentada e resolvida pelo próprio plugin (D8 de
  `research.md`).
- Resolução, uma única vez no arranque do processo (concorrente com a espera pelo `handshake/hello`
  do core), via `subprocess`:

  ```python
  import subprocess

  REFERENCE = "op://Dev/UptimeKuma/API Keys/farol"

  def resolve_api_key() -> str | None:
      try:
          result = subprocess.run(
              ["op", "read", REFERENCE],
              capture_output=True, text=True, timeout=5,
          )
      except FileNotFoundError:
          return None  # binário `op` ausente do PATH -> exec_unavailable (-32003)
      if result.returncode != 0 or not result.stdout:
          return None  # op presente, mas sem resolver a referência -> not_configured (-32005)
      return result.stdout.rstrip("\n")
  ```

  A distinção entre "binário `op` ausente" (`FileNotFoundError`, tratado como `exec_unavailable`,
  `-32003`) e "`op` presente mas retornou erro" (código de saída não-zero — item não encontrado,
  sessão não autenticada, tratado como `not_configured`, `-32005`) é feita no arranque do processo,
  antes de responder ao `handshake/hello` (ver `handshake-delta.md`).
- **MUST NOT**: a credencial (a API Key/senha em si) NUNCA é escrita em nenhum arquivo de
  configuração do plugin, nunca aparece em log/stderr, nunca trafega por nenhuma mensagem do
  protocolo JSON-RPC — só o **caminho de referência** (`op://...`) aparece, e só dentro do manifesto
  de capacidades (`capabilities`).
- **Provisionamento** (pré-requisito manual do usuário, fora do escopo desta feature automatizar —
  documentado em `quickstart.md`): o item precisa já existir no cofre 1Password apontado pela
  referência, e a sessão `op` do ambiente onde o Farol roda precisa estar autenticada.

## Autenticação HTTP contra `/metrics` (FR-019)

HTTP Basic Auth — usuário vazio (ou qualquer valor), a API Key resolvida via `op` como senha
(Clarifications do spec):

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

1. No arranque: se `not_configured` (config ou credencial ausente), a thread **nunca inicia** — todo
   `widget/get` responde `error(-32005, not_configured)` para sempre (processo precisa ser
   reiniciado com configuração válida).
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
- Nenhum enforcement de allowlist de rede nem de acesso ao 1Password — capacidades `network`/`secret`
  são apenas declaradas; o core não restringe, nem media, o acesso real do plugin nem à rede nem ao
  1Password (mesmo padrão "declarado, não aplicado" de `exec` na feature 001).
- Nenhum hot-reload de configuração — `base_url` e a resolução da credencial são fixados no arranque
  do processo, pelo resto da vida daquele processo.
