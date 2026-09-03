# Contrato: delta de protocolo `0.2` → `0.3`

Diff normativo a implementar em `protocol/schema/v0.3/*.schema.json` (copiado de `v0.2/`, `v0.2/`
permanece congelado como histórico, mesmo tratamento de `v0.1`) e em `protocol/SPEC.md`. Decisões
justificadas em `research.md` D2-D5.

## `widget.schema.json`

1. Novo `$defs.VpnConnectionState`: `{"type": "string", "enum": ["disconnected", "connecting",
   "connected"]}`.
2. Novo `$defs.VpnProfile`:
   ```json
   {
     "type": "object",
     "properties": {
       "name": { "type": "string", "minLength": 1 },
       "connect_action": { "$ref": ".../handshake.schema.json#/$defs/ActionDeclaration" }
     },
     "required": ["name", "connect_action"],
     "additionalProperties": false
   }
   ```
3. Novo `$defs.VpnStatusItem`:
   ```json
   {
     "type": "object",
     "properties": {
       "state": { "$ref": "#/$defs/VpnConnectionState" },
       "active_profile": { "type": ["string", "null"] },
       "elapsed_seconds": { "type": ["number", "null"], "minimum": 0 },
       "available_profiles": { "type": "array", "items": { "$ref": "#/$defs/VpnProfile" } },
       "disconnect_action": { "$ref": ".../handshake.schema.json#/$defs/ActionDeclaration" }
     },
     "required": ["state", "active_profile", "elapsed_seconds", "available_profiles", "disconnect_action"],
     "additionalProperties": false
   }
   ```
4. `WidgetGetResult.items` — `anyOf` ganha uma terceira opção: `{"type": "array", "items": {"$ref":
   "#/$defs/VpnStatusItem"}}`. Aditivo — as duas opções existentes (`WidgetItem[]`/
   `MonitorStatusItem[]`) permanecem idênticas.

## `action.schema.json`

`ActionInvokeResult` deixa de ser um `type: object` fixo e passa a `oneOf`:

```json
"ActionInvokeResult": {
  "oneOf": [
    {
      "type": "object",
      "properties": { "repo": { "$ref": "widget.schema.json#/$defs/GitRepository" } },
      "required": ["repo"],
      "additionalProperties": false
    },
    {
      "type": "object",
      "properties": { "vpn_status": { "$ref": "widget.schema.json#/$defs/VpnStatusItem" } },
      "required": ["vpn_status"],
      "additionalProperties": false
    }
  ]
}
```

Wire já emitido por `git-local` (`{"repo": {...}}`) continua validando contra a primeira opção, sem
mudança nenhuma do lado daquele plugin.

## `error.schema.json`

Estende só o catálogo textual (descrição do schema, `data.reason` já é string aberta — nenhuma
mudança de forma):

- `-32008` → `vpn_status_unavailable` → resposta de `widget/get` → `openfortivpn-gui status --json`
  falhou (`internal_error` da CLI ou saída não interpretável) enquanto o binário está presente no
  `PATH`. `data.detail` MAY carregar a mensagem bruta.
- `-32009` → `vpn_action_failed` → resposta de `action/invoke` → `connect`/`disconnect` da CLI
  falhou. `data.detail` = `{"cli_code": <um de profile_not_found/already_connected/not_connected/
  connect_timeout/sudo_denied/internal_error>, "cli_message": <mensagem bruta da CLI>}`.

## `handshake.schema.json`

Sem mudança de forma. Só o comentário de topo do arquivo (`description`) é atualizado para citar
`"0.3"` e o novo `kind`/plugin, mesmo padrão do texto de topo de `v0.2/widget.schema.json` citando
"Evolved from v0.1 ...".

## `protocol/SPEC.md`

- §6.4 (Versionamento): nenhuma mudança de regra — só a nota de "versão atual: `0.3`".
- §6.3.1 ou seção equivalente: registrar `"vpn-status"` ao lado de `"status-grid"`/
  `"monitor-status-grid"` como `kind` de widget conhecido, com a mesma forma de explicação (o que
  cada `kind` reporta em `widget/get`).
- §8 (Modelo de erro): acrescentar as duas linhas do catálogo acima na tabela existente.
