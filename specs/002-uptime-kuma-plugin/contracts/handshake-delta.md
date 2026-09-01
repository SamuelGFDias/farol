# Contrato (delta): Handshake de Inicialização (`handshake/hello`)

**Pré-requisito**: `framing-and-versioning-delta.md` (desta feature); `specs/001-walking-skeleton-git-plugin/contracts/handshake.md`
(normativo, base). Cobre FR-002, FR-003, FR-004, FR-005, FR-006 do `spec.md` desta feature.

## Sequenciamento — inalterado

Mesmo diagrama, mesmas regras da feature 001 (`handshake.md`): o core envia `handshake/hello`
primeiro, o plugin responde antes de aceitar qualquer outro método, `RPC_TIMEOUT_CONTROL` se aplica.
Nada disso muda para `uptime-kuma`.

## Request: `handshake/hello` — inalterado em forma

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "handshake/hello",
  "params": {
    "protocol_version": "0.2",
    "core_name": "farol-core"
  }
}
```

## Response (sucesso): `HandshakeHelloResult` — `capabilities` estruturado (D1) + `required_config` (NOVO, D8)

**Revisão desta sessão (auditoria pós-plan, 2026-09-01)**: os exemplos abaixo substituem versões
anteriores deste documento, que modelavam a credencial como uma capacidade `{"kind": "secret",
"reference": "op://..."}` resolvida pelo plugin via CLI `op`. Essa decisão foi substituída — a
credencial (e a URL base) agora são declaradas via um campo novo, `required_config`, irmão de
`capabilities`/`widgets`/`actions`; o core (não o plugin) armazena e injeta os valores (`research.md`
D8). Como consequência, `uptime-kuma` também deixa de declarar `{"kind": "exec"}` — não invoca mais
nenhum binário externo.

### Caso configurado (`base_url`/`api_key` resolvidos pelo core e injetados como variável de ambiente)

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.2",
    "plugin_name": "uptime-kuma",
    "capabilities": {
      "capabilities": [
        { "kind": "network", "host": "monitor.example.com", "port": 443 }
      ]
    },
    "required_config": [
      { "name": "base_url", "secret": false, "description": "URL base da instância Uptime Kuma" },
      { "name": "api_key", "secret": true, "description": "API Key de métricas do Uptime Kuma" }
    ],
    "widgets": [
      {
        "id": "uptime-kuma-monitors",
        "kind": "monitor-status-grid",
        "title": "Uptime Kuma",
        "suggested_refresh_interval_ms": 30000
      }
    ],
    "actions": []
  }
}
```

### Caso `not_configured` (algum item de `required_config` sem valor injetado — FR-008/FR-019)

O handshake **ainda completa com sucesso** — o processo está vivo; `required_config` é declarado
**sempre**, independentemente de já haver valor armazenado ou não (D8 — é exatamente essa declaração
fixa que permite ao core montar a tela de setup mesmo na primeira execução). O que muda entre este
caso e o anterior é: (a) `capabilities` omite `network` (nada concreto a declarar honestamente — D1),
e (b) o **core**, ao comparar `required_config` recebido contra o que conseguiu injetar, decide não
avançar a conexão para `Ready` — `PluginState = Unavailable{NotConfigured}`, tela de setup exibida em
vez do widget (`research.md` D8/D9; ver `widget-protocol-delta.md` para a salvaguarda de `widget/get`
que só é alcançável se essa barreira do core falhar):

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.2",
    "plugin_name": "uptime-kuma",
    "capabilities": { "capabilities": [] },
    "required_config": [
      { "name": "base_url", "secret": false, "description": "URL base da instância Uptime Kuma" },
      { "name": "api_key", "secret": true, "description": "API Key de métricas do Uptime Kuma" }
    ],
    "widgets": [
      {
        "id": "uptime-kuma-monitors",
        "kind": "monitor-status-grid",
        "title": "Uptime Kuma",
        "suggested_refresh_interval_ms": 30000
      }
    ],
    "actions": []
  }
}
```

## `capabilities` — forma estruturada (D1 de `research.md`, revisado)

Ver `research.md` D1 para o desenho completo do schema `Capability`/`CapabilityManifest` e o schema
JSON ilustrativo (`allOf`/`if`/`then` por `kind`). Resumo dos `kind`s conhecidos nesta versão que
`uptime-kuma` pode declarar:

| `kind` | Campos extras | Declarado por `uptime-kuma` quando... |
|---|---|---|
| `network` | `host` (obrigatório), `port` (opcional) | Quando `base_url` está configurado (resolvido via variável de ambiente injetada pelo core, D8) — `host`/`port` derivados de `urllib.parse.urlsplit(base_url)`, com porta default por esquema (443 https / 80 http) quando não explícita na URL. |

`uptime-kuma` **não** declara `{"kind": "exec"}` (revisão de D8 — não invoca mais nenhum binário
externo) nem `{"kind": "secret"}` (removido do vocabulário — substituído por `required_config`,
acima). `capabilities` MAY ser `[]` (sem `minItems: 1`) quando `base_url` ainda não está resolvido.

**MUST**: `network` só aparece em `capabilities` quando o plugin tem um valor concreto, verdadeiro,
para declarar — nunca um placeholder vazio só para satisfazer a forma do manifesto.

## `required_config` (NOVO, D8) — sempre declarado, independente de já configurado

Ver `research.md` D8 e `data-model.md` §1.6.1 para o desenho completo do tipo `RequiredConfigItem`.
`uptime-kuma` declara, sempre, os dois itens do exemplo acima (`base_url`, não-secreto; `api_key`,
secreto) — a lista não muda entre o caso configurado e o caso `not_configured`; o que muda é se o
core conseguiu injetar valor para cada um.

## `actions: []` — sempre, sem exceção (FR-004)

Diferente de `git-local` (que populava `actions` via `widget/get`, não no handshake — ver nota de
sequenciamento da feature 001), `uptime-kuma` **nunca** declara nenhuma `ActionDeclaration`, nem no
handshake nem em nenhuma resposta subsequente de `widget/get`. O campo `actions` do
`HandshakeHelloResult` MUST vir `[]` e permanece `[]` pelo resto da conexão.

## Timeout — inalterado

`RPC_TIMEOUT_CONTROL` (5s default), o mesmo orçamento de sempre. A leitura de configuração (D8
revisado: `os.environ.get()` para cada item de `required_config`, nenhum arquivo/subprocess do lado
do plugin) acontece **antes** do plugin responder ao handshake — uma leitura de variável de ambiente
em memória do processo, ordens de magnitude mais rápida que a antiga combinação de leitura de TOML +
chamada de subprocess a `op`, e não envolve a rede até o Uptime Kuma (essa chamada só acontece na
thread de polling, `widget-protocol-delta.md`) — folgado dentro do orçamento de 5s do mesmo jeito que
o handshake de `git-local` já era.
