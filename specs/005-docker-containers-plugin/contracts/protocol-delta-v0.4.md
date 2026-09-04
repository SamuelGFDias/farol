# Contrato: delta de protocolo `0.3` → `0.4`

Diff normativo a implementar em `protocol/schema/v0.4/*.schema.json` (copiado de `v0.3/`; `v0.3/`
permanece congelado como histórico, mesmo tratamento já dado a `v0.1`/`v0.2`) e em
`protocol/SPEC.md`. Decisões justificadas em `research.md` D2-D5, D11, D12.

**Natureza do delta**: 100% aditivo. Nenhum campo obrigatório existente muda de forma, nenhum tipo
existente muda de shape. O wire já emitido por `git-local`, `uptime-kuma` e `openfortivpn-vpn`
valida contra os schemas de `v0.4` sem uma única alteração da parte deles — **mas** os três precisam
migrar a constante `PROTOCOL_VERSION` para `"0.4"` mesmo assim, porque a série `0.x` exige igualdade
exata (`ProtocolVersion::is_compatible_with`). Ver D2.

## `widget.schema.json`

1. Novo `$defs.ContainerState`:

   ```json
   {
     "type": "string",
     "enum": ["created", "restarting", "running", "removing", "paused", "exited", "dead", "unknown"],
     "description": "Estado do container. As sete primeiras são o vocabulário publicado pelo Docker, mapeadas 1:1 do campo `State` de `docker ps`. `unknown` NUNCA é emitido pelo Docker: é produzido pelo plugin ao encontrar um valor fora do vocabulário (FR-012), e implica nenhuma ação acionável."
   }
   ```

2. Novo `$defs.ContainerStatusItem`:

   ```json
   {
     "type": "object",
     "properties": {
       "id":            { "type": "string", "pattern": "^[0-9a-f]{64}$" },
       "name":          { "type": "string", "minLength": 1 },
       "image":         { "type": "string", "minLength": 1 },
       "state":         { "$ref": "#/$defs/ContainerState" },
       "status_text":   { "type": ["string", "null"] },
       "start_action":   { "$ref": ".../handshake.schema.json#/$defs/ActionDeclaration" },
       "stop_action":    { "$ref": ".../handshake.schema.json#/$defs/ActionDeclaration" },
       "restart_action": { "$ref": ".../handshake.schema.json#/$defs/ActionDeclaration" }
     },
     "required": ["id", "name", "image", "state", "status_text",
                  "start_action", "stop_action", "restart_action"],
     "additionalProperties": false
   }
   ```

   Notas normativas que o schema **não** consegue expressar sozinho e que ficam no `description` do
   `$defs` + em teste de contrato (`data-model.md` §1.3, invariantes 3-6):

   - Os três `action.target` são idênticos entre si e iguais a `{"type": "docker-container",
     "id": <o próprio `id` do item>}`.
   - `enabled` de cada ação segue exatamente a matriz de FR-008.
   - `timeout_hint_ms` é sempre presente: `20000` (start), `35000` (stop), `45000` (restart).
   - `status_text` é **auxiliar e humano**; nenhum consumidor pode derivar estado dele.
   - O campo chama-se `status_text` e **não** `status` deliberadamente, para não colidir
     estruturalmente com `MonitorStatusItem` na desambiguação untagged de `WidgetItems`
     (`research.md` D12).

3. `WidgetGetResult.items` — o `anyOf`/`oneOf` ganha uma quarta opção:
   `{"type": "array", "items": {"$ref": "#/$defs/ContainerStatusItem"}}`. Aditivo — as três opções
   existentes (`WidgetItem[]`, `MonitorStatusItem[]`, `VpnStatusItem[]`) permanecem idênticas.

   **Requisito de revisão** (não é código, é disciplina): antes de aceitar uma **quinta** opção
   aqui, repetir a análise de disjunção de `research.md` D12 — as quatro formas atuais só não
   colidem porque seus conjuntos de campos obrigatórios são disjuntos, o que é incidental, não
   garantido pelo desenho.

## `action.schema.json`

