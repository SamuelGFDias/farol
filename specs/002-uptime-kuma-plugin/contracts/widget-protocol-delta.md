# Contrato (delta): Widget Declarativo (`widget/get`) — novo `kind: "monitor-status-grid"`

**Pré-requisito**: `framing-and-versioning-delta.md`, `handshake-delta.md` (desta feature);
`specs/001-walking-skeleton-git-plugin/contracts/widget-protocol.md` (normativo, base — modelo de
polling core-iniciado, inalterado). Cobre FR-009 a FR-017 do `spec.md` desta feature.

## Modelo: polling core-iniciado — inalterado

O core continua chamando `widget/get` a cada ciclo de refresh (`suggested_refresh_interval_ms`,
default 30000ms — FR-009). Nada muda no lado do core: `widget/get` para `uptime-kuma-monitors` é,
do ponto de vista do core, uma chamada IPC local como qualquer outra, sujeita ao mesmo
`RPC_TIMEOUT_CONTROL`. **O que muda é inteiramente interno ao plugin** (D6 de `research.md`) — o
handler nunca faz I/O de rede, só lê um cache mantido por uma thread de polling em background.

## Request: `widget/get` — inalterado em forma

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "widget/get",
  "params": { "widget_id": "uptime-kuma-monitors" }
}
```

## Response (sucesso) — novo item `MonitorStatusItem`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "widget_id": "uptime-kuma-monitors",
    "items": [
      { "name": "api_example_com", "status": "up", "response_time_ms": 42 },
      { "name": "internal_service", "status": "down", "response_time_ms": null },
      { "name": "backup_job", "status": "pending", "response_time_ms": null }
    ]
  }
}
```

- `items` MAY ser uma lista vazia — instância Uptime Kuma acessível sem nenhum monitor cadastrado é
  estado válido (Edge Case do spec, análogo a diretório sem repositórios git da feature 001).
- Cada item é `MonitorStatusItem` (`data-model.md` §1.3) — **não** `WidgetItem` (que continua
  exclusivo de `kind: "status-grid"`, inalterado). Nenhum campo de ação: este widget nunca tem
  `fetch_action`/qualquer `ActionDeclaration` associada (FR-004).
- `status` é sempre um dos quatro valores do enum (FR-012). Um monitor cujo `monitor_status` bruto
  estivesse fora de `{0,1,2,3}` não aparece como um item com status desconhecido — invalida a
  resposta inteira daquela tentativa de leitura, tratada como `metrics_parse_error` (ver
  `error-model-delta.md`, Edge Case do spec).

## Response (erro) — três novos `reason`s, mesmo mecanismo já existente

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "error": {
    "code": -32006,
    "message": "falha ao consultar /metrics da instância Uptime Kuma configurada",
    "data": {
      "reason": "metrics_unreachable",
      "detail": "timeout ao conectar a monitor.example.com:443"
    }
  }
}
```

Ver `error-model-delta.md` para a tabela completa (`-32005 not_configured`, `-32006
metrics_unreachable`, `-32007 metrics_parse_error`). **Nenhuma mudança de mecanismo** —
`protocol/SPEC.md` §5.2 já normatiza, sem alteração, que um erro pontual de `widget/get` MUST NOT
mudar `PluginState`, e que o core MUST manter os últimos `items` conhecidos e sinalizar o erro
associado ao widget (FR-017, herdado sem modificação da feature 001).

## Onde a decoupling de FR-010 realmente acontece — nenhuma mudança de wire, só de implementação do plugin

Este contrato de **protocolo** (o formato das mensagens `widget/get`) não muda por causa de FR-010 —
a resposta continua sendo "sucesso com `items`" ou "erro pontual", exatamente como já era. O que
FR-010 exige é uma restrição sobre **como o plugin produz** essa resposta: nunca fazendo a chamada
HTTP síncrona dentro do handler. Isso é responsabilidade inteiramente interna ao plugin (D6 de
`research.md`; detalhes de implementação em `uptime-kuma-plugin.md`) — do ponto de vista do wire,
`widget/get` para este plugin parece idêntico a `widget/get` para `git-local`: uma requisição, uma
resposta rápida, dentro do orçamento de controle.

## Erros e timeout — mecanismo inalterado

- Erro pontual (`-32005`/`-32006`/`-32007`): core mantém os últimos `items` conhecidos, sinaliza o
  erro associado ao widget, `PluginState` não muda (§5.2, inalterado).
- Timeout de `RPC_TIMEOUT_CONTROL` numa chamada de `widget/get`: continua contribuindo para a
  detecção de plugin travado (D6 da feature 001, inalterado) — mas **não deveria, pelo desenho de
  D6 desta feature (`research.md`), nunca ocorrer por causa da instância Uptime Kuma estar lenta ou
  inacessível**, já que o handler nunca espera a rede. Um timeout de controle real neste plugin
  sinalizaria um problema do próprio processo do plugin (ex.: thread de polling segurando o lock por
  tempo desproporcional, bug), não da rede externa — diagnóstico útil para quem for implementar.
