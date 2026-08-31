# Contrato: Plugin de Referência `git-local`

**Pré-requisito**: todos os contratos anteriores nesta pasta. Este documento cobre o que é
específico do plugin de referência (não genérico ao protocolo Farol) — FR-012, FR-013, FR-014,
manifesto de capacidades, e a decisão de linguagem (D3 de `research.md`).

## Identidade

- `plugin_name`: `"git-local"`.
- `protocol_version`: `"0.1"`.
- `capabilities.capabilities`: `["exec"]` — única capacidade desta feature, cobre a invocação do
  binário `git` (Assumptions da spec).
- Linguagem de implementação: **Python 3.11+, apenas biblioteca padrão** (D3) — nenhuma dependência
  em `farol-protocol` nem em qualquer código Rust do core. Implementa o protocolo lendo
  exclusivamente `framing-and-versioning.md` + os demais contratos desta pasta (que, no repositório
  real, correspondem a `protocol/SPEC.md` + `protocol/schema/`).

## Widget oferecido

- Um único widget: `id: "repo-status"`, `kind: "status-grid"`, `title: "Repositórios Git"`.
- `suggested_refresh_interval_ms`: **não enviado** (o plugin de referência não sugere intervalo —
  exercita o caminho "default de 30s" do core, FR-011). Documentado aqui como escolha deliberada
  para que o walking skeleton cubra os dois ramos de FR-011 (default vs. sugestão do plugin) — o
  ramo "sugestão presente" fica coberto por teste de contrato usando um payload sintético (ver
  `quickstart.md`), já que o plugin de referência real não precisa exercitar ambos os ramos.

## Configuração (FR-012)

- Arquivo de configuração próprio do plugin — **não é lido pelo core, nem faz parte do protocolo
  JSON-RPC**. Caminho: `$XDG_CONFIG_HOME/farol/plugins/git-local/config.toml` (fallback
  `~/.config/farol/plugins/git-local/config.toml` quando `XDG_CONFIG_HOME` não está definida —
  convenção XDG padrão em Linux, consistente com Princípio I da constitution, "app nativo Linux").
- Formato TOML, campo único usado nesta feature:

  ```toml
  # ~/.config/farol/plugins/git-local/config.toml
  scan_root = "~/dev"
  ```

- Se o arquivo não existir, ou existir mas não definir `scan_root`: default `~/dev` (FR-012,
  Clarifications Q3). `~` é expandido pelo próprio plugin (não pelo core — o core nunca lê nem
  interpreta este arquivo).
- **MUST NOT**: o caminho `scan_root` MUST NOT estar hardcoded em nenhum lugar do código do plugin
  fora deste mecanismo de configuração — nem mesmo como valor de teste (o default `~/dev` é a única
  string literal de caminho permitida, e só como fallback, nunca como valor fixo ignorando o
  arquivo).

## Varredura (FR-013)

- O plugin varre (não-recursivamente em profundidade arbitrária — um nível: subdiretórios diretos
  de `scan_root` que contêm um diretório/arquivo `.git`) o `scan_root` a cada `widget/get`
  recebido (sem cache persistente entre chamadas nesta feature — cada `widget/get` refaz a
  varredura; é aceitável porque o ciclo é de 30s, não um caminho de alta frequência).
- Para cada repositório encontrado, reporta via `git` (subprocess, capacidade `exec`):
  - `dirty`: equivalente a `git status --porcelain` não-vazio.
  - `remote_status`: se o repositório não tem nenhum remote configurado (`git remote` vazio) →
    `{"kind":"no_remote"}` (FR-014). Caso contrário, `ahead`/`behind` do branch atual em relação ao
    upstream configurado (equivalente a `git rev-list --left-right --count
    <upstream>...HEAD`), reportados como `{"kind":"tracked","ahead":N,"behind":M}`.
- Diretório configurado inexistente ou sem nenhum repositório git sob ele → `widget/get` retorna
  sucesso com `items: []` (estado vazio válido — Edge Case da spec, Assumptions), nunca um erro.

## Ação de fetch

- `action_id`: `"git.fetch"` — mesmo identificador para todo repositório (o que diferencia é
  `target.id`, o caminho do repositório — ver `action-protocol.md`).
- `enabled`: `false` sempre que `remote_status.kind == "no_remote"` para aquele repositório
  (Clarifications Q4); `true` caso contrário.
- Execução: `git fetch` (sem argumentos extras — usa o remote/refspec padrão configurado no
  repositório) via subprocess, no diretório do repositório-alvo.
- Falha do `git fetch` (código de saída != 0) → resposta de erro `-32001 fetch_failed`
  (`error-model.md`), **sem** encerrar o processo do plugin (FR-017) — o plugin continua
  respondendo a `widget/get`/outras invocações depois.
- Binário `git` ausente do sistema → erro `-32003 exec_unavailable` na primeira tentativa de uso
  (seja em `widget/get`, seja em `action/invoke`); o plugin não crasha, apenas reporta a
  indisponibilidade daquela operação (Edge Case da spec).
