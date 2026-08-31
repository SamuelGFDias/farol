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

## Response (sucesso): `HandshakeHelloResult` — `capabilities` estruturado (D1), demais campos inalterados em forma

### Caso configurado (base_url presente, credencial resolvível via `op`)

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.2",
    "plugin_name": "uptime-kuma",
    "capabilities": {
      "capabilities": [
        { "kind": "exec" },
        { "kind": "network", "host": "monitor.example.com", "port": 443 },
        { "kind": "secret", "reference": "op://Dev/UptimeKuma/API Keys/farol" }
      ]
    },
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

### Caso `not_configured` (`base_url` ausente e/ou credencial não resolvível — FR-008/FR-019)

O handshake **ainda completa com sucesso** — o processo está vivo, o widget é declarado; só
`widget/get` carrega o sinal de erro (ver `widget-protocol-delta.md`, e `research.md` D9 para a
justificativa de por que essa condição não falha o handshake em si). `capabilities` omite as
entradas `network`/`secret` (nada concreto a declarar honestamente — D1):

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.2",
    "plugin_name": "uptime-kuma",
    "capabilities": { "capabilities": [ { "kind": "exec" } ] },
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

## `capabilities` — forma estruturada (D1 de `research.md`)

Ver `research.md` D1 para o desenho completo do schema `Capability`/`CapabilityManifest` e o schema
JSON ilustrativo (`allOf`/`if`/`then` por `kind`). Resumo dos três `kind`s conhecidos nesta versão:

| `kind` | Campos extras | Declarado por `uptime-kuma` quando... |
|---|---|---|
| `exec` | nenhum | Sempre — usado para invocar `op` (D8) e, no futuro, qualquer outro binário de sistema. |
| `network` | `host` (obrigatório), `port` (opcional) | Quando `base_url` está configurado — `host`/`port` derivados de `urllib.parse.urlsplit(base_url)`, com porta default por esquema (443 https / 80 http) quando não explícita na URL. |
| `secret` | `reference` (obrigatório) | Quando a credencial via `op` é resolvível no arranque (D8/D9) — `reference` é a constante fixa `"op://Dev/UptimeKuma/API Keys/farol"` (uma única instância suportada por vez, Assumptions do spec). |

**MUST**: `network`/`secret` só aparecem em `capabilities` quando o plugin tem um valor concreto,
verdadeiro, para declarar — nunca um placeholder vazio só para satisfazer a contagem mínima de
`minItems: 1` do manifesto (que `exec`, sempre presente, já garante).

## `actions: []` — sempre, sem exceção (FR-004)

Diferente de `git-local` (que populava `actions` via `widget/get`, não no handshake — ver nota de
sequenciamento da feature 001), `uptime-kuma` **nunca** declara nenhuma `ActionDeclaration`, nem no
handshake nem em nenhuma resposta subsequente de `widget/get`. O campo `actions` do
`HandshakeHelloResult` MUST vir `[]` e permanece `[]` pelo resto da conexão.

## Timeout — inalterado

`RPC_TIMEOUT_CONTROL` (5s default), o mesmo orçamento de sempre. A resolução de configuração/keyring
(leitura do TOML + `op read`) acontece **antes** do plugin responder ao handshake — mas ambas as
operações são rápidas (leitura de arquivo local + uma chamada de subprocess ao `op`, tipicamente
< 1s) e não envolvem a rede até o Uptime Kuma (essa chamada só acontece na thread de polling,
`widget-protocol-delta.md`) — folgado dentro do orçamento de 5s do mesmo jeito que o handshake de
`git-local` já era.