`ActionInvokeResult` ganha uma terceira opção no `oneOf` já existente:

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
    },
    {
      "type": "object",
      "properties": { "container": { "$ref": "widget.schema.json#/$defs/ContainerStatusItem" } },
      "required": ["container"],
      "additionalProperties": false
    }
  ]
}
```

Diferentemente de `WidgetGetResult.items`, aqui a desambiguação é **estrutural e sólida**: as três
opções são objetos com chaves de topo disjuntas (`repo` / `vpn_status` / `container`), então nenhuma
depende de ordem de tentativa.

As três ações de container devolvem o `ContainerStatusItem` **inteiro** pós-ação — não só o novo
estado — porque `enabled` das três `ActionDeclaration` muda com o estado e o core MUST NOT
recalculá-lo por conta própria (`protocol/SPEC.md` §5.3, `research.md` D4/D11).

## `error.schema.json`

Estende só o catálogo textual (o `data.reason` já é string aberta — **nenhuma** mudança de forma):

- `-32010` → `docker_unavailable` → resposta de `widget/get` → o binário `docker` está presente no
  `PATH`, mas a consulta de estado não pôde ser satisfeita.
  `data.detail.condition` (REQUIRED) ∈
  `{"daemon_unreachable", "permission_denied", "timeout", "cli_error"}`;
  `data.detail.raw` (OPTIONAL) = stderr truncado.
  O `message` do `ErrorObject` MUST ser a tradução legível **distinta por condição** exigida por
  FR-010 — três remédios diferentes para o usuário (instalar, subir o serviço, entrar no grupo)
  exigem três mensagens diferentes.
- `-32011` → `container_action_failed` → resposta de `action/invoke` → a invocação de
  `docker start|stop|restart` falhou.
  `data.detail.docker_condition` (REQUIRED) ∈
  `{"no_such_container", "container_gone", "permission_denied", "daemon_unreachable", "timeout",
  "cli_error"}`; `data.detail.raw` (OPTIONAL) = stderr truncado.
  Um único código de domínio cobre todas as causas, seguindo o precedente de `-32001`/`fetch_failed`
  (git-local) e `-32009`/`vpn_action_failed` (feature 004): a causa específica vai em `data`, não em
  códigos distintos (`research.md` D5).

Para o binário `docker` ausente do `PATH`, **nenhum código novo**: reaproveita
`-32003`/`exec_unavailable`, exatamente como `git-local` faz para `git` e `openfortivpn-vpn` para
`openfortivpn-gui`.

`-32000`..`-32009` estão inteiramente ocupados no catálogo de `v0.3`; `-32010`/`-32011` são os
próximos livres da faixa de domínio Farol (`-32000` a `-32099`).

## `handshake.schema.json`

Sem mudança de forma. Só o texto de topo (`description`) é atualizado para citar `"0.4"`, o novo
`kind` e o novo plugin de referência — mesmo padrão do texto de topo de `v0.3/widget.schema.json`
citando a evolução a partir de `v0.2`.

`ActionDeclaration` e `ActionTarget` **não mudam**: `{type: "docker-container", id: <ID>}` já cabe
na forma genérica existente.

## `protocol/SPEC.md`

- **§5.2.1** (`kind`s de widget conhecidos): acrescentar a linha de `container-status-grid` à tabela
  existente, na mesma forma das três já lá:

  > `container-status-grid` (novo em v0.4) | `items`: `ContainerStatusItem[]` — um item por
  > container Docker local (incluindo os não-executando), cada um emparelhando identidade
  > (`id`/`name`/`image`), estado (`state`, vocabulário fechado + `unknown`) e **três**
  > `ActionDeclaration` (`start`/`stop`/`restart`) cujo `enabled` o plugin decide a partir do
  > estado. Lista de N itens independentes, uma linha por container — estruturalmente igual a
  > `status-grid`/`monitor-status-grid`, e diferente do singleton `vpn-status`. | `docker-containers`

- **§6.4** (Versionamento): nenhuma mudança de regra — só a nota de "versão atual: `0.4`".
- **§7.2** (`RPC_TIMEOUT_ACTION`): nenhuma mudança de regra. Vale citar, como exemplo já existente
  na seção, que as ações de container declaram `timeout_hint_ms` explicitamente
  (`20000`/`35000`/`45000`) porque o período de graça default de `docker stop` (10 s) torna o
  default de 120 s do core inadequadamente frouxo.
- **§8.2** (Faixa de domínio Farol): acrescentar as duas linhas do catálogo acima à tabela
  existente.
- **§11** (Relação com os JSON Schemas): apontar `v0.4/` como a versão corrente e `v0.3/` como
  congelada.

## Checklist de migração dos plugins existentes (D2)

Mudança mecânica de uma linha em cada — nenhum deles usa nenhum campo novo:

| Arquivo | Mudança |
|---|---|
| `plugins/git-local/main.py` | `PROTOCOL_VERSION = "0.3"` → `"0.4"` |
| `plugins/uptime-kuma/main.py` | idem |
| `plugins/openfortivpn-vpn/main.py` | idem |
| `crates/farol-core/src/plugin_worker.rs` | `CORE_PROTOCOL_VERSION` → `ProtocolVersion { major: 0, minor: 4 }` |
| `crates/farol-protocol/tests/contract_schema_validation.rs` | 4 `include_str!` + 4 `$id` → `protocol/schema/v0.4/` |
| `crates/farol-protocol/tests/schema_boundaries.rs` | idem |

**Atenção de teste** (aprendizado da feature 004): `crates/farol-core/src/e2e_tests.rs` afirma
`PluginState::Ready` para cada plugin de referência contra um `Emulator` real. Enquanto qualquer
plugin ainda declarar `"0.3"` contra um core em `"0.4"`, o handshake resolve para
`Unavailable { VersionIncompatible }` e a suíte quebra em bloco — a feature 004 viu 8 testes caírem
por isso. Some-se a migração dos `include_str!`/`$id` dos dois arquivos de teste de contrato acima.
É por isso que o bump é tarefa *foundational*, e não de polimento. Ver `research.md` D2.
