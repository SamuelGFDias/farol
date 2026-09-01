# Contrato (delta): Modelo de Erro — novos `reason`s do plugin `uptime-kuma`

**Pré-requisito**: `framing-and-versioning-delta.md` (desta feature);
`specs/001-walking-skeleton-git-plugin/contracts/error-model.md` (normativo, base — forma do objeto
de erro, faixas de código reservadas). Cobre FR-008, FR-015, FR-016, FR-019.

## Forma do objeto de erro — inalterada

Mesma forma JSON-RPC 2.0 padrão (`code`, `message`, `data.reason` + campos extras) já normativa.
Nenhuma mudança de estrutura — só um novo conjunto de valores de `data.reason`, dentro da mesma faixa
de domínio Farol (`-32000` a `-32099`) já reservada por `protocol/SPEC.md` §8.2/§10.

## Novos códigos de domínio Farol — plugin `uptime-kuma`

`protocol/SPEC.md` §8.2/§10 permite explicitamente que cada plugin reserve seus próprios `reason`s
dentro da faixa, documentados na seção do próprio plugin, sem mecanismo de registro central. Os
códigos abaixo foram escolhidos para não colidir com os já usados por `git-local`
(`-32000`..`-32004`), por prudência — não há garantia formal de unicidade entre plugins distintos
(mesma ressalva já registrada em `protocol/SPEC.md`).

> **Revisão desta sessão (auditoria pós-plan — substitui H5/H6 identificados numa auditoria técnica
> anterior)**: sem CLI `op`, não existe mais a distinção "binário externo ausente" (`exec_unavailable`)
> vs. "binário presente mas falhando" (`not_configured`) para credencial — essa distinção só fazia
> sentido quando o *plugin* invocava uma ferramenta externa. Nesta arquitetura (`research.md` D8), o
> **core** gerencia armazenamento/injeção; `not_configured` (`-32005`) passa a cobrir tanto "variável
> de ambiente não injetada/vazia" quanto "arquivo de storage (`config.toml`/`secrets.toml`) ausente ou
> corrompido do lado do core" — este último caso, na prática, nem chega a esta resposta de
> `widget/get`: o core já barra o spawn normal da conexão (`PluginState = Unavailable{NotConfigured}`)
> e mostra a tela de setup antes de sequer chamar `widget/get` (ver "Papel revisado de `not_configured`"
> abaixo). `exec_unavailable` (`-32003`) deixa de ter uso neste plugin — continua existindo no
> catálogo geral do protocolo (usado por `git-local` para o binário `git`).

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32005` | `not_configured` | resposta de `widget/get` (salvaguarda — ver abaixo) | Alguma variável de `required_config` (`base_url` e/ou `api_key`) ausente/vazia na *environment* do processo do plugin (`FAROL_PLUGIN_UPTIME_KUMA_...`, `research.md` D8) — cobre tanto "core não injetou porque não havia valor armazenado" quanto "arquivo de storage do core ausente/corrompido" (este segundo caso normalmente nunca chega até aqui, ver abaixo). Detectado uma única vez no arranque do processo; permanece até o processo ser reiniciado com configuração válida (sem hot-reload nesta feature). |
| `-32006` | `metrics_unreachable` | resposta de `widget/get` | A última tentativa da thread de polling de contatar `${base_url}/metrics` falhou por motivo de rede: timeout HTTP (10s, não normativo do protocolo — `research.md` D5), conexão recusada, host incorreto, ou resposta HTTP não-2xx (inclui `401`/`403` de autenticação inválida — esta feature não distingue "credencial errada" de "host inacessível" dentro deste `reason`; FR-015/016 não pedem essa granularidade). `data.detail` MAY carregar a mensagem de exceção subjacente. |
| `-32007` | `metrics_parse_error` | resposta de `widget/get` | A última tentativa obteve uma resposta HTTP, mas o corpo não é reconhecível como `/metrics` Prometheus válido do Uptime Kuma: nenhuma linha `monitor_status{...}` encontrada, **ou** algum valor de `monitor_status` fora de `{0,1,2,3}` (Edge Case do spec — tratado como falha da resposta inteira daquela tentativa, leitura literal do texto do Edge Case). |

### Papel revisado de `not_configured` — salvaguarda, não caminho primário (D8/D9 de `research.md`)

O caminho **primário** pelo qual o usuário percebe "não configurado" deixa de ser um erro de
`widget/get` — passa a ser o **core** recusando avançar `PluginState` para `Ready` (transição para
`Unavailable{NotConfigured}`, novo `UnavailableReason`) assim que ele compara o `required_config`
recebido no handshake contra o que conseguiu injetar como variável de ambiente; nesse estado o core
não chama `widget/get` para essa conexão — em vez disso, exibe a tela de setup construída a partir de
`required_config` (`data-model.md` §3.2). `error(-32005, not_configured)` de `widget/get` continua
existindo como **salvaguarda de defesa em profundidade**, alcançável na prática só se a barreira do
core, por algum motivo, deixar passar uma conexão sem todos os valores injetados.

## Códigos reaproveitados sem alteração de significado

| `code` | `data.reason` | Reuso neste plugin |
|---|---|---|
| `-32000` | `protocol_version_incompatible` | Já genérico a qualquer plugin (`protocol/SPEC.md` §6.1/§8.2) — caminho de recusa do lado do plugin no handshake. `uptime-kuma`, como `git-local`, não exercita este caminho por conta própria nesta feature (sempre responde com `result`; é o **core** quem aplica a regra de compatibilidade sobre `"0.2"` recebido — mesmo padrão da feature 001). |

## Não reaproveitados nesta feature

- **`-32003` `exec_unavailable`**: continua existindo no catálogo geral do protocolo (`git-local` o usa
  para o binário `git`), mas **sem uso por `uptime-kuma`** desde a revisão de D8 — este plugin não
  invoca mais nenhum binário externo (a chamada `op` que justificava reaproveitar este `reason` em
  versões anteriores deste documento não existe mais).
- **`-32001 fetch_failed`** e **`-32002 action_timeout`** — ambos ligados a `action/invoke`, que este
  plugin nunca expõe (FR-004, `actions: []` sempre). Nenhum código novo é necessário para cobrir isso —
  a ausência de ações torna esses `reason`s simplesmente inaplicáveis, não substituídos por outra coisa.

## Regra geral — inalterada

Nenhum erro desta tabela deriva o processo do plugin nem o core (`protocol/SPEC.md` §8.3, herdado
sem modificação). `-32005`/`-32006`/`-32007` são todos erros pontuais de uma única chamada de
`widget/get` — exibidos associados ao widget, sem afetar `PluginState`.
