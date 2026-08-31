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

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32005` | `not_configured` | resposta de `widget/get` | `base_url` ausente/vazio no arquivo de configuração (FR-008) **ou** credencial não resolvível via `op read` — item não encontrado no 1Password, sessão `op` não autenticada (FR-019). Detectado uma única vez no arranque do processo; permanece até o processo ser reiniciado com configuração válida (sem hot-reload nesta feature). `data.detail` MAY distinguir qual das duas causas se aplica, para orientar o usuário. |
| `-32006` | `metrics_unreachable` | resposta de `widget/get` | A última tentativa da thread de polling de contatar `${base_url}/metrics` falhou por motivo de rede: timeout HTTP (10s, não normativo do protocolo — `research.md` D5), conexão recusada, host incorreto, ou resposta HTTP não-2xx (inclui `401`/`403` de autenticação inválida — esta feature não distingue "credencial errada" de "host inacessível" dentro deste `reason`; FR-015/016 não pedem essa granularidade). `data.detail` MAY carregar a mensagem de exceção subjacente. |
| `-32007` | `metrics_parse_error` | resposta de `widget/get` | A última tentativa obteve uma resposta HTTP, mas o corpo não é reconhecível como `/metrics` Prometheus válido do Uptime Kuma: nenhuma linha `monitor_status{...}` encontrada, **ou** algum valor de `monitor_status` fora de `{0,1,2,3}` (Edge Case do spec — tratado como falha da resposta inteira daquela tentativa, leitura literal do texto do Edge Case). |

## Códigos reaproveitados sem alteração de significado

| `code` | `data.reason` | Reuso neste plugin |
|---|---|---|
| `-32000` | `protocol_version_incompatible` | Já genérico a qualquer plugin (`protocol/SPEC.md` §6.1/§8.2) — caminho de recusa do lado do plugin no handshake. `uptime-kuma`, como `git-local`, não exercita este caminho por conta própria nesta feature (sempre responde com `result`; é o **core** quem aplica a regra de compatibilidade sobre `"0.2"` recebido — mesmo padrão da feature 001). |
| `-32003` | `exec_unavailable` | Reaproveitado especificamente para "o binário `op` (1Password CLI) não está disponível no `PATH`" — mesmo significado textual já normativo ("um binário do qual o plugin depende para uma capacidade declarada não está disponível"), só a capacidade em questão passa a ser `secret` (via `exec`) em vez da operação principal de `git-local`. **Distinto** de `not_configured`: `exec_unavailable` = ambiente quebrado (ferramenta ausente); `not_configured` = ambiente correto, mas a referência/credencial não resolve. |

## Não reaproveitados nesta feature

`-32001 fetch_failed` e `-32002 action_timeout` — ambos ligados a `action/invoke`, que este plugin
nunca expõe (FR-004, `actions: []` sempre). Nenhum código novo é necessário para cobrir isso — a
ausência de ações torna esses `reason`s simplesmente inaplicáveis, não substituídos por outra coisa.

## Regra geral — inalterada

Nenhum erro desta tabela deriva o processo do plugin nem o core (`protocol/SPEC.md` §8.3, herdado
sem modificação). `-32005`/`-32006`/`-32007` são todos erros pontuais de uma única chamada de
`widget/get` — exibidos associados ao widget, sem afetar `PluginState`.
