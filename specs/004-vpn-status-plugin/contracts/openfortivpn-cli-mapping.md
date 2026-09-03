# Contrato: mapeamento CLI `openfortivpn-gui` → protocolo Farol

Fonte normativa da CLI: `../../../openfortivpn-gui/specs/001-add-cli-interface/contracts/
cli-commands.md` e `.../contracts/status-schema.json` (não copiado aqui — este documento só mapeia,
não redefine aquele contrato).

## `widget/get` (widget `vpn-status`, id `vpn-connection`)

1. Se `shutil.which("openfortivpn-gui")` é `None` → resposta de erro `-32003`/`exec_unavailable`
   (sem executar nada).
2. Senão, executar `openfortivpn-gui status --json`.
   - Exit `0`, JSON válido no shape `StatusPayload` → sucesso, `items: [VpnStatusItem]` (mapeamento
     de campos em `data-model.md` §3).
   - Exit `0` mas JSON no shape `ErrorPayload` (`error.code == "internal_error"`, único código
     possível para `status`), ou saída não parseável como JSON, ou exit inesperado ≠ `0` → erro
     `-32008`/`vpn_status_unavailable`, `data.detail` = mensagem bruta (stdout+stderr truncados).

## `action/invoke` — `vpn.connect`

Request: `action_id: "vpn.connect"`, `target: {type: "vpn-profile", id: <perfil>}`.

1. Executar `openfortivpn-gui connect <perfil> --json` (sem `--timeout` customizado nesta
   revisão — default da CLI, ~20s, é suficiente; `timeout_hint_ms` da `ActionDeclaration`
   correspondente MUST ser maior que esse default, ex. `25000`, para o core não sintetizar
   `action_timeout` antes da CLI resolver, `research.md` Technical Context).
2. Exit `0`, `StatusPayload` com `state: "connected"` → sucesso, `result: {"vpn_status":
   <VpnStatusItem mapeado>}`.
3. Exit `1`, `ErrorPayload` → erro `-32009`/`vpn_action_failed`, `message` = tradução PT-BR de
   `error.code` (tabela abaixo, FR-007), `data.detail = {"cli_code": error.code, "cli_message":
   error.message}`.

## `action/invoke` — `vpn.disconnect`

Request: `action_id: "vpn.disconnect"`, `target: {type: "vpn-connection", id: "active"}`.

1. Executar `openfortivpn-gui disconnect --json`.
2. Exit `0`, `StatusPayload` com `state: "disconnected"` → sucesso, `result: {"vpn_status":
   <VpnStatusItem mapeado>}`.
3. Exit `1`, `ErrorPayload` → mesmo tratamento do item 3 de `vpn.connect` acima.

## Tabela de tradução `error.code` → mensagem legível (FR-007)

| `error.code` da CLI | Comando(s) | Mensagem PT-BR sugerida |
|---|---|---|
| `profile_not_found` | connect | "Perfil não encontrado — pode ter sido removido ou renomeado." |
| `already_connected` | connect | "Já existe uma conexão VPN ativa." |
| `not_connected` | disconnect | "Não há conexão VPN ativa para desconectar." |
| `connect_timeout` | connect | "A conexão não confirmou dentro do tempo esperado." |
| `sudo_denied` | connect, disconnect | "Permissão de sistema negada para abrir/fechar o túnel VPN." |
| `internal_error` | connect, disconnect, status | "Erro interno ao consultar/operar a VPN." |

`message` do `ErrorObject` do protocolo Farol usa sempre a tradução acima — nunca o `error.message`
bruto da CLI como única informação (FR-007); o bruto vai só em `data.detail.cli_message`, para
diagnóstico.

## Casos de borda cobertos (rastreabilidade com `spec.md` § Edge Cases)

| Edge case do `spec.md` | Tratamento |
|---|---|
| `openfortivpn-gui` ausente do `PATH` | `-32003`/`exec_unavailable` (item 1 de `widget/get` acima) |
| Nenhum perfil configurado (`profiles: []`) | Sucesso, `available_profiles: []` — não é erro |
| Conectar a perfil inexistente | `-32009`/`vpn_action_failed`, `cli_code: profile_not_found` |
| Timeout de conexão | `-32009`/`vpn_action_failed`, `cli_code: connect_timeout` |
| Conectar já conectado / desconectar já desconectado | `-32009`/`vpn_action_failed`, `cli_code: already_connected`/`not_connected` |
| Permissão de sistema negada | `-32009`/`vpn_action_failed`, `cli_code: sudo_denied` |
| Estado muda por via externa ao Farol (GUI, outro processo) | Refletido no próximo `widget/get` — mesma limitação já aceita para os demais widgets (sem push) |
